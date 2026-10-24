use super::*;

#[test]
fn a_join_answer_gives_the_session_status_with_its_flags() {
    let answer = r#"{"id":2,"result":{"thread":{"id":"t1","status":
        {"type":"active","activeFlags":["waitingOnApproval"]}}}}"#;

    assert_eq!(
        read(answer),
        Incoming::Answer {
            id: JOIN,
            result: Ok(Some(SessionStatus::Active)),
        }
    );
}

#[test]
fn a_refused_join_gives_the_daemon_s_words() {
    let answer =
        r#"{"id":2,"error":{"code":-32600,"message":"no rollout found for thread id t1"}}"#;

    assert_eq!(
        read(answer),
        Incoming::Answer {
            id: JOIN,
            result: Err("no rollout found for thread id t1".to_owned()),
        }
    );
}

#[test]
fn a_request_of_the_daemon_is_no_answer_even_with_the_join_s_id() {
    let request = r#"{"id":2,"method":"item/commandExecution/requestApproval",
        "params":{"threadId":"t1","turnId":"u1","itemId":"i1"}}"#;

    assert_eq!(read(request), Incoming::Other);
}

#[test]
fn a_user_message_gives_the_prompt_of_its_turn_without_its_images() {
    let item = r#"{"method":"item/completed","params":{"threadId":"t1","turnId":"u1",
        "completedAtMs":1,"item":{"type":"userMessage","id":"m1","clientId":null,"content":[
        {"type":"text","text":"first line","text_elements":[]},
        {"type":"localImage","path":"/tmp/shot.png"},
        {"type":"text","text":"second line","text_elements":[]}]}}}"#;

    assert_eq!(
        read(item),
        Incoming::Notice {
            session: "t1".to_owned(),
            notice: Notice::Event(SessionEvent::Prompt {
                turn: "u1".to_owned(),
                text: "first line\nsecond line".to_owned(),
            }),
        }
    );
}

#[test]
fn another_item_says_nothing() {
    let item = r#"{"method":"item/completed","params":{"threadId":"t1","turnId":"u1",
        "completedAtMs":1,"item":{"type":"agentMessage","id":"m2","text":"done"}}}"#;

    assert_eq!(read(item), Incoming::Other);
}

#[test]
fn a_completed_turn_says_how_it_ended() {
    let ended = |status: &str| {
        read(&format!(
            r#"{{"method":"turn/completed","params":{{"threadId":"t1",
            "turn":{{"id":"u1","items":[],"status":"{status}"}}}}}}"#
        ))
    };
    let end = |end| Incoming::Notice {
        session: "t1".to_owned(),
        notice: Notice::Event(SessionEvent::TurnEnded {
            turn: "u1".to_owned(),
            end,
        }),
    };

    assert_eq!(ended("completed"), end(TurnEnd::Completed));
    assert_eq!(ended("interrupted"), end(TurnEnd::Interrupted));
    assert_eq!(ended("failed"), end(TurnEnd::Failed));
}

#[test]
fn a_closed_session_is_no_longer_loaded() {
    assert_eq!(
        read(r#"{"method":"thread/closed","params":{"threadId":"t1"}}"#),
        Incoming::Notice {
            session: "t1".to_owned(),
            notice: Notice::Event(SessionEvent::Status(SessionStatus::NotLoaded)),
        }
    );
}

#[test]
fn a_goal_a_turn_changed_is_no_snapshot() {
    let snapshot = Incoming::Notice {
        session: "t1".to_owned(),
        notice: Notice::GoalSnapshot,
    };
    let goal = r#"{"objective":"ship it","status":"active"}"#;

    assert_eq!(
        read(r#"{"method":"thread/goal/cleared","params":{"threadId":"t1"}}"#),
        snapshot
    );
    assert_eq!(
        read(&format!(
            r#"{{"method":"thread/goal/updated","params":{{"threadId":"t1","turnId":null,"goal":{goal}}}}}"#
        )),
        snapshot
    );
    assert_eq!(
        read(&format!(
            r#"{{"method":"thread/goal/updated","params":{{"threadId":"t1","turnId":"u1","goal":{goal}}}}}"#
        )),
        Incoming::Other
    );
}
