use super::*;

/// An agent reading `turns` from a new directory.
fn agent(turns: &[Value]) -> (tempfile::TempDir, ScriptedAgent) {
    let directory = tempfile::tempdir().unwrap();
    for turn in turns {
        let path = directory
            .path()
            .join(format!("turn-{:06}.json", number(turn)));
        std::fs::write(path, serde_json::to_vec(turn).unwrap()).unwrap();
    }
    let agent = ScriptedAgent::new(directory.path().to_owned(), 0);
    (directory, agent)
}

fn sent(turn: u64, kind: &str) -> Value {
    json!({
        "turn": turn, "kind": kind, "delivered": true,
        "access": format!("access-{turn}"), "round": "round", "request": format!("request-{turn}"),
        "checkpoint": {"review_unit": "unit", "checkpoint": "commit"},
        "text": "prompt",
    })
}

#[test]
fn turns_are_returned_once_each_in_order() {
    let (_directory, mut agent) = agent(&[sent(1, "kickoff"), sent(2, "wakeup")]);
    // No wait: the turns are there, or the call reports that none came.
    let wait = Duration::ZERO;

    assert_eq!(agent.next_turn(None, wait).unwrap()["turn"], 1);
    assert_eq!(agent.next_turn(None, wait).unwrap()["turn"], 2);
    assert_eq!(
        agent.next_turn(None, wait).unwrap_err().to_string(),
        "the reviewer sent no turn after turn 2 in 0 ms; the latest is 2"
    );
    assert_eq!(agent.next_turn(Some(0), wait).unwrap()["turn"], 1);
}

#[test]
fn a_later_turn_never_overtakes_one_still_being_written() {
    let (_directory, mut agent) = agent(&[sent(2, "wakeup")]);

    assert!(agent.next_turn(None, Duration::ZERO).is_err());
}

#[test]
fn a_turn_written_while_waiting_ends_the_wait() {
    let (directory, mut agent) = agent(&[]);
    let turns = directory.path().to_owned();
    // The turn is written once the wait watches and found none, so only the watcher's event
    // can end the wait; the guard fires only if the test fails.
    let turn = agent.next_turn_watching(None, Duration::from_secs(30), move || {
        let partial = turns.join("turn-000001.json.partial");
        std::fs::write(&partial, serde_json::to_vec(&sent(1, "kickoff")).unwrap()).unwrap();
        std::fs::rename(partial, turns.join("turn-000001.json")).unwrap();
    });
    assert_eq!(turn.unwrap()["turn"], 1);
}

#[test]
fn a_reply_must_answer_the_latest_turn() {
    let (_directory, agent) = agent(&[sent(1, "kickoff"), sent(2, "wakeup")]);

    let error = agent
        .reply(1, "submit_question", json!({}))
        .unwrap_err()
        .to_string();

    assert_eq!(error, "turn 1 is stale; latest is 2");
}

#[test]
fn undelivered_and_implement_turns_take_no_reply() {
    let mut failed = sent(1, "kickoff");
    failed["delivered"] = false.into();
    failed["error"] = "the pane is gone".into();
    let (_directory, undelivered) = agent(&[failed]);
    let error = undelivered
        .reply(1, "submit_question", json!({}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("was not delivered"), "{error}");

    let implement = json!({"turn": 1, "kind": "implement", "delivered": true, "text": "tasks"});
    let (_directory, implementing) = agent(&[implement]);
    let error = implementing
        .reply(1, "submit_conclusion", json!({}))
        .unwrap_err()
        .to_string();
    assert_eq!(error, "turn 1 is an implement prompt; it takes no reply");
}

#[test]
fn a_turn_fills_the_identity_the_arguments_leave_out() {
    let identity = Identity::of(&sent(2, "wakeup")).unwrap();
    let checkpoint = json!({"review_unit": "unit", "checkpoint": "commit"});

    let mut question = json!({"update": {"request": "chosen"}});
    identity.fill("submit_question", &mut question).unwrap();
    assert_eq!(
        question,
        json!({"review": "access-2", "update": {
            "instance": "round", "request": "chosen", "checkpoint": checkpoint,
        }})
    );

    let mut conclusion = json!({"summary": "Done"});
    identity.fill("submit_conclusion", &mut conclusion).unwrap();
    assert_eq!(
        conclusion,
        json!({
            "review": "access-2", "summary": "Done", "instance": "round",
            "request": "request-2", "checkpoint": checkpoint,
        })
    );
}

#[test]
fn an_interpretation_is_about_the_turns_answer_unless_it_names_one() {
    let mut turn = sent(2, "wakeup");
    turn["answer"] = json!({"id": "answer-1"});
    let identity = Identity::of(&turn).unwrap();

    let mut conclusion = json!({"interpretation": {"status": "accepted"}});
    identity.fill("submit_conclusion", &mut conclusion).unwrap();
    assert_eq!(conclusion["interpretation"]["answer"], "answer-1");

    let mut question = json!({"update": {"interpretation": {"answer": "chosen"}}});
    identity.fill("submit_question", &mut question).unwrap();
    assert_eq!(question["update"]["interpretation"]["answer"], "chosen");

    let mut none = json!({"interpretation": null});
    identity.fill("submit_conclusion", &mut none).unwrap();
    assert_eq!(none["interpretation"], Value::Null);
}
