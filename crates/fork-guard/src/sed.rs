//! `sed -n` with a script that only prints lines: `20,30p`, `/fn main/,/^}/p`, `1p;$p`. Any
//! other command or option is refused, among them the `w`, `W`, `r` and `e` commands, the `w`
//! and `e` flags of `s`, `-i` and `-f`.
//!
//! GNU sed does not end a regex at a `/` inside brackets, as in `/[/]/p`, where this module
//! does. Outside the regexes, an accepted script holds only line numbers, `$`, `+`, `~`, `,`,
//! `;`, spaces and `p`: no `]`. So sed can only end a regex later than this module, at a `/`
//! this module also ends one at, and the next command it reads is `p` all the same.

use super::shell::Word;

/// Whether the arguments of `sed` print lines and nothing else. GNU sed takes options after
/// its operands too, so every word before `--` that starts with `-`, but `-` itself (standard
/// input), is an option.
pub(super) fn prints_only(arguments: &[Word]) -> bool {
    let mut options = Options::default();
    let mut operands = Vec::new();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let text = word.text.as_str();
        if text == "--" {
            operands.extend(words.by_ref());
        } else if text.len() > 1 && text.starts_with('-') {
            if !options.read(text, &mut words) {
                return false;
            }
        } else {
            operands.push(word);
        }
    }
    if options.scripts.is_empty() {
        options.scripts.extend(operands.first().copied());
    }
    options.quiet
        && !options.scripts.is_empty()
        && options
            .scripts
            .iter()
            .all(|script| !script.globs && prints(&script.text))
}

#[derive(Default)]
struct Options<'a> {
    /// `-n`: sed prints only what the script prints.
    quiet: bool,
    /// The scripts given with `-e` or `--expression`.
    scripts: Vec<&'a Word>,
}

impl<'a> Options<'a> {
    /// Reads the option `text`, and its value from `words` when it takes one; `false` when the
    /// option is not one a fork may give.
    fn read(&mut self, text: &'a str, words: &mut impl Iterator<Item = &'a Word>) -> bool {
        match text {
            "--quiet" | "--silent" => self.quiet = true,
            "--regexp-extended" | "--separate" | "--null-data" | "--unbuffered" | "--posix"
            | "--sandbox" => {}
            "--expression" => return self.script(words.next()),
            // A bundle of short options; any other long option reads as one whose first letter,
            // `-`, `bundle` refuses.
            _ => return self.bundle(&text[1..], words),
        }
        true
    }

    /// Reads a bundle of short options, such as `-nE` or `-ne`: `e` takes the rest of the
    /// bundle, or the next word, as a script.
    fn bundle(&mut self, letters: &'a str, words: &mut impl Iterator<Item = &'a Word>) -> bool {
        for (index, letter) in letters.char_indices() {
            match letter {
                'n' => self.quiet = true,
                'E' | 'r' | 's' | 'u' | 'z' => {}
                // A script inside the bundle, as in `-ne20p`, has no word of its own: refuse it
                // rather than track it.
                'e' => return index + 1 == letters.len() && self.script(words.next()),
                _ => return false,
            }
        }
        true
    }

    fn script(&mut self, word: Option<&'a Word>) -> bool {
        self.scripts.extend(word);
        word.is_some()
    }
}

/// Whether the sed script `script` is a list of `p` commands, each with an optional address
/// or range of addresses, separated by `;`.
fn prints(script: &str) -> bool {
    let mut rest = script;
    loop {
        let Some(after) = print_command(rest.trim_start()) else {
            return false;
        };
        match after.trim_start().strip_prefix(';') {
            Some(next) if next.trim().is_empty() => return true,
            Some(next) => rest = next,
            None => return after.trim().is_empty(),
        }
    }
}

/// What follows the `p` command at the start of `text`, after its address or range.
fn print_command(text: &str) -> Option<&str> {
    let rest = match address(text, false) {
        Some(rest) => match rest.strip_prefix(',') {
            Some(end) => address(end, true)?,
            None => rest,
        },
        None => text,
    };
    rest.trim_start().strip_prefix('p')
}

/// What follows the address at the start of `text`, if one is there: a line number, `$` or a
/// `/regex/`; GNU's `first~step`; and for the end of a range, `+count` and `~multiple` too.
fn address(text: &str, end: bool) -> Option<&str> {
    if let Some(regex) = text.strip_prefix('/') {
        return regex_end(regex);
    }
    if let Some(rest) = text.strip_prefix('$') {
        return Some(rest);
    }
    if end && let Some(count) = text.strip_prefix(['+', '~']) {
        return digits(count);
    }
    let rest = digits(text)?;
    Some(rest.strip_prefix('~').and_then(digits).unwrap_or(rest))
}

/// What follows the line number at the start of `text`, if one is there.
fn digits(text: &str) -> Option<&str> {
    let rest = text.trim_start_matches(|c: char| c.is_ascii_digit());
    (rest.len() < text.len()).then_some(rest)
}

/// What follows the `/` that closes the regex `text` starts, a backslash escaping the next
/// character.
fn regex_end(text: &str) -> Option<&str> {
    let mut chars = text.char_indices();
    while let Some((index, character)) = chars.next() {
        match character {
            '/' => return Some(&text[index + 1..]),
            '\\' => {
                chars.next()?;
            }
            _ => {}
        }
    }
    None
}
