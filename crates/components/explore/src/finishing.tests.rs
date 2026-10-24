//! The pane while the session waits for the fork that finishes the turn of the reviewer's bare
//! answer.

use std::time::{Duration, Instant};

use review_turn_path::{PlainReason, TurnPath};
use ui_events::{ExploreTurnFinishing, ExploreTurnPath};

use super::*;

const FINISHING: &str = "The agent is finishing a turn it started while you were thinking";

/// The pane that answered the first question of its round with its first choice, as the
/// session saved it, and the answer's turn.
fn answered() -> (Pane, TurnRequest) {
    let round = asked_round();
    let mut pane = Pane::showing(&round);
    let component = pane.component();
    component.select_choice(0);
    let actions = component.answer(Control::Send);
    let [Action::Explore(review_explore::Command::Turn(request))] = actions.as_slice() else {
        panic!("the pane sends a turn: {actions:?}");
    };
    let request = (**request).clone();
    let mut saved = round;
    assert!(saved.post(&request).unwrap());
    saved.revision += 1;
    pane.bus
        .publish(ExplorePosted {
            request: request.clone(),
            result: Ok(Arc::new(saved)),
        })
        .unwrap();
    (pane, request)
}

/// The session waits for the fork of `request`, which started it `ago` before the pane hears of
/// it at `now`.
fn finishing_at(pane: &mut Pane, request: &TurnRequest, ago: Duration, now: Instant) {
    const NOW_MS: u64 = 1_800_000_000_000;
    let started_at_ms = NOW_MS - u64::try_from(ago.as_millis()).expect("a short time");
    let event = ExploreTurnFinishing {
        round: request.instance.clone(),
        request: request.request.clone(),
        started_at_ms,
    };
    pane.component().finishing_since(&event, NOW_MS, now);
}

/// The session waits for the fork of `request`, which started it `ago`.
fn finishing(pane: &mut Pane, request: &TurnRequest, ago: Duration) {
    finishing_at(pane, request, ago, Instant::now());
}

#[test]
fn the_pane_says_the_agent_finishes_the_fork_s_turn_with_the_time_since_it_started() {
    let (mut pane, request) = answered();

    finishing(&mut pane, &request, Duration::from_millis(62_400));

    let rows = pane.answers();
    assert!(
        rows.iter()
            .any(|row| row.contains(&format!("{FINISHING} · 1:02"))),
        "{rows:?}"
    );
    let now = Instant::now();
    let component = pane.component();
    assert!(
        component.finishing_ticks_between(now, now + Duration::from_secs(1)),
        "the pane counts on"
    );
    component.finishing_tick(&ui_events::ToastExpirationTick {
        now: now + Duration::from_secs(2),
    });
    let rows = pane.answers();
    assert!(
        rows.iter()
            .any(|row| row.contains(&format!("{FINISHING} · 1:04"))),
        "{rows:?}"
    );
}

#[test]
fn once_the_turn_s_path_is_known_the_pane_says_it_waits_for_the_agent() {
    let (mut pane, request) = answered();
    finishing(&mut pane, &request, Duration::from_secs(5));

    pane.bus
        .publish(ExploreTurnPath {
            round: request.instance.clone(),
            request: request.request.clone(),
            path: TurnPath::Plain {
                reason: PlainReason::StoppedWaiting,
            },
        })
        .unwrap();

    let rows = pane.answers();
    assert!(!rows.iter().any(|row| row.contains(FINISHING)), "{rows:?}");
    assert!(
        rows.iter()
            .any(|row| row.contains("Waiting for the implementation agent")),
        "{rows:?}"
    );
    let now = Instant::now();
    assert!(
        !pane
            .component()
            .finishing_ticks_between(now, now + Duration::from_secs(1)),
        "nothing to count"
    );
}

#[test]
fn stopping_the_wait_says_the_answer_goes_to_the_agent_rather_than_to_retry() {
    let (mut pane, request) = answered();
    finishing(&mut pane, &request, Duration::from_secs(5));

    let _ = pane.component().cancel();

    let rows = pane.answers();
    assert!(!rows.iter().any(|row| row.contains(FINISHING)), "{rows:?}");
    assert!(
        rows.iter()
            .any(|row| row.contains("your answer goes to the agent")),
        "{rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("Retry sends")),
        "{rows:?}"
    );
}
