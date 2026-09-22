//! Factorial study planning reuses the eval's recursive splitter and tokenizer.

use std::{io::Write, path::PathBuf};

use eyre::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    super::Candidate,
    dataset::{Case, Dataset},
    plan::Strategy,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Study {
    dataset: PathBuf,
    output: PathBuf,
    budgets: Vec<usize>,
    profiles: Vec<RequestProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RequestProfile {
    name: String,
    prompt: String,
    metadata: Metadata,
    questions: Value,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Metadata {
    Rows,
    PathLanguage,
    Headers,
}

impl RequestProfile {
    pub(super) fn request(&self, candidate: &Candidate, case: &Case) -> Value {
        let mut state = candidate.state.clone();
        match self.metadata {
            Metadata::Rows => {
                state.as_object_mut().unwrap().remove("path");
                state.as_object_mut().unwrap().remove("language_hint");
            }
            Metadata::PathLanguage => {}
            Metadata::Headers => {
                state["file_context"] = case.context.clone();
            }
        }
        json!({"model":"jev-1.13.0", "state":state, "questions":self.questions})
    }

    /// Budget state plus the longest question, including conservative JSON overhead.
    pub(super) fn tokens(body: &Value) -> usize {
        let tokenizer = tiktoken_rs::o200k_base_singleton();
        let question = body["questions"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, question)| {
                tokenizer
                    .encode_ordinary(&json!({name:question}).to_string())
                    .len()
            })
            .max()
            .unwrap();
        tokenizer.encode_ordinary(&body["state"].to_string()).len() + question + 32
    }
}

impl Study {
    pub(super) fn from_env() -> Result<Self> {
        let bytes = std::fs::read(std::env::var("JEV_STUDY_CONFIG")?)?;
        let study: Self = serde_json::from_slice(&bytes)?;
        ensure!(!study.profiles.is_empty(), "missing profiles");
        ensure!(
            !study.budgets.is_empty() && study.budgets.iter().all(|b| (1024..=24000).contains(b)),
            "study budgets must be 1024..=24000"
        );
        for profile in &study.profiles {
            ensure!(
                profile.questions.as_object().is_some_and(|q| !q.is_empty()),
                "profile questions must be a nonempty object"
            );
        }
        Ok(study)
    }

    pub(super) fn export(&self) -> Result<()> {
        let (fixtures, fingerprint) = Dataset::load(&self.dataset)?;
        std::fs::create_dir_all(&self.output)?;
        let mut stream = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.output.join("planned.jsonl"))?;
        for fixture in &fixtures {
            for profile in &self.profiles {
                for &budget in &self.budgets {
                    for (chunk, planned) in Strategy::RecursiveOverlap
                        .plan_with(fixture, budget, Some(profile))
                        .into_iter()
                        .enumerate()
                    {
                        let record = json!({
                            "case":fixture.case.id, "profile":profile.name,
                            "prompt":profile.prompt,
                            "budget":budget, "chunk":chunk,
                            "request":planned.body, "units":planned.candidate.units,
                            "estimated_tokens":planned.estimated_tokens,
                            "oversized":planned.oversized,
                        });
                        serde_json::to_writer(&mut stream, &record)?;
                        stream.write_all(b"\n")?;
                    }
                }
            }
            eprintln!("Planned {}", fixture.case.id);
        }
        std::fs::write(
            self.output.join("dataset.sha256"),
            format!("{fingerprint}\n"),
        )?;
        Ok(())
    }
}
