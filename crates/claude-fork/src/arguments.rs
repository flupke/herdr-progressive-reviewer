//! The pane agent's arguments as a fork keeps them: without those that pick a session, an
//! interactive mode or an output format, and without a prompt given on the command line. An
//! option this table does not know stops the fork: a fork starts with the agent's exact flags
//! or not at all.

/// How many values an option of `claude` takes.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Values {
    None,
    One,
    /// One, when the next argument is not an option.
    Optional,
    /// Every following argument that is not an option.
    Many,
}

/// What a fork does with an option of the pane agent's command line.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Fate {
    /// The fork keeps it, with its values: it shapes the agent and its prompt cache.
    Kept,
    /// The fork drops it: it picks a session, an output, an interactive mode or the model,
    /// which the fork sets itself.
    Dropped,
    /// The fork cannot start with it: run-ahead does not fork the agent.
    Refused,
}

/// Every option of `claude` (2.1), with its values and what a fork does with it.
const OPTIONS: &[(&[&str], Values, Fate)] = &[
    (&["--add-dir"], Values::Many, Fate::Kept),
    (&["--agent"], Values::One, Fate::Kept),
    (&["--agents"], Values::One, Fate::Kept),
    (
        &["--allow-dangerously-skip-permissions"],
        Values::None,
        Fate::Kept,
    ),
    (
        &["--allowedTools", "--allowed-tools"],
        Values::Many,
        Fate::Kept,
    ),
    (&["--append-system-prompt"], Values::One, Fate::Kept),
    (&["--append-system-prompt-file"], Values::One, Fate::Kept),
    (&["--autocompact"], Values::One, Fate::Kept),
    (&["--ax-screen-reader"], Values::None, Fate::Kept),
    (&["--bg", "--background"], Values::None, Fate::Dropped),
    (&["--bare"], Values::None, Fate::Kept),
    (&["--betas"], Values::Many, Fate::Kept),
    (&["--brief"], Values::None, Fate::Kept),
    (&["--chrome"], Values::None, Fate::Kept),
    (&["--cloud"], Values::Optional, Fate::Dropped),
    (&["--continue", "-c"], Values::None, Fate::Dropped),
    (
        &["--dangerously-skip-permissions"],
        Values::None,
        Fate::Kept,
    ),
    (&["--debug", "-d"], Values::Optional, Fate::Dropped),
    (&["--debug-file"], Values::One, Fate::Dropped),
    (&["--desktop"], Values::None, Fate::Dropped),
    (&["--disable-slash-commands"], Values::None, Fate::Kept),
    (
        &["--disallowedTools", "--disallowed-tools"],
        Values::Many,
        Fate::Kept,
    ),
    (&["--effort"], Values::One, Fate::Kept),
    (&["--environment"], Values::One, Fate::Dropped),
    (
        &["--exclude-dynamic-system-prompt-sections"],
        Values::None,
        Fate::Kept,
    ),
    (&["--fallback-model"], Values::One, Fate::Kept),
    (&["--file"], Values::Many, Fate::Dropped),
    (&["--fork-session"], Values::None, Fate::Dropped),
    (&["--forward-subagent-text"], Values::None, Fate::Dropped),
    (&["--from-pr"], Values::Optional, Fate::Dropped),
    (&["--ide"], Values::None, Fate::Dropped),
    (&["--include-hook-events"], Values::None, Fate::Dropped),
    (&["--include-partial-messages"], Values::None, Fate::Dropped),
    (&["--input-format"], Values::One, Fate::Dropped),
    (&["--json-schema"], Values::One, Fate::Dropped),
    (&["--max-budget-usd"], Values::One, Fate::Kept),
    (&["--max-turns"], Values::One, Fate::Kept),
    (&["--mcp-config"], Values::Many, Fate::Kept),
    (&["--model"], Values::One, Fate::Dropped),
    (&["--name", "-n"], Values::One, Fate::Dropped),
    (&["--no-chrome"], Values::None, Fate::Kept),
    (&["--no-session-persistence"], Values::None, Fate::Dropped),
    (&["--output-format"], Values::One, Fate::Dropped),
    (&["--permission-mode"], Values::One, Fate::Kept),
    (&["--permission-prompt-tool"], Values::One, Fate::Kept),
    (&["--permission-prompts"], Values::One, Fate::Dropped),
    (&["--plugin-dir"], Values::One, Fate::Kept),
    (&["--plugin-url"], Values::One, Fate::Kept),
    (&["--print", "-p"], Values::None, Fate::Dropped),
    (&["--prompt-suggestions"], Values::Optional, Fate::Dropped),
    (&["--remote-control"], Values::Optional, Fate::Dropped),
    (
        &["--remote-control-session-name-prefix"],
        Values::One,
        Fate::Dropped,
    ),
    (&["--replay-user-messages"], Values::None, Fate::Dropped),
    (&["--restricted"], Values::None, Fate::Kept),
    (&["--resume", "-r"], Values::Optional, Fate::Dropped),
    (&["--safe-mode"], Values::None, Fate::Kept),
    (&["--session-id"], Values::One, Fate::Dropped),
    (&["--setting-sources"], Values::One, Fate::Kept),
    // A fork brings its own settings, with its guard: two would replace one another.
    (&["--settings"], Values::One, Fate::Refused),
    (&["--strict-mcp-config"], Values::None, Fate::Kept),
    (&["--system-prompt"], Values::One, Fate::Kept),
    (&["--system-prompt-file"], Values::One, Fate::Kept),
    (&["--system-prompt-snapshot"], Values::One, Fate::Kept),
    (&["--teleport"], Values::Optional, Fate::Dropped),
    (&["--tmux"], Values::None, Fate::Dropped),
    (&["--tools"], Values::Many, Fate::Kept),
    (&["--verbose"], Values::None, Fate::Dropped),
    (&["--worktree", "-w"], Values::Optional, Fate::Dropped),
];

/// The arguments of the pane agent's command line, after its program, that a fork keeps, and
/// the model they name, if any; or why the agent cannot be forked with its exact flags.
pub(crate) fn kept(arguments: &[String]) -> Result<(Vec<String>, Option<String>), String> {
    let mut kept = Vec::new();
    let mut model = None;
    let mut rest = arguments.iter().peekable();
    while let Some(argument) = rest.next() {
        // A value outside an option is a prompt: the fork's prompt comes on its input.
        if !argument.starts_with('-') || argument == "-" {
            continue;
        }
        let (option, inline) = match argument.split_once('=') {
            Some((option, value)) if option.starts_with("--") => (option, Some(value)),
            _ => (argument.as_str(), None),
        };
        let Some(&(_, values, fate)) = OPTIONS.iter().find(|(names, ..)| names.contains(&option))
        else {
            return Err(format!(
                "the agent's option {option} is unknown to run-ahead"
            ));
        };
        let taken = if inline.is_none() {
            option_values(values, &mut rest)
        } else {
            Vec::new()
        };
        if option == "--model" {
            model = inline.map(str::to_owned).or_else(|| taken.first().cloned());
        }
        match fate {
            Fate::Kept => {
                kept.push(argument.clone());
                kept.extend(taken);
            }
            Fate::Dropped => {}
            Fate::Refused => {
                return Err(format!(
                    "a fork cannot start with the agent's option {option}"
                ));
            }
        }
    }
    Ok((kept, model))
}

/// The values of an option that takes `values`, taken from `rest`.
fn option_values<'a>(
    values: Values,
    rest: &mut std::iter::Peekable<impl Iterator<Item = &'a String>>,
) -> Vec<String> {
    let more = |next: &&String| !next.starts_with('-');
    let mut taken = Vec::new();
    match values {
        Values::None => {}
        Values::One => taken.extend(rest.next().cloned()),
        Values::Optional => taken.extend(rest.next_if(more).cloned()),
        Values::Many => {
            while let Some(value) = rest.next_if(more) {
                taken.push(value.clone());
            }
        }
    }
    taken
}

#[cfg(test)]
#[path = "arguments.tests.rs"]
mod tests;
