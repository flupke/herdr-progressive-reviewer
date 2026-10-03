//! Numbers from the saved Explore rounds of a repository, to see whether a
//! change to Explore helps the reviewer.
mod period;
mod proposals;
mod sample;
mod summary;
mod table;
mod words;

pub use period::Period;
use review_store::SavedRounds;
use sample::RoundSample;
use std::fmt;
use summary::Summary;

/// The numbers of some rounds, for all of them and split by Challenger.
#[derive(Debug)]
struct Columns {
    all: Summary,
    challenger: Summary,
    without_challenger: Summary,
}

impl Columns {
    fn of<'a>(samples: impl Iterator<Item = &'a RoundSample> + Clone) -> Self {
        Self {
            all: Summary::of(samples.clone()),
            challenger: Summary::of(samples.clone().filter(|sample| sample.challenger)),
            without_challenger: Summary::of(samples.filter(|sample| !sample.challenger)),
        }
    }
}

/// The numbers of every saved round, and of the rounds started in a period.
#[derive(Debug)]
pub struct Report {
    unreadable: usize,
    all_time: Columns,
    period: Option<(Period, Columns)>,
}

impl Report {
    pub fn new(saved: &SavedRounds, period: Option<Period>) -> Self {
        let samples: Vec<_> = saved.rounds.iter().map(RoundSample::of).collect();
        let period = period.map(|period| {
            let columns = Columns::of(
                samples
                    .iter()
                    .filter(|sample| period.contains(sample.started_at_ms)),
            );
            (period, columns)
        });
        Self {
            unreadable: saved.unreadable,
            all_time: Columns::of(samples.iter()),
            period,
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            output,
            "Rounds that could not be read (earlier format or damaged): {}",
            self.unreadable
        )?;
        writeln!(output)?;
        self.all_time.write(output, "All time")?;
        if let Some((period, columns)) = &self.period {
            writeln!(output)?;
            columns.write(output, &format!("Started {period}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
