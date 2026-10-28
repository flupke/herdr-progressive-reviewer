//! The repository's saved rounds, as the Explore page on this machine lists and opens them.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use review_explore_page::{Interruption, RoundStage};
use review_repository::diff::DiffRow;

use super::*;
use crate::past::SavedRoundsReader;

/// The reader of the harness's saved rounds.
fn reader(harness: &Harness) -> SavedRoundsReader {
    SavedRoundsReader::new(harness.store.clone(), harness.repository.clone())
}

/// The round of the harness, asking its first question, which cites `reviewed.rs`.
fn asking() -> Harness {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    harness
}

/// The text of each row of the first citation of the question `stage` asks.
fn cited(stage: &RoundStage) -> Vec<DiffRow> {
    let RoundStage::Question { citations, .. } = stage else {
        panic!("the round shows {stage:?}");
    };
    citations[0]
        .lines
        .as_ref()
        .unwrap()
        .iter()
        .map(|row| row.diff.clone())
        .collect()
}

#[test]
fn a_reset_round_opens_where_it_stopped_with_the_code_it_cited_then() {
    let mut harness = asking();
    let instance = harness.exploration().instance.clone();
    harness.session.handle(Input::Command(Command::Reset));
    // The working copy moved on since the round cited it.
    harness
        .files
        .write("reviewed.rs", b"pub fn rewritten() {}\n");

    let past = reader(&harness).open(&instance).expect("the reset round");

    assert_eq!(past.id(), instance);
    let RoundStage::Question { question, .. } = past.stage() else {
        panic!("the round shows {:?}", past.stage());
    };
    assert_eq!(question.id, "q1");
    assert_eq!(
        cited(past.stage()),
        [DiffRow::Add {
            new_line: 1,
            text: "+pub fn reviewed() {}".into()
        }]
    );
    assert_eq!(past.review().title, "Git working tree");
}

#[test]
fn a_round_saved_while_the_agent_worked_shows_where_it_stopped() {
    let mut harness = asking();
    let (answer, _access) = harness.answer("Keep it.");
    let instance = harness.exploration().instance.clone();

    let past = reader(&harness).open(&instance).expect("the saved round");

    let RoundStage::Interrupted {
        request,
        interruption,
        answer: sent,
        ..
    } = past.stage()
    else {
        panic!("the round shows {:?}", past.stage());
    };
    assert_eq!(request.as_deref(), Some(answer.request.as_str()));
    // The prompt reached the agent, whose turn never came back.
    assert_eq!(*interruption, Interruption::Stopped);
    let sent = sent.as_ref().expect("the answer the turn carries");
    assert_eq!(sent.kept.comment, "Keep it.");
}

#[test]
fn the_saved_rounds_are_listed_by_change_the_change_with_the_newest_round_first() {
    let harness = asking();
    let saved = harness.saved();
    let records = harness.store.lock_explore(&harness.unit).unwrap();
    // A later round of the same change, and a round of another change saved since.
    let mut later = saved.clone();
    later.exploration.instance = "later-1".into();
    records.create_round(&later).unwrap();
    drop(records);
    let mut other = saved.clone();
    other.exploration.instance = "other-1".into();
    let comparison = Arc::make_mut(&mut other.exploration.comparison);
    comparison.checkpoint.review_unit = "other-change".into();
    comparison.base = None;
    let records = harness.store.lock_explore(&"other-change".into()).unwrap();
    records.create_round(&other).unwrap();
    drop(records);
    let now = SystemTime::now();
    let age = |instance: &str, unit: &str, ago: u64| {
        let files = harness.store.saved_explore_round_files().unwrap();
        let file = files
            .iter()
            .find(|file| {
                file.path().ends_with(format!("{instance}.json"))
                    && harness
                        .store
                        .read_round_heading(file)
                        .is_some_and(|heading| heading.review_unit.as_str() == unit)
            })
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(file.path())
            .unwrap()
            .set_modified(now - Duration::from_secs(ago))
            .unwrap();
    };
    age(&saved.exploration.instance, harness.unit.as_str(), 300);
    age("later-1", harness.unit.as_str(), 100);
    age("other-1", "other-change", 10);

    let changes = reader(&harness).list();

    let listed: Vec<(&str, Vec<&str>)> = changes
        .iter()
        .map(|change| {
            let rounds = change
                .rounds
                .iter()
                .map(|round| round.id.as_str())
                .collect();
            (change.title.as_str(), rounds)
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("No description", vec!["other-1"]),
            (
                "Git working tree",
                vec!["later-1", saved.exploration.instance.as_str()]
            ),
        ]
    );
    let millis = |ago: u64| {
        (now - Duration::from_secs(ago))
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    };
    assert_eq!(u128::from(changes[1].rounds[0].saved_at_ms), millis(100));
}

#[test]
fn a_round_saved_in_an_earlier_format_is_neither_listed_nor_opened() {
    let harness = asking();
    let instance = harness.exploration().instance.clone();
    let path = harness.round_path();
    let saved = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, saved.replacen("\"version\":2", "\"version\":1", 1)).unwrap();

    let reader = reader(&harness);

    assert_eq!(reader.list(), []);
    assert!(reader.open(&instance).is_none());
}
