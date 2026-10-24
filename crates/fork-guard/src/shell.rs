//! The words of a fork's shell command, split as Bash and Zsh split them: quotes and
//! backslashes removed, and the commands of a pipe apart. Any other shell form outside quotes
//! (redirections, command lists, substitutions, expansions) is refused, so that what the guard
//! reads is what the shell runs.
//!
//! The split is written here because the shell-word crates (`shell-words`, `shlex`) treat
//! operators as word characters and drop which characters were quoted, so they cannot tell
//! `rg '|'` from `rg |`.

use std::str::Chars;

/// One shell word, its quotes and backslashes removed.
#[derive(Debug, Default)]
pub(super) struct Word {
    pub(super) text: String,
    /// Whether the word holds a glob character outside quotes, which the shell may replace
    /// with file names.
    pub(super) globs: bool,
}

/// Why a command cannot be split.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ShellError {
    /// A character that starts a shell form, outside quotes (or `$` and the backtick inside
    /// double quotes).
    Form(char),
    /// A glob at the start of a word or in an option, which file names could turn into options.
    LeadingGlob(char),
    /// A newline or another control character, inside quotes or out; a tab outside quotes
    /// separates words.
    Control,
    /// A quote that is not closed, or a backslash that ends the command.
    Unclosed,
    /// A pipe with no command on one side, or an empty command.
    Empty,
}

/// Characters that start a shell form outside quotes.
const FORMS: &[char] = &[';', '&', '>', '<', '$', '`', '(', ')', '{', '}'];
/// Where a command may send its error messages: `2>` followed by one of these.
const STDERR_TARGETS: &[&str] = &["/dev/null", "&1"];
/// Characters that make a word a pattern of file names outside quotes, in Bash or in Zsh with
/// `EXTENDED_GLOB`.
const GLOBS: &[char] = &['*', '?', '[', '~', '^', '#'];

/// The commands of the pipe `command`, each as its words.
pub(super) fn split_pipe(command: &str) -> Result<Vec<Vec<Word>>, ShellError> {
    let mut lexer = Lexer {
        chars: command.chars(),
        commands: Vec::new(),
        words: Vec::new(),
        word: None,
        quoted: false,
    };
    while let Some(character) = lexer.chars.next() {
        lexer.unquoted(character)?;
    }
    lexer.end_command()?;
    Ok(lexer.commands)
}

struct Lexer<'a> {
    chars: Chars<'a>,
    commands: Vec<Vec<Word>>,
    words: Vec<Word>,
    /// The word being read; `Some` once a character or a quote started it, so `''` is a word.
    word: Option<Word>,
    /// Whether a quote or a backslash is part of the word being read.
    quoted: bool,
}

impl Lexer<'_> {
    fn unquoted(&mut self, character: char) -> Result<(), ShellError> {
        match character {
            ' ' | '\t' => self.end_word(),
            '|' => self.end_command()?,
            '>' => self.stderr_redirection()?,
            '\'' => self.single_quoted()?,
            '"' => self.double_quoted()?,
            '\\' => {
                self.quoted = true;
                let escaped = self.next_literal()?;
                self.push(escaped);
            }
            '~' if self.word.is_none() => self.push(character),
            _ if GLOBS.contains(&character) => self.glob(character)?,
            _ if FORMS.contains(&character) => return Err(ShellError::Form(character)),
            _ => self.push(non_control(character)?),
        }
        Ok(())
    }

    /// Reads up to the closing single quote: every character inside is literal.
    fn single_quoted(&mut self) -> Result<(), ShellError> {
        self.word.get_or_insert_default();
        self.quoted = true;
        loop {
            match self.next_literal()? {
                '\'' => return Ok(()),
                character => self.push(character),
            }
        }
    }

    /// Reads up to the closing double quote: a backslash escapes `$`, the backtick, `"` and
    /// itself only, and `$` or a backtick would start a substitution.
    fn double_quoted(&mut self) -> Result<(), ShellError> {
        self.word.get_or_insert_default();
        self.quoted = true;
        loop {
            match self.next_literal()? {
                '"' => return Ok(()),
                '\\' => match self.next_literal()? {
                    escaped @ ('$' | '`' | '"' | '\\') => self.push(escaped),
                    other => {
                        self.push('\\');
                        self.push(other);
                    }
                },
                // `$` before the closing quote is a literal dollar sign.
                '$' if self.chars.as_str().starts_with('"') => self.push('$'),
                form @ ('$' | '`') => return Err(ShellError::Form(form)),
                character => self.push(character),
            }
        }
    }

    /// Takes `2>/dev/null` or `2>&1`, whose unquoted `2` is the word read so far: they only discard
    /// error messages or mix them into the output. Any other redirection is refused.
    fn stderr_redirection(&mut self) -> Result<(), ShellError> {
        let rest = self.chars.as_str();
        let target = STDERR_TARGETS.iter().find(|target| {
            rest.strip_prefix(**target)
                .is_some_and(|after| after.is_empty() || after.starts_with([' ', '\t', '|']))
        });
        match target {
            Some(target)
                if !self.quoted && self.word.as_ref().is_some_and(|word| word.text == "2") =>
            {
                self.word = None;
                self.chars = rest[target.len()..].chars();
                Ok(())
            }
            _ => Err(ShellError::Form('>')),
        }
    }

    /// Takes a glob character, which only a word that already starts with a literal
    /// character other than `-` may hold: the file names it matches then start with that
    /// character, and none can be an option. Nor `~`, which Zsh expands as `~[name]` with a
    /// function of the user's.
    fn glob(&mut self, character: char) -> Result<(), ShellError> {
        match &mut self.word {
            Some(word) if !word.text.is_empty() && !word.text.starts_with(['-', '~']) => {
                word.text.push(character);
                word.globs = true;
                Ok(())
            }
            _ => Err(ShellError::LeadingGlob(character)),
        }
    }

    /// The next character of the command, refused when it is a control character or missing.
    fn next_literal(&mut self) -> Result<char, ShellError> {
        non_control(self.chars.next().ok_or(ShellError::Unclosed)?)
    }

    fn push(&mut self, character: char) {
        self.word.get_or_insert_default().text.push(character);
    }

    fn end_word(&mut self) {
        self.quoted = false;
        self.words.extend(self.word.take());
    }

    fn end_command(&mut self) -> Result<(), ShellError> {
        self.end_word();
        if self.words.is_empty() {
            return Err(ShellError::Empty);
        }
        self.commands.push(std::mem::take(&mut self.words));
        Ok(())
    }
}

/// `character`, unless it is a newline or another control character.
fn non_control(character: char) -> Result<char, ShellError> {
    if character.is_control() {
        Err(ShellError::Control)
    } else {
        Ok(character)
    }
}

#[cfg(test)]
#[path = "shell.tests.rs"]
mod tests;
