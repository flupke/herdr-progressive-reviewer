//! Claude Code's input box, read off the agent's screen with its styles: an empty box shows a
//! dim placeholder or a suggested prompt, which is not text the reviewer typed.

/// The text typed in Claude Code's input box, from its screen `screen` with ANSI styles; dim
/// text does not count. `None` when the screen shows no input box.
pub(crate) fn input_box_text(screen: &str) -> Option<String> {
    screen.lines().rev().find_map(|line| {
        let at = line.find(PROMPT)?;
        let before = plain(&line[..at]);
        if !before.chars().all(|c| c.is_whitespace() || c == BORDER) {
            return None;
        }
        Some(
            undimmed(&line[at + PROMPT.len_utf8()..])
                .trim_matches(|c: char| c.is_whitespace() || c == BORDER)
                .to_owned(),
        )
    })
}

/// The mark at the start of the input box.
const PROMPT: char = '❯';
/// The side of a box drawn around it.
const BORDER: char = '│';

/// `text` without its escape sequences.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            skip_sequence(&mut chars);
        } else {
            out.push(c);
        }
    }
    out
}

/// The characters of `text` that are not drawn dim, without its escape sequences.
fn undimmed(text: &str) -> String {
    let mut out = String::new();
    let mut dim = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if !dim {
                out.push(c);
            }
            continue;
        }
        let (parameters, end) = skip_sequence(&mut chars);
        if end != Some('m') {
            continue;
        }
        for parameter in parameters.split(';') {
            match parameter {
                "" | "0" | "22" => dim = false,
                "2" => dim = true,
                _ => {}
            }
        }
    }
    out
}

/// Skips one escape sequence after its ESC; returns the parameters and final character of a
/// control sequence.
fn skip_sequence(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> (String, Option<char>) {
    if chars.peek() != Some(&'[') {
        chars.next();
        return (String::new(), None);
    }
    chars.next();
    let mut parameters = String::new();
    for c in chars.by_ref() {
        if ('@'..='~').contains(&c) {
            return (parameters, Some(c));
        }
        parameters.push(c);
    }
    (parameters, None)
}

#[cfg(test)]
#[path = "screen.tests.rs"]
mod tests;
