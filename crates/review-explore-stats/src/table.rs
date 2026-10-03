//! A table of numbers: one row per measure, one column per set of rounds.
use crate::{
    Columns,
    proposals::ProposalCounts,
    summary::{Spread, Summary},
};
use std::{fmt, time::Duration};

const LABEL: usize = 42;
const COLUMN: usize = 15;

/// One row: its label and how it shows a column's value.
struct Row {
    label: &'static str,
    value: fn(&Summary) -> Cell,
}

/// One value of a table, or a dash when the rounds have none.
enum Cell {
    Count(Option<usize>),
    Number(Option<f64>),
    Range(Option<Spread>),
    Percent(Option<f64>),
    Time(Option<Duration>),
}

impl Row {
    const ALL: [Row; 20] = [
        Row {
            label: "Rounds",
            value: |s| Cell::Count(Some(s.rounds)),
        },
        Row {
            label: "  with answers",
            value: |s| Cell::Count(Some(s.answered)),
        },
        Row {
            label: "  with a conclusion",
            value: |s| Cell::Count(Some(s.concluded)),
        },
        Row {
            label: "Questions per answered round, median",
            value: |s| Cell::Number(s.questions.map(|questions| questions.median)),
        },
        Row {
            label: "  range",
            value: |s| Cell::Range(s.questions),
        },
        Row {
            label: "Answers asking for a change",
            value: |s| Cell::Percent(s.change_requests.ratio()),
        },
        Row {
            label: "  of answers taken up",
            value: |s| Cell::Count(Some(s.change_requests.whole)),
        },
        Row {
            label: "Answers not taking the recommendation",
            value: |s| Cell::Percent(s.declined_recommendations.ratio()),
        },
        Row {
            label: "  of answers to a recommendation",
            value: |s| Cell::Count(Some(s.declined_recommendations.whole)),
        },
        Row {
            label: "Agent turn, median",
            value: |s| Cell::Time(s.agent_turn),
        },
        Row {
            label: "  after an answer",
            value: |s| Cell::Time(s.agent_turn_after_answer),
        },
        Row {
            label: "Reviewer's answer, median",
            value: |s| Cell::Time(s.reviewer_answer),
        },
        Row {
            label: "Round time waiting for the agent, median",
            value: |s| Cell::Percent(s.waiting_share),
        },
        Row {
            label: "Words to read per question, median",
            value: |s| Cell::Number(s.question_words),
        },
        Row {
            label: "Challenger's proposals",
            value: |s| Cell::Count(s.proposals.map(ProposalCounts::total)),
        },
        Row {
            label: "  rounds that reported proposals",
            value: |s| Cell::Count(s.proposals.map(|p| p.rounds)),
        },
        Row {
            label: "  asked",
            value: |s| Cell::Count(s.proposals.map(|p| p.asked)),
        },
        Row {
            label: "  merged with the implementer's",
            value: |s| Cell::Count(s.proposals.map(|p| p.merged)),
        },
        Row {
            label: "  retired by a fact",
            value: |s| Cell::Count(s.proposals.map(|p| p.retired)),
        },
        Row {
            label: "  kept for a later turn",
            value: |s| Cell::Count(s.proposals.map(|p| p.kept)),
        },
    ];

    fn write(&self, output: &mut fmt::Formatter<'_>, columns: &Columns) -> fmt::Result {
        writeln!(
            output,
            "{:LABEL$}{:>COLUMN$}{:>COLUMN$}{:>COLUMN$}",
            self.label,
            (self.value)(&columns.all).to_string(),
            (self.value)(&columns.challenger).to_string(),
            (self.value)(&columns.without_challenger).to_string(),
        )
    }
}

/// `12`, `2.5`, `1-6`, `25%`, `45s`, `2m 05s` or `1h 02m`.
impl fmt::Display for Cell {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count(Some(count)) => write!(output, "{count}"),
            Self::Number(Some(number)) if number.fract() == 0.0 => write!(output, "{number:.0}"),
            Self::Number(Some(number)) => write!(output, "{number:.1}"),
            Self::Range(Some(spread)) => write!(output, "{}-{}", spread.least, spread.most),
            Self::Percent(Some(ratio)) => write!(output, "{:.0}%", ratio * 100.0),
            Self::Time(Some(duration)) => {
                let seconds = (duration.as_millis() + 500) / 1000;
                match seconds {
                    0..60 => write!(output, "{seconds}s"),
                    60..3600 => write!(output, "{}m {:02}s", seconds / 60, seconds % 60),
                    _ => write!(output, "{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
                }
            }
            Self::Count(None)
            | Self::Number(None)
            | Self::Range(None)
            | Self::Percent(None)
            | Self::Time(None) => output.write_str("-"),
        }
    }
}

impl Columns {
    pub(crate) fn write(&self, output: &mut fmt::Formatter<'_>, title: &str) -> fmt::Result {
        writeln!(
            output,
            "{title:LABEL$}{:>COLUMN$}{:>COLUMN$}{:>COLUMN$}",
            "All", "Challenger", "No challenger"
        )?;
        for row in &Row::ALL {
            row.write(output, self)?;
        }
        Ok(())
    }
}
