//! Past rounds on the page's socket: listed beside the round the page follows, and shown read
//! only on a socket of their own, which refuses every action.

use super::*;
use crate::{PastChange, PastRound, PastRoundEntry, PastRounds, PastRoundsPublisher, ReviewName};

const PAST: &str = "past-1";

/// The saved round `PAST`, left at a one-way question it asked.
fn past_round() -> PastRound {
    past_round_at(asking(question("q0", "one_way")))
}

/// The saved round `PAST`, of the review the page follows, left at `stage`; the threads read with
/// it hold no message.
fn past_round_at(stage: RoundStage) -> PastRound {
    let review: ReviewUnit = REVIEW.into();
    let overview = RoundOverview {
        rail: Vec::new(),
        decisions: Vec::new(),
        earlier: Vec::new(),
        title: TabTitle::AgentWorking,
        glossary: std::collections::BTreeMap::new(),
    };
    let round = PublishedRound {
        id: PAST,
        review_unit: &review,
        design: None,
        changed_files: 1,
        author: review_explore::ChangeAuthor::Reviewer,
        cancellable: None,
        earlier: false,
        overview: &overview,
        earlier_citations: &[],
    };
    let name = ReviewName {
        title: "Name the queue's flush policy".into(),
        ..ReviewName::default()
    };
    PastRound::new(round, stage, name, None, 1_000)
}

/// The change of the saved rounds `rounds`, saved in that order, newest first.
fn change(rounds: &[&str]) -> PastChange {
    PastChange {
        title: "Name the queue's flush policy".into(),
        revision: review_repository::repository::ShortRevision::default(),
        rounds: rounds
            .iter()
            .map(|id| PastRoundEntry {
                id: (*id).into(),
                saved_at_ms: 1_000,
            })
            .collect(),
    }
}

/// `owner`, whose page lists `changes` and opens the round `PAST`.
fn with_past(owner: Owner, publisher: &PastRoundsPublisher, changes: Vec<PastChange>) -> Owner {
    with_past_round(owner, publisher, changes, past_round)
}

/// `owner`, whose page lists `changes` and opens the round `PAST` that `past` makes.
fn with_past_round(
    mut owner: Owner,
    publisher: &PastRoundsPublisher,
    changes: Vec<PastChange>,
    past: fn() -> PastRound,
) -> Owner {
    publisher.publish(changes);
    owner.past = Some(PastRounds::new(publisher, move |id| {
        (id == PAST).then(past)
    }));
    owner
}

/// The view of the socket's first message.
async fn first_view(socket: &mut Socket) -> Value {
    next(socket).await["params"]["view"].clone()
}

#[tokio::test]
async fn the_page_lists_the_saved_rounds_but_the_one_it_follows() {
    let publisher = PastRoundsPublisher::default();
    // The round the page follows is saved too, with the past round of its change.
    let changes = vec![change(&[ROUND, PAST]), change(&[ROUND])];
    let owner = with_past(Owner::new(no_round()), &publisher, changes);
    let address = serve(owner).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();

    let view = first_view(&mut socket).await;

    let listed = &view["past_rounds"];
    assert_eq!(listed.as_array().map(Vec::len), Some(1), "{listed}");
    assert_eq!(listed[0]["rounds"][0]["id"], PAST);
    assert_eq!(listed[0]["rounds"].as_array().map(Vec::len), Some(1));
    assert!(view["past"].is_null());

    // A round the reviewer resets is listed at once.
    publisher.publish(vec![change(&["past-2", PAST])]);
    let view = next(&mut socket).await["params"]["view"].clone();
    assert_eq!(view["past_rounds"][0]["rounds"][0]["id"], "past-2");
}

#[tokio::test]
async fn a_past_round_shows_read_only_on_a_socket_of_its_own() {
    let publisher = PastRoundsPublisher::default();
    let owner = with_past(
        Owner::new(asking(question("q1", "two_way"))),
        &publisher,
        vec![change(&[PAST])],
    );
    let address = serve(owner.clone()).await;
    let path = format!("/ws?past={PAST}");
    let mut socket = Upgrade::of(address)
        .connect_to(address, &path)
        .await
        .unwrap();

    let view = first_view(&mut socket).await;

    assert_eq!(view["past"]["round"], PAST);
    // The round the page follows asks a question meanwhile.
    assert_eq!(view["past"]["current_waits"], true);
    assert_eq!(view["review"]["title"], "Name the queue's flush policy");
    let question = &view["question"];
    assert_eq!(question["id"], "q0");
    assert_eq!(question["answerable"], false);
    // A one-way question of a past round shows the recommendation it showed then.
    assert_eq!(question["recommendation"], "shown");
    assert!(view["reset"].is_null());
    assert!(
        view["cards"].as_array().unwrap().is_empty(),
        "{}",
        view["cards"]
    );
    assert_eq!(view["past_rounds"][0]["rounds"][0]["id"], PAST);
}

#[tokio::test]
async fn a_socket_that_shows_a_past_round_refuses_every_action_but_a_ping() {
    let publisher = PastRoundsPublisher::default();
    let owner = with_past(
        Owner::new(asking(question("q0", "two_way"))),
        &publisher,
        vec![change(&[PAST])],
    );
    let address = serve(owner.clone()).await;
    let path = format!("/ws?past={PAST}");
    let mut socket = Upgrade::of(address)
        .connect_to(address, &path)
        .await
        .unwrap();
    next(&mut socket).await;

    // The same question as the one the page follows, by its identity: still refused.
    let answer =
        json!({ "round": ROUND, "question": "q0", "version": 1, "choice": "keep", "comment": "" });
    let card = refusal(&mut socket, 1, "answer", answer).await;
    assert_eq!(card["title"], "This past round is read only");
    let reset = json!({ "round": PAST });
    refusal(&mut socket, 2, "reset", reset).await;
    let message = message("00000000-0000-4000-8000-000000000001", PAST);
    refusal(&mut socket, 3, "send-message", message).await;
    request(&mut socket, json!({ "id": 4, "method": "ping" })).await;
    assert!(reply(&mut socket, 4).await["error"].is_null());

    assert_eq!(owner.commands(), Vec::<String>::new());
    assert_eq!(owner.thread_commands(), Vec::<String>::new());
}

#[tokio::test]
async fn a_socket_for_a_round_no_one_saved_shows_the_round_the_page_follows() {
    let publisher = PastRoundsPublisher::default();
    let owner = with_past(
        Owner::new(asking(question("q1", "two_way"))),
        &publisher,
        vec![change(&[PAST])],
    );
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address)
        .connect_to(address, "/ws?past=gone-1")
        .await
        .unwrap();

    let view = first_view(&mut socket).await;

    assert!(view["past"].is_null());
    assert_eq!(view["question"]["id"], "q1");
    let answer =
        json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "keep", "comment": "" });
    request(
        &mut socket,
        json!({ "id": 1, "method": "answer", "params": answer }),
    )
    .await;
    assert_eq!(applied(&reply(&mut socket, 1).await), Some(true));
}

#[tokio::test]
async fn a_page_that_offers_no_past_round_neither_lists_nor_opens_one() {
    // As the page on the network: its round has no past rounds.
    let address = serve(Owner::new(asking(question("q1", "two_way")))).await;
    let path = format!("/ws?past={PAST}");
    let mut socket = Upgrade::of(address)
        .connect_to(address, &path)
        .await
        .unwrap();

    let view = first_view(&mut socket).await;

    assert!(view["past_rounds"].is_null());
    assert!(view["past"].is_null());
    assert_eq!(view["question"]["id"], "q1");
}

#[tokio::test]
async fn a_past_round_that_stopped_on_a_turn_says_so_with_no_action() {
    let publisher = PastRoundsPublisher::default();
    let stopped = || past_round_at(interrupted("turn-3", "attempt-1"));
    let owner = with_past_round(
        Owner::new(no_round()),
        &publisher,
        vec![change(&[PAST])],
        stopped,
    );
    let address = serve(owner).await;
    let path = format!("/ws?past={PAST}");
    let mut socket = Upgrade::of(address)
        .connect_to(address, &path)
        .await
        .unwrap();

    let view = first_view(&mut socket).await;

    let cards = view["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert_eq!(
        cards[0]["title"],
        "The agent had not answered when the round was saved"
    );
    assert_eq!(cards[0]["actions"], json!([]), "{}", cards[0]);
    // The round the page follows waits for no answer.
    assert_eq!(view["past"]["current_waits"], false);
}

#[tokio::test]
async fn a_socket_asked_for_the_round_the_page_follows_shows_it_live() {
    let publisher = PastRoundsPublisher::default();
    publisher.publish(vec![change(&[ROUND])]);
    let mut owner = Owner::new(asking(question("q1", "two_way")));
    // A store that would open any round, the followed one too.
    owner.past = Some(PastRounds::new(&publisher, |_| Some(past_round())));
    let address = serve(owner).await;
    let path = format!("/ws?past={ROUND}");
    let mut socket = Upgrade::of(address)
        .connect_to(address, &path)
        .await
        .unwrap();

    let view = first_view(&mut socket).await;

    assert!(view["past"].is_null());
    assert_eq!(view["question"]["answerable"], true);
}

#[tokio::test]
async fn a_past_rounds_conversation_follows_the_threads_the_page_holds() {
    let publisher = PastRoundsPublisher::default();
    let owner = with_past(Owner::new(no_round()), &publisher, vec![change(&[PAST])]);
    let post = |text: &str| {
        let mut book = lock(&owner.book);
        book.post(Post::to_round(PAST, text.into(), None, None))
            .unwrap();
        owner.threads.loaded(book.clone());
    };
    post("Why keep the draft?");
    let address = serve(owner.clone()).await;
    let path = format!("/ws?past={PAST}");
    let mut socket = Upgrade::of(address)
        .connect_to(address, &path)
        .await
        .unwrap();

    let view = first_view(&mut socket).await;
    assert_eq!(view["conversation"]["round"], PAST);
    assert_eq!(
        view["conversation"]["messages"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(view["conversation"]["writable"], false);

    // A message posted meanwhile, in another review pane, shows at once.
    post("And the old drafts?");
    let view = next(&mut socket).await["params"]["view"].clone();
    assert_eq!(
        view["conversation"]["messages"].as_array().map(Vec::len),
        Some(2)
    );
}

#[tokio::test]
async fn a_past_round_saved_again_shows_again_on_the_open_socket() {
    let publisher = PastRoundsPublisher::default();
    publisher.publish(vec![change(&[PAST])]);
    // The round, as another review pane saves it: at a turn, then at its next question.
    let saved = Arc::new(Mutex::new(interrupted("turn-1", "attempt-1")));
    let store = saved.clone();
    let mut owner = Owner::new(no_round());
    owner.past = Some(PastRounds::new(&publisher, move |id| {
        let stage = lock(&store).clone();
        (id == PAST).then(|| past_round_at(stage))
    }));
    let address = serve(owner).await;
    let path = format!("/ws?past={PAST}");
    let mut socket = Upgrade::of(address)
        .connect_to(address, &path)
        .await
        .unwrap();
    assert!(first_view(&mut socket).await["question"].is_null());

    *lock(&saved) = asking(question("q2", "two_way"));
    let mut later = change(&[PAST]);
    later.rounds[0].saved_at_ms = 2_000;
    publisher.publish(vec![later]);

    let view = next(&mut socket).await["params"]["view"].clone();
    assert_eq!(view["question"]["id"], "q2");
}
