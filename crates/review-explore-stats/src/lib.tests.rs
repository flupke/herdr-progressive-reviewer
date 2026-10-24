//! The numbers of three synthetic saved rounds, worked out by hand from the
//! fixtures in `testdata`:
//!
//! - `answered.json`, started 2026-09-10 10:00 UTC, without a Challenger: two
//!   questions, both answered, then a conclusion. Agent turns of 200, 120 and
//!   30 s; the reviewer answered after 30 and 90 s. The first answer declined
//!   the recommended choice and asked for a change; the second question had no
//!   recommendation. The questions hold 18 and 15 words.
//! - `challenger.json`, started 2026-09-20 10:00 UTC, with a Challenger: three
//!   questions, two answered, no conclusion. Agent turns of 240, 180 and
//!   150 s; the reviewer answered after 60 and 120 s. The first answer took the
//!   recommendation, the second chose None of the above. No change was asked.
//!   The questions hold 8, 10 and 5 words. The Challenger proposed five
//!   questions: one asked, one kept on the first turn and merged on the
//!   second, two retired and one still kept at the end.
//!   Run-ahead watched both answers (`challenger.forks.json`). The first used the
//!   prepared turn of its fork for "mutex", which had submitted 10 s before the
//!   answer, so its 180 s are the agent's switch to the fork, no agent turn; the
//!   other fork of that question had not. The second answer chose
//!   None of the above: of its question's three forks, one was discarded by a
//!   message in the round conversation before the answer, one had submitted 10 s before it, and
//!   one, saved before forks kept when they submitted, had its turn when the
//!   answer discarded it. The fork used spent 55,010 tokens, 50,000 of them read
//!   from the cache; the others 92,705, 90,000 from the cache.
//! - `unanswered.json`, saved before turns recorded their timing, without a
//!   Challenger: one question of 6 words, never answered.
//!
//! The answered and unanswered rounds have no run-ahead record, as every round
//! saved before run-ahead existed.
use super::*;
use proposals::ProposalCounts;
use review_run_ahead::RoundForks;
use review_run_ahead::TokenUsage;
use review_store::SavedRound;
use run_ahead::RunAheadSummary;
use std::time::{Duration, SystemTime};
use summary::{Share, Spread};

const ANSWERED: &str = include_str!("../testdata/answered.json");
const CHALLENGER: &str = include_str!("../testdata/challenger.json");
const UNANSWERED: &str = include_str!("../testdata/unanswered.json");
const CHALLENGER_FORKS: &str = include_str!("../testdata/challenger.forks.json");

/// When the unanswered round was saved: 2026-09-25 10:00 UTC.
const UNANSWERED_SAVED_AT_S: u64 = 1_790_330_400;

fn saved(json: &str, saved_at: SystemTime) -> SavedRound {
    let mut stored: serde_json::Value = serde_json::from_str(json).unwrap();
    SavedRound {
        round: serde_json::from_value(stored["value"].take()).unwrap(),
        saved_at,
        run_ahead: RoundForks::default(),
    }
}

fn forks(json: &str) -> RoundForks {
    let mut stored: serde_json::Value = serde_json::from_str(json).unwrap();
    serde_json::from_value(stored["value"].take()).unwrap()
}

fn rounds() -> SavedRounds {
    let late = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    SavedRounds {
        rounds: vec![
            saved(ANSWERED, late),
            SavedRound {
                run_ahead: forks(CHALLENGER_FORKS),
                ..saved(CHALLENGER, late)
            },
            saved(
                UNANSWERED,
                SystemTime::UNIX_EPOCH + Duration::from_secs(UNANSWERED_SAVED_AT_S),
            ),
        ],
        unreadable: 2,
    }
}

fn seconds(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn assert_share(actual: Option<f64>, expected: f64) {
    let actual = actual.expect("a share");
    assert!(
        (actual - expected).abs() < 1e-9,
        "share {actual} is not {expected}"
    );
}

#[test]
fn every_saved_round_counts_and_unanswered_ones_stay_out_of_the_answer_numbers() {
    let report = Report::new(&rounds(), None);
    let all = &report.all_time.all;

    assert_eq!(report.unreadable, 2);
    assert_eq!((all.rounds, all.answered, all.concluded), (3, 2, 1));
    assert_eq!(
        all.questions,
        Some(Spread {
            median: 2.5,
            least: 2,
            most: 3
        })
    );
    assert_eq!(all.change_requests, Share { part: 1, whole: 4 });
    assert_eq!(all.declined_recommendations, Share { part: 2, whole: 3 });
    assert_eq!(all.agent_turn, Some(seconds(150)));
    assert_eq!(all.agent_turn_after_answer, Some(seconds(120)));
    assert_eq!(all.reviewer_answer, Some(seconds(75)));
    assert_share(
        all.waiting_share,
        f64::midpoint(350.0 / 470.0, 570.0 / 750.0),
    );
    assert_eq!(all.question_words, Some(9.0));
}

#[test]
fn rounds_split_between_with_and_without_a_challenger() {
    let report = Report::new(&rounds(), None);
    let columns = &report.all_time;
    let challenger = &columns.challenger;
    let without = &columns.without_challenger;

    assert_eq!(
        (challenger.rounds, challenger.answered, challenger.concluded),
        (1, 1, 0)
    );
    assert_eq!(
        challenger.questions,
        Some(Spread {
            median: 3.0,
            least: 3,
            most: 3
        })
    );
    assert_eq!(challenger.change_requests, Share { part: 0, whole: 2 });
    assert_eq!(
        challenger.declined_recommendations,
        Share { part: 1, whole: 2 }
    );
    assert_eq!(challenger.agent_turn, Some(seconds(195)));
    assert_eq!(challenger.agent_turn_after_answer, Some(seconds(150)));
    assert_eq!(challenger.reviewer_answer, Some(seconds(90)));
    assert_share(challenger.waiting_share, 570.0 / 750.0);
    assert_eq!(challenger.question_words, Some(8.0));

    assert_eq!(
        (without.rounds, without.answered, without.concluded),
        (2, 1, 1)
    );
    assert_eq!(without.change_requests, Share { part: 1, whole: 2 });
    assert_eq!(
        without.declined_recommendations,
        Share { part: 1, whole: 1 }
    );
    assert_eq!(without.agent_turn, Some(seconds(120)));
    assert_eq!(without.agent_turn_after_answer, Some(seconds(75)));
    assert_eq!(without.reviewer_answer, Some(seconds(60)));
    assert_share(without.waiting_share, 350.0 / 470.0);
    assert_eq!(without.question_words, Some(15.0));
}

#[test]
fn a_period_keeps_the_rounds_started_in_it() {
    let period = Period::parse(Some("2026-09-15T00:00:00Z"), None).unwrap();

    let report = Report::new(&rounds(), Some(period));
    let period = &report.period.as_ref().expect("the period's numbers").1;

    assert_eq!(report.all_time.all.rounds, 3);
    assert_eq!((period.all.rounds, period.all.answered), (2, 1));
    assert_eq!(period.all.agent_turn, Some(seconds(195)));
    assert_eq!(period.without_challenger.rounds, 1);
    assert_eq!(period.without_challenger.reviewer_answer, None);
}

#[test]
fn a_period_ends_before_its_until_time() {
    let period = Period::parse(Some("2026-09-10T10:00:00Z"), Some("2026-09-20T10:00:00Z")).unwrap();

    let report = Report::new(&rounds(), Some(period));

    assert_eq!(report.period.as_ref().unwrap().1.all.rounds, 1);
    assert_eq!(
        report.period.as_ref().unwrap().1.without_challenger.rounds,
        1
    );
}

#[test]
fn a_date_covers_the_whole_local_day() {
    let paris = time::UtcOffset::from_hms(2, 0, 0).unwrap();
    let period = Period::parse_at(Some("2026-09-20"), Some("2026-09-20"), |_| paris).unwrap();

    let report = Report::new(&rounds(), Some(period));

    assert_eq!(report.period.as_ref().unwrap().1.all.rounds, 1);
    assert_eq!(report.period.as_ref().unwrap().1.challenger.rounds, 1);
}

#[test]
fn an_unknown_time_is_refused() {
    assert!(Period::parse(Some("yesterday"), None).is_err());
    assert!(Period::parse(None, Some("2026-13-01")).is_err());
}

#[test]
fn no_rounds_have_no_medians() {
    let report = Report::new(&SavedRounds::default(), None);
    let all = &report.all_time.all;

    assert_eq!(all.rounds, 0);
    assert_eq!(all.questions, None);
    assert_eq!(all.change_requests, Share { part: 0, whole: 0 });
    assert_eq!(all.agent_turn, None);
    assert_eq!(all.waiting_share, None);
    assert_eq!(all.question_words, None);
}

#[test]
fn proposals_count_once_at_their_latest_result_in_rounds_with_a_challenger() {
    let report = Report::new(&rounds(), None);
    let columns = &report.all_time;
    let counts = Some(ProposalCounts {
        rounds: 1,
        asked: 1,
        merged: 1,
        retired: 2,
        kept: 1,
    });

    assert_eq!(columns.challenger.proposals, counts);
    assert_eq!(columns.all.proposals, counts);
    assert_eq!(columns.without_challenger.proposals, None);
}

#[test]
fn back_to_the_drawing_board_declines_no_recommendation() {
    let mut stored: serde_json::Value = serde_json::from_str(ANSWERED).unwrap();
    // The first answer declined the recommended choice: it sends the agent back instead.
    let answer = &mut stored["value"]["exploration"]["answers"][0];
    answer["option"] = serde_json::Value::Null;
    answer["drawing_board"] = true.into();
    let round = saved(&stored.to_string(), SystemTime::UNIX_EPOCH);
    let rounds = SavedRounds {
        rounds: vec![round],
        unreadable: 0,
    };

    let report = Report::new(&rounds, None);

    assert_eq!(
        report.all_time.all.declined_recommendations,
        Share { part: 0, whole: 0 }
    );
}

#[test]
fn run_ahead_counts_the_prepared_turns_used_why_not_the_waits_and_the_fork_tokens() {
    let report = Report::new(&rounds(), None);
    let columns = &report.all_time;
    let expected = RunAheadSummary {
        rounds: 1,
        answers: Share { part: 1, whole: 2 },
        not_prepared: [("None of the above", 1)].into(),
        prepared_wait: Some(seconds(180)),
        plain_wait: Some(seconds(150)),
        forks: 5,
        ready: Share { part: 3, whole: 4 },
        used_tokens: TokenUsage {
            input: 10,
            cache_creation: 2000,
            cache_read: 50_000,
            output: 3000,
        },
        discarded_tokens: TokenUsage {
            input: 5,
            cache_creation: 1000,
            cache_read: 90_000,
            output: 1700,
        },
    };

    assert_eq!(columns.all.run_ahead, expected);
    assert_eq!(columns.challenger.run_ahead, expected);
}

#[test]
fn a_round_saved_without_a_run_ahead_record_counts_as_having_no_run_ahead() {
    let report = Report::new(&rounds(), None);

    assert_eq!(
        report.all_time.without_challenger.run_ahead,
        RunAheadSummary::default()
    );
    assert_eq!(report.all_time.without_challenger.answered, 1);
}

#[test]
fn a_prepared_turn_whose_switch_did_not_end_was_not_used() {
    let mut stored: serde_json::Value = serde_json::from_str(CHALLENGER_FORKS).unwrap();
    stored["value"]["forks"][0]["continued"] =
        serde_json::json!({ "state": "switching", "at_ms": 1_789_898_700_500_u64 });
    let mut rounds = rounds();
    rounds.rounds[1].run_ahead = forks(&stored.to_string());

    let report = Report::new(&rounds, None);
    let run_ahead = &report.all_time.all.run_ahead;

    assert_eq!(run_ahead.answers, Share { part: 0, whole: 2 });
    assert_eq!(
        run_ahead.not_prepared,
        [("None of the above", 1), ("prepared turn not settled", 1)].into()
    );
    assert_eq!(run_ahead.prepared_wait, None);
    assert_eq!(run_ahead.plain_wait, Some(seconds(165)));
    assert_eq!(run_ahead.used_tokens, TokenUsage::default());
    assert_eq!(
        report.all_time.all.agent_turn,
        Some(seconds(165)),
        "a turn no fork prepared is the agent's"
    );
}

#[test]
fn a_fork_taken_again_after_a_cancelled_answer_counts_at_the_next_answer() {
    let mut stored: serde_json::Value = serde_json::from_str(CHALLENGER_FORKS).unwrap();
    // An answer to the second question, before its last forks were taken, then cancelled.
    stored["value"]["answers"].as_array_mut().unwrap().insert(
        1,
        serde_json::json!({
            "question": "b-q2",
            "version": 1,
            "answer": "b-cancelled",
            "request": "b-r-cancelled",
            "at_ms": 1_789_898_950_000_u64,
            "path": { "path": "plain", "reason": { "reason": "comment" } }
        }),
    );
    let mut rounds = rounds();
    rounds.rounds[1].run_ahead = forks(&stored.to_string());

    let report = Report::new(&rounds, None);

    assert_eq!(
        report.all_time.all.run_ahead.ready,
        Share { part: 3, whole: 4 }
    );
}
