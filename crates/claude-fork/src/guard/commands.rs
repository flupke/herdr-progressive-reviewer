//! Which commands of a pipe only read: the listed commands, with their reading subcommands and
//! without the options that write a file or run another program.

use super::sed;
use super::shell::Word;

/// Why a command of a pipe is refused.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// A command, subcommand or option that may write or run another program.
    NotReading,
    /// A jj command without `--ignore-working-copy`, which would snapshot the working copy and
    /// record an operation.
    Snapshot,
}

/// Commands a fork may run with any arguments but the writing options.
const PLAIN: &[&str] = &[
    "rg", "grep", "ls", "cat", "head", "tail", "wc", "find", "pwd", "nl", "cut", "diff",
];
/// The jj subcommands a fork may run, each as its words.
const JJ_SUBCOMMANDS: &[&[&str]] = &[
    &["log"],
    &["diff"],
    &["show"],
    &["st"],
    &["status"],
    &["evolog"],
    &["interdiff"],
    &["root"],
    &["file", "show"],
    &["file", "list"],
];
/// An option a command takes before its subcommand.
enum Global {
    Flag(&'static str),
    /// An option followed by its value, in the next word or after `=`.
    Valued(&'static str),
}

/// The jj options a fork may give before the subcommand.
const JJ_GLOBALS: &[Global] = &[
    Global::Flag("--ignore-working-copy"),
    Global::Flag("--no-pager"),
    Global::Flag("--quiet"),
    Global::Valued("--at-op"),
    Global::Valued("--at-operation"),
    Global::Valued("--color"),
];
/// Git options that are prefixes of a writing option but options of their own.
const GIT_READING_PREFIXES: &[&str] = &["--text", "--filter"];
const GIT_SUBCOMMANDS: &[&str] = &[
    "log",
    "diff",
    "show",
    "status",
    "blame",
    "ls-files",
    "cat-file",
    "rev-parse",
    "grep",
    "merge-base",
];
/// The git options a fork may give before the subcommand: never `-c`, which sets the pager or
/// a diff program, nor `-C`, which reads another repository's configuration.
const GIT_GLOBALS: &[Global] = &[
    Global::Flag("--no-pager"),
    Global::Flag("-P"),
    Global::Flag("--no-optional-locks"),
];
/// Options of the listed commands that write a file, run another program, or change the
/// configuration that could run one.
const WRITING_OPTIONS: &[&str] = &[
    "--config",
    "--config-file",
    "--config-toml",
    "--tool",
    "--pre",
    "--hostname-bin",
    "--output",
    "--open-files-in-pager",
    "--ext-diff",
    "--textconv",
    "--filters",
    "-exec",
    "-execdir",
    "-delete",
    "-ok",
    "-okdir",
    "-fprint",
    "-fprint0",
    "-fprintf",
    "-fls",
];

/// Why the command `words` (its name first) is refused; `None` when it only reads.
pub(super) fn refusal(words: &[Word]) -> Option<Refusal> {
    let (name, arguments) = words.split_first()?;
    let reads = match name.text.as_str() {
        _ if arguments.iter().any(|word| writes(&word.text)) => false,
        "jj" => return jj_refusal(arguments),
        "git" => {
            !arguments.iter().any(|word| git_writes(&word.text))
                && subcommand(arguments, GIT_GLOBALS)
                    .is_some_and(|rest| GIT_SUBCOMMANDS.contains(&rest[0].text.as_str()))
        }
        "sed" => sed::prints_only(arguments),
        name => PLAIN.contains(&name),
    };
    (!reads).then_some(Refusal::NotReading)
}

/// Whether `word` is one of the writing options.
fn writes(word: &str) -> bool {
    WRITING_OPTIONS.contains(&option_name(word))
}

/// Whether git would take `word` for a writing option: a prefix of a long one, which git takes
/// for that option when no other of the subcommand's options starts with it, or a bundle of
/// short options with `-O`, which opens git grep's matches in a program.
fn git_writes(word: &str) -> bool {
    let name = option_name(word);
    let abbreviation = name.len() > 2
        && name.starts_with("--")
        && !GIT_READING_PREFIXES.contains(&name)
        && WRITING_OPTIONS
            .iter()
            .any(|option| option.starts_with(name));
    abbreviation || bundles(word, 'O')
}

/// Whether `word` is a bundle of short options, as `-nO` or `-3O`, that holds the option
/// `letter`, which takes the rest of the word as its value.
fn bundles(word: &str, letter: char) -> bool {
    word.strip_prefix('-').is_some_and(|bundle| {
        bundle
            .trim_start_matches(|other: char| other.is_ascii_alphanumeric() && other != letter)
            .starts_with(letter)
    })
}

/// The option `word` names, without the value it gives after `=`.
fn option_name(word: &str) -> &str {
    word.split_once('=').map_or(word, |(name, _)| name)
}

fn jj_refusal(arguments: &[Word]) -> Option<Refusal> {
    let reads = subcommand(arguments, JJ_GLOBALS).is_some_and(|rest| {
        JJ_SUBCOMMANDS.iter().any(|path| {
            rest.len() >= path.len() && path.iter().zip(rest).all(|(name, word)| word.text == *name)
        })
    });
    // jj takes its global options after the subcommand too: another repository could run a
    // program from its configuration.
    let repository =
        |word: &Word| bundles(&word.text, 'R') || option_name(&word.text) == "--repository";
    if !reads || arguments.iter().any(repository) {
        return Some(Refusal::NotReading);
    }
    let options = arguments.iter().take_while(|word| word.text != "--");
    (!options
        .into_iter()
        .any(|word| word.text == "--ignore-working-copy"))
    .then_some(Refusal::Snapshot)
}

/// The arguments from the subcommand on, after the options `globals` allows before it; `None`
/// when another option or no subcommand comes.
fn subcommand<'a>(arguments: &'a [Word], globals: &[Global]) -> Option<&'a [Word]> {
    let mut index = 0;
    while let Some(word) = arguments.get(index) {
        if !word.text.starts_with('-') {
            return Some(&arguments[index..]);
        }
        index += match word.text.split_once('=') {
            Some((name, _)) => globals
                .iter()
                .any(|global| matches!(global, Global::Valued(valued) if *valued == name))
                .then_some(1)?,
            None => match globals.iter().find(|global| global.name() == word.text)? {
                Global::Flag(_) => 1,
                Global::Valued(_) => 2,
            },
        };
    }
    None
}

impl Global {
    fn name(&self) -> &'static str {
        match self {
            Global::Flag(name) | Global::Valued(name) => name,
        }
    }
}
