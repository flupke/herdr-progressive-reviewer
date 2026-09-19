use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentPrompter, AgentStatus, HerdrReader};

mod screen;

/// Defer notifications when the reviewer may be composing input.
///
/// Herdr currently has no atomic "prompt only if empty" operation. Check focus
/// again after reading the screen, and fail closed for unrecognized composers.
pub(super) struct PromptGate;

impl PromptGate {
    /// The only reviewer-to-agent send: transient readiness defers without losing the request.
    pub(super) fn send(
        client: &HerdrClient,
        agent: &Agent,
        text: &str,
        still_pending: impl FnOnce() -> bool,
    ) -> Result<bool, String> {
        Self::send_observed(client, agent, text, || Ok(still_pending()))
            .map_err(|error| error.to_string())
    }

    pub(super) fn send_observed(
        client: &HerdrClient,
        agent: &Agent,
        text: &str,
        before_attempt: impl FnOnce() -> Result<bool, crate::PromptError>,
    ) -> Result<bool, crate::PromptError> {
        if !matches!(agent.agent_status, AgentStatus::Idle | AgentStatus::Done)
            || !Self::ready(client, agent)
            || !before_attempt()?
        {
            return Ok(false);
        }
        client
            .prompt_agent(&agent.pane_id, text)
            .map_err(|error| crate::PromptError::Unknown(error.to_string()))?;
        Ok(true)
    }

    fn ready(client: &HerdrClient, agent: &Agent) -> bool {
        let Ok(snapshot) = client.session_snapshot() else {
            return false;
        };
        if snapshot.focused_pane_id.as_ref() == Some(&agent.pane_id) {
            return false;
        }
        client
            .read_agent_screen_ansi(&agent.pane_id)
            .is_ok_and(|screen| Self::empty(&screen))
            && client
                .session_snapshot()
                .is_ok_and(|snapshot| snapshot.focused_pane_id.as_ref() != Some(&agent.pane_id))
    }

    fn empty(screen: &str) -> bool {
        let Some(screen) = screen::ComposerScreen::parse(screen) else {
            return false;
        };
        Self::empty_text(&screen.text()) || screen.empty_codex_placeholder()
    }

    fn empty_text(screen: &str) -> bool {
        let lines: Vec<_> = screen.lines().collect();
        let Some((index, prompt)) = lines
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, line)| Self::prompt(line).map(|text| (index, text)))
        else {
            return false;
        };
        if !prompt.trim().is_empty() || Self::has_input_above(&lines[..index]) {
            return false;
        }
        for line in &lines[index + 1..] {
            let line = line.trim_end();
            if line.trim().is_empty() {
                continue;
            }
            if line
                .chars()
                .all(|character| matches!(character, '─' | '╰' | '╯' | '└' | '┘'))
            {
                return true;
            }
            if line == "? for shortcuts" || line == "? for shortcuts · esc to interrupt" {
                return true;
            }
            return false;
        }
        true
    }

    fn prompt(line: &str) -> Option<&str> {
        // Native prompt markers occupy the left margin. Wrapped/pasted input
        // is indented; stripping that indentation could submit an existing draft.
        let line = line.trim_end();
        ['›', '❯'].into_iter().find_map(|marker| {
            let text = line.strip_prefix(marker)?;
            (text.is_empty() || text.starts_with(' ')).then_some(text)
        })
    }

    fn has_input_above(lines: &[&str]) -> bool {
        // Attachment chips can precede an otherwise empty input line. Also
        // reject adjacent prompt markers instead of guessing which is current.
        lines
            .iter()
            .rev()
            .find(|line| !line.trim().is_empty())
            .is_some_and(|line| {
                let text = line.trim();
                Self::prompt(line).is_some()
                    || text.starts_with("[Image")
                    || text.starts_with("[image")
                    || text.starts_with("[Attachment")
            })
    }
}

#[cfg(test)]
mod tests {
    use super::PromptGate;

    #[test]
    fn codex_placeholder_and_sparkles_are_not_unposted_text() {
        let screen = include_str!("prompt/codex-empty.ansi");
        assert!(!PromptGate::empty_text(
            &super::screen::ComposerScreen::parse(screen).unwrap().text()
        ));
        assert!(PromptGate::empty(screen));
    }

    #[test]
    fn codex_placeholder_without_a_background_is_not_unposted_text() {
        let screen = include_str!("prompt/codex-plain-empty.ansi");
        assert!(PromptGate::empty(screen));
        for occupied in [
            screen.replace("\x1b[2m", ""),
            screen.replace("Ask Codex to do anything", "unfinished prompt"),
            screen.replace("Ask Codex to do anything", "Ask Codex to do anything extra"),
            screen.replace("anything\x1b[0m", "anything\x1b[0m draft"),
            screen.replace("anything\x1b[0m", "anything\x1b[0m\n  continuation"),
            format!("[Image #1]\n{screen}"),
            format!("[Attachment: notes]\n{screen}"),
            screen.replace("Ready", "unknown footer"),
        ] {
            assert!(!PromptGate::empty(&occupied), "{occupied:?}");
        }
    }

    #[test]
    fn codex_placeholder_recognition_still_rejects_drafts_and_attachments() {
        let screen = include_str!("prompt/codex-empty.ansi");
        for occupied in [
            screen.replace("\x1b[2m", ""), // The same words typed as normal input.
            screen.replace("Ask Codex to do anything", "unfinished prompt"),
            screen.replace("Ask Codex to do anything", "Ask Codex to do anything extra"),
            format!("[Image #1]\n{screen}"),
            screen.replacen('\n', "\n\x1b[48;2;30;30;30m[Attachment: notes]\x1b[0m\n", 1),
            screen.replacen(
                '\n',
                "\n\x1b[48;2;30;30;30m  continuation of a draft\x1b[0m\n",
                1,
            ),
            screen.replace(" · Ready · ", " · unknown footer · "),
        ] {
            assert!(!PromptGate::empty(&occupied), "{occupied:?}");
        }
    }

    #[test]
    fn stock_codex_footer_allows_only_its_styled_empty_composer() {
        // Captured from Codex 0.154.0 with the default status line.
        let screen = "\x1b[1m›\x1b[0m \x1b[2mAsk Codex to do anything\x1b[0m\n \n  gpt-6-astra xhigh · /tmp/explore…\n";
        assert!(PromptGate::empty(screen));
        for occupied in [
            screen.replace("\x1b[2m", ""),
            screen.replace("anything\x1b[0m", "anything\x1b[0m extra"),
            format!("{screen}  pasted continuation\n"),
            format!("[Image #1]\n{screen}"),
        ] {
            assert!(!PromptGate::empty(&occupied), "{occupied:?}");
        }
    }

    #[test]
    fn only_a_visible_empty_composer_allows_an_automatic_prompt() {
        for screen in ["answer\n› \n", "────\n❯ \n────\n? for shortcuts"] {
            assert!(PromptGate::empty(screen), "{screen}");
        }
        for screen in [
            "",
            "working",
            "› unfinished prompt",
            "❯ [Image #1]",
            "›\n  second line\n────",
            "│ > parked draft │\n╰────╯",
            "› unknown placeholder\n? for shortcuts",
            "› unfinished prompt\n  >\n────",
            "› unfinished prompt\n  ›\n────",
            "❯ unfinished prompt\n  ❯\n────",
            "› unfinished prompt\n›\n────",
            "›\n  ────\n  still part of the draft",
            "  [Image #1]\n\n›\n? for shortcuts",
            "  [Attachment: diagram.png]\n❯\n────",
            "│ >      │\n╰────────╯",
            "╭────────────╮\n│ [Image #1] │\n│ >         │\n╰────────────╯",
        ] {
            assert!(!PromptGate::empty(screen), "{screen}");
        }
    }
}
