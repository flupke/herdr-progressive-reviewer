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
    let wait = Duration::from_millis(10);

    assert_eq!(agent.next_turn(None, wait).unwrap()["turn"], 1);
    assert_eq!(agent.next_turn(None, wait).unwrap()["turn"], 2);
    let none = agent.next_turn(None, wait).unwrap();
    assert_eq!(none["status"], "timeout");
    assert_eq!(none["latest"], 2);
    assert_eq!(agent.next_turn(Some(0), wait).unwrap()["turn"], 1);
}

#[test]
fn a_later_turn_never_overtakes_one_still_being_written() {
    let (_directory, mut agent) = agent(&[sent(2, "wakeup")]);

    let waiting = agent.next_turn(None, Duration::from_millis(10)).unwrap();

    assert_eq!(waiting["status"], "timeout");
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
