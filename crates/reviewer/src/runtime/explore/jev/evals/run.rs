use std::{
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

use eyre::{Context, Result, ensure};
use review_explore::{Significance, SignificanceResult};
use serde::Serialize;
use serde_json::{Value, json};

use super::super::{MODEL, RUBRIC, parse_response, request_json};
use super::{
    dataset::{Dataset, Fixture},
    metrics::{Scores, owns},
    plan::{Chunk, Strategy},
};

// o200k_base undercounts Jev's reported tokens on this corpus; 14k leaves
// headroom below Jev's 32k state-plus-longest-question limit.
const MAX_ESTIMATED_TOKENS: usize = 14_000;

pub(super) struct Experiment {
    fixtures: Vec<Fixture>,
    fingerprint: String,
    budgets: Vec<usize>,
    repeats: usize,
    output: PathBuf,
    max_requests: usize,
    filter: String,
}

#[derive(Default, Serialize)]
struct Totals {
    scores: Scores,
    requests: usize,
    input_tokens: u64,
    output_tokens: u64,
    elapsed_ms: u128,
    failures: usize,
    missing_usage: usize,
    estimated_tokens: usize,
}

#[derive(Clone, Copy)]
struct Trial {
    strategy: Strategy,
    budget: usize,
    repeat: usize,
}

struct Recorder {
    file: std::fs::File,
    totals: BTreeMap<String, Totals>,
    calls: usize,
}

impl Experiment {
    pub(super) fn export_recursive(&self) -> Result<()> {
        std::fs::create_dir_all(&self.output)?;
        let mut stream = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.output.join("planned.jsonl"))?;
        for fixture in &self.fixtures {
            for &budget in &self.budgets {
                for (chunk, planned) in Strategy::Recursive
                    .plan(fixture, budget)
                    .into_iter()
                    .enumerate()
                {
                    let record = json!({
                        "case": fixture.case.id,
                        "strategy": Strategy::Recursive,
                        "budget": budget,
                        "repeat": 0,
                        "chunk": chunk,
                        "request": planned.body,
                        "result": {"units": planned.candidate.units},
                        "estimated_tokens": planned.estimated_tokens,
                        "oversized": planned.oversized,
                    });
                    serde_json::to_writer(&mut stream, &record)?;
                    stream.write_all(b"\n")?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn from_env() -> Result<Self> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/jev-evals");
        let (mut fixtures, fingerprint) = Dataset::load(&root)?;
        let filter = std::env::var("JEV_EVAL_CASE").unwrap_or_default();
        fixtures.retain(|fixture| fixture.case.id.contains(&filter));
        ensure!(!fixtures.is_empty(), "case filter matched no fixtures");
        let budgets = std::env::var("JEV_EVAL_BUDGETS")
            .unwrap_or_else(|_| "1024,2048,14000".into())
            .split(',')
            .map(str::parse)
            .collect::<std::result::Result<Vec<usize>, _>>()?;
        ensure!(
            !budgets.is_empty()
                && budgets
                    .iter()
                    .all(|budget| (512..=MAX_ESTIMATED_TOKENS).contains(budget)),
            "budgets must be 512..={MAX_ESTIMATED_TOKENS} estimated tokens (margin included)"
        );
        let repeats = Self::number("JEV_EVAL_REPEATS", 3)?;
        let max_requests = Self::number("JEV_EVAL_MAX_REQUESTS", 2000)?;
        ensure!(
            repeats > 0 && max_requests > 0,
            "repeats and request cap must be positive"
        );
        let output = std::env::var_os("JEV_EVAL_OUTPUT").map_or_else(
            || {
                root.join("../../../../target/jev-evals")
                    .join(uuid::Uuid::new_v4().to_string())
            },
            PathBuf::from,
        );
        Ok(Self {
            fixtures,
            fingerprint,
            budgets,
            repeats,
            output,
            max_requests,
            filter,
        })
    }

    fn number(name: &str, default: usize) -> Result<usize> {
        std::env::var(name).map_or(Ok(default), |value| {
            value.parse().wrap_err_with(|| name.to_owned())
        })
    }

    pub(super) fn run(&self) -> Result<()> {
        let key =
            std::env::var("TYPESAFE_API_KEY").wrap_err("live eval requires TYPESAFE_API_KEY")?;
        ensure!(!key.trim().is_empty(), "TYPESAFE_API_KEY is empty");
        let planned = self.planned_requests();
        ensure!(
            planned > 0 && planned <= self.max_requests,
            "planned {planned} requests exceed cap {} or no requests fit; adjust JEV_EVAL_MAX_REQUESTS/budgets/case filter",
            self.max_requests
        );
        std::fs::create_dir_all(&self.output)?;
        eprintln!(
            "Planned {planned} Jev requests; writing {}",
            self.output.display()
        );
        let mut recorder = Recorder {
            file: std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.output.join("requests.jsonl"))?,
            totals: BTreeMap::new(),
            calls: 0,
        };
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        for repeat in 0..self.repeats {
            for fixture in &self.fixtures {
                for budget in &self.budgets {
                    for index in 0..Strategy::ALL.len() {
                        let strategy = Strategy::ALL[(index + repeat) % Strategy::ALL.len()];
                        let chunks = strategy.plan(fixture, *budget);
                        recorder.evaluate(
                            &client,
                            &key,
                            fixture,
                            &chunks,
                            Trial {
                                strategy,
                                budget: *budget,
                                repeat,
                            },
                        )?;
                        self.save_summary(&recorder, false)?;
                    }
                }
            }
        }
        self.save_summary(&recorder, true)?;
        for (name, totals) in &recorder.totals {
            eprintln!(
                "{name}: {} calls, {} input tokens, {} false exclusions / {} significant lines, {} useful exclusions / {} insignificant lines, {} provider failures",
                totals.requests,
                totals.input_tokens,
                totals.scores.false_exclusions,
                totals.scores.significant,
                totals.scores.correct_exclusions,
                totals.scores.insignificant,
                totals.failures
            );
        }
        eprintln!("Jev eval artifacts: {}", self.output.display());
        ensure!(recorder.calls == planned, "not all planned requests ran");
        ensure!(
            recorder.totals.values().all(|totals| totals.failures == 0),
            "provider failures: inspect requests.jsonl; no quality conclusion is valid for missing responses"
        );
        ensure!(
            recorder
                .totals
                .values()
                .all(|totals| totals.missing_usage == 0),
            "missing provider usage; token calibration is incomplete"
        );
        Ok(())
    }

    fn planned_requests(&self) -> usize {
        self.fixtures
            .iter()
            .flat_map(|fixture| self.budgets.iter().map(move |budget| (fixture, budget)))
            .map(|(fixture, budget)| {
                Strategy::ALL
                    .iter()
                    .map(|strategy| {
                        strategy
                            .plan(fixture, *budget)
                            .iter()
                            .filter(|chunk| !chunk.oversized)
                            .count()
                    })
                    .sum::<usize>()
            })
            .sum::<usize>()
            * self.repeats
    }

    fn save_summary(&self, recorder: &Recorder, complete: bool) -> Result<()> {
        let groups: BTreeMap<_, _> = recorder
            .totals
            .iter()
            .map(|(name, totals)| {
                (
                    name,
                    json!({"counts": totals, "rates": totals.scores.rates()}),
                )
            })
            .collect();
        let report = json!({
            "schema_version": 1, "complete":complete,
            "dataset_sha256":self.fingerprint, "case_filter": self.filter,
            "model":MODEL, "rubric":RUBRIC, "tokenizer":"tiktoken-rs/o200k_base",
            "budgets":self.budgets, "repeats":self.repeats,
            "calls":recorder.calls, "groups":groups,
            "limitations":["Labels are agent-authored development labels, not independent human gold labels.",
                "Lines and repeated runs within a fixture are correlated; do not treat them as independent samples.",
                "Legacy uses the production prompt/state shape and caps; only recursive vs recursive_overlap isolates overlap.",
                "Test success validates execution, not a strategy-quality threshold or statistical superiority."]
        });
        let path = self.output.join("summary.json");
        std::fs::write(
            path.with_extension("json.tmp"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        std::fs::rename(path.with_extension("json.tmp"), path)?;
        Ok(())
    }
}

impl Recorder {
    fn evaluate(
        &mut self,
        client: &reqwest::blocking::Client,
        key: &str,
        fixture: &Fixture,
        chunks: &[Chunk],
        trial: Trial,
    ) -> Result<()> {
        let Trial {
            strategy,
            budget,
            repeat,
        } = trial;
        let group = format!("{strategy:?}/{budget}");
        let mut results = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            let start = Instant::now();
            let raw = if chunk.oversized {
                None
            } else {
                self.calls += 1;
                Some(request_json(client, key, &chunk.body))
            };
            let elapsed_ms = start.elapsed().as_millis();
            let result = Self::result(chunk, raw.as_ref());
            let totals = self.totals.entry(group.clone()).or_default();
            if let Some(raw) = &raw {
                totals.requests += 1;
                totals.estimated_tokens += chunk.estimated_tokens;
                totals.elapsed_ms += elapsed_ms;
                if let Ok(value) = raw {
                    totals.observe_usage(value);
                }
                totals.failures += usize::from(result.outcome == Significance::Failed);
            }
            let record = json!({"case":fixture.case.id,"strategy":strategy,"budget":budget,"repeat":repeat,
                "chunk":index,"request":chunk.body,"estimated_tokens":chunk.estimated_tokens,
                "elapsed_ms":elapsed_ms,"response":raw,"result":result});
            writeln!(self.file, "{}", serde_json::to_string(&record)?)?;
            self.file.flush()?;
            results.push(result);
        }
        self.score(fixture, &results, &group, trial)?;
        eprintln!(
            "{} {strategy:?} budget={budget} repeat={} ({} chunks)",
            fixture.case.id,
            repeat + 1,
            chunks.len()
        );
        Ok(())
    }

    fn result(
        chunk: &Chunk,
        raw: Option<&std::result::Result<Value, String>>,
    ) -> SignificanceResult {
        match raw {
            Some(Ok(value)) => parse_response(&chunk.candidate, value),
            Some(Err(message)) => chunk.candidate.result(
                Significance::Failed,
                None,
                BTreeMap::new(),
                None,
                Some(message.clone()),
            ),
            None => chunk.candidate.result(
                Significance::Oversized,
                None,
                BTreeMap::new(),
                None,
                Some("Does not fit this strategy's request budget".into()),
            ),
        }
    }

    fn score(
        &mut self,
        fixture: &Fixture,
        results: &[SignificanceResult],
        group: &str,
        trial: Trial,
    ) -> Result<()> {
        let mut scores = Scores::default();
        let mut lines = Vec::new();
        for label in &fixture.case.labels {
            let owners: Vec<_> = results
                .iter()
                .filter(|result| owns(result, &label.unit()))
                .collect();
            ensure!(owners.len() <= 1, "overlapping judgment targets");
            let result = owners.first().copied();
            scores.observe(label, result);
            self.totals
                .entry(group.to_owned())
                .or_default()
                .scores
                .observe(label, result);
            lines.push(json!({"label":label,"prediction":result.map(|result| &result.outcome)}));
        }
        writeln!(
            self.file,
            "{}",
            json!({"case":fixture.case.id,"strategy":trial.strategy,"budget":trial.budget,"repeat":trial.repeat,"line_scores":lines,"counts":scores,"rates":scores.rates()})
        )?;
        self.file.flush()?;
        Ok(())
    }
}

impl Totals {
    fn observe_usage(&mut self, response: &Value) {
        match (
            response["usage"]["input_tokens"].as_u64(),
            response["usage"]["output_tokens"].as_u64(),
        ) {
            (Some(input), Some(output)) => {
                self.input_tokens += input;
                self.output_tokens += output;
            }
            _ => self.missing_usage += 1,
        }
    }
}
