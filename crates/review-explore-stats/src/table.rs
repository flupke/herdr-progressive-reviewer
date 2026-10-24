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
    Tokens(u64),
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

    /// The run-ahead rows before the reasons an answer used no prepared turn.
    const RUN_AHEAD: [Row; 3] = [
        Row {
            label: "Rounds with run-ahead",
            value: |s| Cell::Count(Some(s.run_ahead.rounds)),
        },
        Row {
            label: "Answers run-ahead watched",
            value: |s| Cell::Count(Some(s.run_ahead.answers.whole)),
        },
        Row {
            label: "  that used a prepared turn",
            value: |s| Cell::Percent(s.run_ahead.answers.ratio()),
        },
    ];

    /// The run-ahead rows after the reasons.
    const RUN_AHEAD_COSTS: [Row; 8] = [
        Row {
            label: "Wait after an answer, prepared, median",
            value: |s| Cell::Time(s.run_ahead.prepared_wait),
        },
        Row {
            label: "  plain chain, median",
            value: |s| Cell::Time(s.run_ahead.plain_wait),
        },
        Row {
            label: "Forks",
            value: |s| Cell::Count(Some(s.run_ahead.forks)),
        },
        Row {
            label: "  ready when their question was answered",
            value: |s| Cell::Percent(s.run_ahead.ready.ratio()),
        },
        Row {
            label: "Fork tokens, used turns",
            value: |s| Cell::Tokens(s.run_ahead.used_tokens.total()),
        },
        Row {
            label: "  read from the cache",
            value: |s| Cell::Tokens(s.run_ahead.used_tokens.cache_read),
        },
        Row {
            label: "Fork tokens, discarded forks",
            value: |s| Cell::Tokens(s.run_ahead.discarded_tokens.total()),
        },
        Row {
            label: "  read from the cache",
            value: |s| Cell::Tokens(s.run_ahead.discarded_tokens.cache_read),
        },
    ];

    fn write(&self, output: &mut fmt::Formatter<'_>, columns: &Columns) -> fmt::Result {
        line(output, self.label, columns.map(self.value))
    }
}

/// One row of a table: its label, then its value in each column.
fn line(output: &mut fmt::Formatter<'_>, label: &str, cells: [Cell; 3]) -> fmt::Result {
    let [all, challenger, without_challenger] = cells;
    writeln!(
        output,
        "{label:LABEL$}{:>COLUMN$}{:>COLUMN$}{:>COLUMN$}",
        all.to_string(),
        challenger.to_string(),
        without_challenger.to_string(),
    )
}

/// `12`, `2.5`, `1-6`, `25%`, `45s`, `2m 05s`, `1h 02m`, or tokens as `950`, `12.5k` or `3.2M`.
impl fmt::Display for Cell {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count(Some(count)) => write!(output, "{count}"),
            Self::Number(Some(number)) if number.fract() == 0.0 => write!(output, "{number:.0}"),
            Self::Number(Some(number)) => write!(output, "{number:.1}"),
            Self::Range(Some(spread)) => write!(output, "{}-{}", spread.least, spread.most),
            Self::Percent(Some(ratio)) => write!(output, "{:.0}%", ratio * 100.0),
            Self::Time(Some(duration)) => write_time(output, *duration),
            Self::Tokens(tokens) => write_tokens(output, *tokens),
            Self::Count(None)
            | Self::Number(None)
            | Self::Range(None)
            | Self::Percent(None)
            | Self::Time(None) => output.write_str("-"),
        }
    }
}

fn write_time(output: &mut fmt::Formatter<'_>, duration: Duration) -> fmt::Result {
    let seconds = (duration.as_millis() + 500) / 1000;
    match seconds {
        0..60 => write!(output, "{seconds}s"),
        60..3600 => write!(output, "{}m {:02}s", seconds / 60, seconds % 60),
        _ => write!(output, "{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

#[allow(clippy::cast_precision_loss)]
fn write_tokens(output: &mut fmt::Formatter<'_>, tokens: u64) -> fmt::Result {
    match tokens {
        0..1000 => write!(output, "{tokens}"),
        1000..1_000_000 => write!(output, "{:.1}k", tokens as f64 / 1e3),
        _ => write!(output, "{:.1}M", tokens as f64 / 1e6),
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
        for row in &Row::RUN_AHEAD {
            row.write(output, self)?;
        }
        for reason in self.not_prepared_reasons() {
            let count = |summary: &Summary| {
                Cell::Count(Some(
                    summary
                        .run_ahead
                        .not_prepared
                        .get(reason)
                        .copied()
                        .unwrap_or(0),
                ))
            };
            line(output, &format!("  not: {reason}"), self.map(count))?;
        }
        for row in &Row::RUN_AHEAD_COSTS {
            row.write(output, self)?;
        }
        Ok(())
    }

    /// The value of each column.
    fn map(&self, value: impl Fn(&Summary) -> Cell) -> [Cell; 3] {
        [
            value(&self.all),
            value(&self.challenger),
            value(&self.without_challenger),
        ]
    }

    /// Why answers used no prepared turn, the most frequent first.
    fn not_prepared_reasons(&self) -> Vec<&'static str> {
        let counts = &self.all.run_ahead.not_prepared;
        let mut reasons: Vec<_> = counts.keys().copied().collect();
        reasons.sort_by_key(|reason| std::cmp::Reverse(counts[reason]));
        reasons
    }
}
