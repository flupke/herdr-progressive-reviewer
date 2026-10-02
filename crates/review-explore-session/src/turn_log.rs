//! Test tooling for `make vision` only: a record of every prompt the session
//! sent, for a vision session's scripted agent. One numbered JSON file per
//! prompt, written once its delivery finished, so the agent reads what was
//! sent instead of parsing a terminal.
//!
//! The files hold the access value of each prompt, so a reviewer outside a
//! vision session never has a turn log.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use review_explore::{ReviewerAnswer, TopicStatus, TurnRequest};
use review_source::ReviewCheckpoint;
use review_thread_service::PromptError;
use serde::Serialize;

/// The directory a vision session reads its turns from.
#[derive(Clone, Debug)]
pub struct TurnLog {
    directory: PathBuf,
    recorded: Arc<AtomicU64>,
}

/// One prompt as it was sent.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SentTurn {
    kind: TurnKind,
    /// What a reply must carry; an Implement prompt takes no reply.
    #[serde(flatten)]
    identity: Option<TurnIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    answer: Option<SentAnswer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cancelled: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unreviewed: Option<String>,
    text: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum TurnKind {
    Kickoff,
    Wakeup,
    Implement,
}

#[derive(Clone, Debug, Serialize)]
struct TurnIdentity {
    access: String,
    round: String,
    request: String,
    checkpoint: ReviewCheckpoint,
}

/// The reviewer's answer as the prompt states it.
#[derive(Clone, Debug, Serialize)]
struct SentAnswer {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    question: Option<SentQuestion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_conclusion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    option: Option<SentOption>,
    text: String,
}

#[derive(Clone, Debug, Serialize)]
struct SentQuestion {
    id: String,
    version: u32,
}

#[derive(Clone, Debug, Serialize)]
struct SentOption {
    id: String,
    text: String,
    outcome: TopicStatus,
}

/// A sent turn with its number and how its delivery ended.
#[derive(Serialize)]
struct RecordedTurn<'a> {
    turn: u64,
    delivered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(flatten)]
    sent: &'a SentTurn,
}

impl SentAnswer {
    fn new(answer: &ReviewerAnswer) -> Self {
        Self {
            id: answer.id.clone(),
            question: answer.question.as_ref().map(|question| SentQuestion {
                id: question.id.clone(),
                version: question.version,
            }),
            reply_to_conclusion: answer
                .question
                .is_none()
                .then(|| answer.in_reply_to.clone()),
            option: answer.option.as_ref().map(|option| SentOption {
                id: option.id.clone(),
                text: option.text.clone(),
                outcome: option.outcome,
            }),
            text: answer.text.clone(),
        }
    }
}

impl SentTurn {
    /// An interview prompt: the kickoff, or the wakeup after an answer.
    pub(crate) fn interview(
        request: &TurnRequest,
        access: &str,
        unreviewed: String,
        text: String,
    ) -> Self {
        Self {
            kind: if request.answer.is_some() {
                TurnKind::Wakeup
            } else {
                TurnKind::Kickoff
            },
            identity: Some(TurnIdentity {
                access: access.to_owned(),
                round: request.instance.clone(),
                request: request.request.clone(),
                checkpoint: request.checkpoint.clone(),
            }),
            answer: request.answer.as_ref().map(SentAnswer::new),
            cancelled: request.cancelled.clone(),
            unreviewed: Some(unreviewed),
            text,
        }
    }

    pub(crate) fn implement(text: String) -> Self {
        Self {
            kind: TurnKind::Implement,
            identity: None,
            answer: None,
            cancelled: Vec::new(),
            unreviewed: None,
            text,
        }
    }
}

impl TurnLog {
    /// Continue after the highest turn `directory` already holds, so a
    /// reopened reviewer never reuses a number.
    pub fn open(directory: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&directory)?;
        let recorded = std::fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()?
                    .strip_prefix("turn-")?
                    .strip_suffix(".json")?
                    .parse::<u64>()
                    .ok()
            })
            .max()
            .unwrap_or(0);
        Ok(Self {
            directory,
            recorded: Arc::new(AtomicU64::new(recorded)),
        })
    }

    /// Save `sent` under the next number. A file appears complete or not at all.
    pub(crate) fn record(&self, sent: &SentTurn, result: &Result<(), PromptError>) {
        let turn = self.recorded.fetch_add(1, Ordering::AcqRel) + 1;
        let recorded = RecordedTurn {
            turn,
            delivered: result.is_ok(),
            error: result.as_ref().err().map(ToString::to_string),
            sent,
        };
        let path = self.directory.join(format!("turn-{turn:06}.json"));
        let partial = path.with_extension("partial");
        // A vision session is a test tool: a turn it cannot save shows as a
        // turn the scripted agent never receives.
        if let Ok(bytes) = serde_json::to_vec_pretty(&recorded)
            && std::fs::write(&partial, bytes).is_ok()
        {
            let _ = std::fs::rename(partial, path);
        }
    }
}
