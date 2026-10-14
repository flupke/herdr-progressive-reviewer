//! The files of a turn: the rule files the round's setup needs, the turn's own data, and the
//! index that says which to read now and when to read each of the others.

use std::fmt::Write as _;

use markdown_marks::{Callout, Mark, StatusMark};
use review_explore::{ChangeAuthor, Comparison, TurnRequest};
use review_explore_round_settings::WritingStyle;
use review_repository::repository::SnapshotIdentity;

use crate::INDEX;

/// One file of a turn's directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnFile {
    pub name: &'static str,
    pub text: String,
}

/// The file of the turn's own data: its identity, and the reviewer's answer after the first.
const TURN: &str = "turn.md";
/// The file of the round's data, the same on every turn, which the index describes.
const ROUND: &str = "round.md";

/// The heading of the index's list of the files to read at the start of the turn.
const READ_NOW: &str = "## Read now";
/// The heading of the index's list of the files to read once the agent reaches what each covers.
pub(crate) const READ_LATER: &str = "## Read when you need it";

/// When the agent reads a file, as the index says.
enum When {
    /// At the start of the turn; the text says what the file holds.
    Now(&'static str),
    /// Once the agent reaches what the file covers, which the text names.
    On(&'static str),
}

struct Rule {
    name: &'static str,
    text: String,
    when: When,
}

impl Rule {
    fn new(name: &'static str, text: impl Into<String>, when: When) -> Self {
        Self {
            name,
            text: text.into(),
            when,
        }
    }
}

/// The files of the turn `request` of a round on `comparison`, whose own data is `input` and
/// whose round's data is `round`: the index, the turn's and the round's data, then the rules the
/// round's setup needs that the index does not hold.
pub(super) fn files(
    request: &TurnRequest,
    comparison: &Comparison,
    input: &str,
    round: &str,
) -> Vec<TurnFile> {
    let rules = rules(request);
    let mut files = vec![
        TurnFile {
            name: INDEX,
            text: index(request, comparison, &rules),
        },
        TurnFile {
            name: TURN,
            text: format!("{}\n{input}", include_str!("rules/turn.md")),
        },
        TurnFile {
            name: ROUND,
            text: format!("{}\n{round}", include_str!("rules/round.md")),
        },
    ];
    files.extend(rules.into_iter().map(|rule| TurnFile {
        name: rule.name,
        text: rule.text,
    }));
    files
}

/// The rule files of the turn `request` beside its index: only those its round's setup needs.
fn rules(request: &TurnRequest) -> Vec<Rule> {
    let drawing_board = request.at_drawing_board();
    let mut rules = vec![if request.is_kickoff() {
        Rule::new(
            "first-turn.md",
            include_str!("rules/first-turn.md"),
            When::Now("the steps of this turn"),
        )
    } else if drawing_board {
        Rule::new(
            "drawing-board.md",
            include_str!("rules/drawing-board.md"),
            When::Now("the steps of this turn, which ends the round"),
        )
    } else {
        Rule::new(
            "next-turn.md",
            include_str!("rules/next-turn.md"),
            When::Now("the steps of this turn, and how to take up the reviewer's answer"),
        )
    }];
    if request.author == ChangeAuthor::Other {
        rules.push(Rule::new(
            "others-change.md",
            include_str!("rules/others-change.md"),
            When::Now("how a review of someone else's change differs, which overrides the rest"),
        ));
    }
    // At the drawing board, the agent concludes alone.
    if request.challenger && !drawing_board {
        rules.push(Rule::new(
            "challenger.md",
            include_str!("rules/challenger.md"),
            When::Now("how you and the challenger share each turn"),
        ));
    }
    rules.push(Rule::new(
        "diagrams.md",
        diagrams(),
        When::On("before you draw a diagram"),
    ));
    // At the drawing board, its own file says how to conclude.
    if !drawing_board {
        rules.push(Rule::new(
            "conclusion.md",
            include_str!("rules/conclusion.md"),
            When::On("before you conclude"),
        ));
    }
    rules.push(Rule::new(
        "conversation.md",
        include_str!("rules/conversation.md"),
        When::On("when a review comments wakeup names this round"),
    ));
    rules
}

/// The index of the turn `request` of a round on `comparison`, whose rule files are `rules`:
/// what the round is, which files to read now and when to read each of the others, how the turn
/// ends, then the rules that every turn needs, so that one read gives them.
fn index(request: &TurnRequest, comparison: &Comparison, rules: &[Rule]) -> String {
    let kickoff = request.is_kickoff();
    let data = if kickoff {
        "this turn's identity and unreviewed diffs"
    } else {
        "this turn's identity and unreviewed diffs, and the reviewer's answer"
    };
    let round = "the change, its author, the reset rounds, the earlier decisions and the reviewer \
                 notes";
    let mut index = format!(
        "{}\n{READ_NOW}\n\n- `{TURN}`: {data}.\n",
        include_str!("rules/index.md")
    );
    if kickoff {
        let _ = writeln!(index, "- `{ROUND}`: {round}.");
    }
    for rule in rules {
        if let When::Now(about) = rule.when {
            let _ = writeln!(index, "- `{}`: {about}.", rule.name);
        }
    }
    let _ = write!(index, "\n{READ_LATER}\n\n");
    if !kickoff {
        let _ = writeln!(
            index,
            "- `{ROUND}`: {round}, when they are no longer in your context."
        );
    }
    for rule in rules {
        if let When::On(condition) = rule.when {
            let _ = writeln!(index, "- `{}`: {condition}.", rule.name);
        }
    }
    let _ = write!(index, "\n{}", include_str!("rules/delivery.md"));
    if kickoff {
        let _ = write!(index, "\n{}", include_str!("rules/design.md"));
    }
    let _ = write!(
        index,
        "\n{}\n{}\n{}\n{}",
        include_str!("rules/questions.md"),
        sources(comparison),
        texts(request.writing),
        include_str!("rules/not-relevant.md"),
    );
    index
}

/// How to read the sources, with the commands of the version control the round's change comes
/// from, or of both when the round does not say.
fn sources(comparison: &Comparison) -> String {
    let jj = include_str!("rules/sources-jj.md");
    let git = include_str!("rules/sources-git.md");
    let commands = match comparison.base {
        Some(SnapshotIdentity::Jj { .. }) => jj.to_owned(),
        Some(SnapshotIdentity::Git { .. }) => git.to_owned(),
        None => format!("{jj}\n{git}"),
    };
    format!("{}\n{commands}", include_str!("rules/sources.md"))
}

/// How to write the texts the reviewer reads, with the markers of their callouts and status
/// marks, and the rules of the round's writing style when it has some.
fn texts(writing: WritingStyle) -> String {
    fn markers<M: Mark>() -> String {
        M::ALL
            .iter()
            .map(|mark| format!("`{}`", mark.marker()))
            .collect::<Vec<_>>()
            .join(", ")
    }
    let mut texts = format!(
        "{}\nA callout is a quote that opens with its marker, as in `> {} Run it twice.`: {}.\n\
         A status mark opens a table cell, as in `| {} 2 ms |`: {}.\n",
        include_str!("rules/texts.md"),
        Callout::Tip.marker(),
        markers::<Callout>(),
        StatusMark::Good.marker(),
        markers::<StatusMark>(),
    );
    if writing == WritingStyle::SimplifiedTechnicalEnglish {
        let _ = write!(texts, "\n{}", include_str!("rules/writing-style.md"));
    }
    texts
}

/// How the page draws a diagram, and how to avoid parse errors.
fn diagrams() -> String {
    format!(
        "# Diagrams\n\nA diagram is a fenced block that opens with ```` ```{} ````, which the \
         Explore page draws with Mermaid {}.\n{}",
        mermaid_js::FENCE,
        mermaid_js::VERSION,
        include_str!("rules/diagrams.md"),
    )
}
