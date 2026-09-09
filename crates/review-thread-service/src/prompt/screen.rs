use ansi_to_tui::IntoText;
use ratatui::{
    style::{Color, Modifier},
    text::{Line, Span, Text},
};

pub(super) struct ComposerScreen(Text<'static>);

impl ComposerScreen {
    pub(super) fn parse(screen: &str) -> Option<Self> {
        screen.into_text().ok().map(Self)
    }

    pub(super) fn text(&self) -> String {
        self.0.to_string()
    }

    pub(super) fn empty_codex_placeholder(&self) -> bool {
        let lines = &self.0.lines;
        let Some(index) = lines
            .iter()
            .rposition(|line| super::PromptGate::prompt(&line.to_string()).is_some())
        else {
            return false;
        };
        let Some(placeholder) = CodexPlaceholder::from_line(&lines[index]) else {
            return false;
        };
        let mut start = index;
        while start > 0 && placeholder.has_background(&lines[start - 1]) {
            start -= 1;
            if !placeholder.decoration(&lines[start]) {
                return false;
            }
        }
        let above = lines[..start]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if super::PromptGate::has_input_above(&above.iter().map(String::as_str).collect::<Vec<_>>())
        {
            return false;
        }
        let mut end = index + 1;
        while end < lines.len() && placeholder.has_background(&lines[end]) {
            if !placeholder.decoration(&lines[end]) {
                return false;
            }
            end += 1;
        }
        lines[end..].iter().all(|line| {
            let text = line.to_string();
            let text = text.trim();
            text.is_empty()
                || text == "? for shortcuts"
                || (text.starts_with("gpt-")
                    && text.contains(" · Ready · ")
                    && (text.ends_with("Vim: Normal") || text.ends_with("Vim: Insert")))
        })
    }
}

struct CodexPlaceholder {
    background: Color,
}

impl CodexPlaceholder {
    fn from_line(line: &Line<'_>) -> Option<Self> {
        // Plain text cannot distinguish this native hint from a typed draft.
        // Require its dim styling and validate the rest of the composer too.
        let index = line.spans.iter().position(|span| {
            span.content == "Ask Codex to do anything"
                && span.style.add_modifier.contains(Modifier::DIM)
        })?;
        let prefix = line.spans[..index]
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        if prefix != "› " {
            return None;
        }
        let placeholder = Self {
            background: line.spans[index].style.bg?,
        };
        line.spans[index + 1..]
            .iter()
            .all(|span| placeholder.decoration_span(span))
            .then_some(placeholder)
    }

    fn has_background(&self, line: &Line<'_>) -> bool {
        line.spans
            .iter()
            .any(|span| span.style.bg == Some(self.background))
    }

    fn decoration(&self, line: &Line<'_>) -> bool {
        line.spans.iter().all(|span| self.decoration_span(span))
    }

    fn decoration_span(&self, span: &Span<'_>) -> bool {
        // Codex's empty-composer animation uses gray RGB braille cells. Typed
        // braille has the input text style and must not be discarded here.
        let sparkle = span.style.bg == Some(self.background)
            && matches!(span.style.fg, Some(Color::Rgb(r, g, b)) if r == g && g == b);
        span.content.chars().all(|character| {
            character.is_whitespace() || (sparkle && ('\u{2800}'..='\u{28ff}').contains(&character))
        })
    }
}
