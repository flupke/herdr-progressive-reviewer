use super::*;

#[test]
fn the_first_answer_is_selected_by_default_and_enter_submits_it_once() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    assert!(fixture.text().contains("› 1. Keep resolved"));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    let answer = request.answer.unwrap();
    let option = answer.option.unwrap();
    assert_eq!(option.id, "keep");
    assert_eq!(option.text, "Keep resolved");
    assert_eq!(option.outcome, TopicStatus::Accepted);
    assert!(answer.text.is_empty());
    assert!(
        !fixture
            .app
            .update(UserInput::Key(Key::Enter))
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::Turn(_))))
    );
}

#[test]
fn arrows_and_jk_select_the_answer_that_enter_records() {
    let (mut fixture, request) = ExploreUi::new();
    let mut response = fixture.response(&request, 1);
    response.next.as_mut().unwrap().alternatives.truncate(1);
    response.next.as_mut().unwrap().alternatives.extend([
        Alternative {
            id: "change".into(),
            text: "Change the policy".into(),
            outcome: TopicStatus::NeedsFollowUp,
            recommendation: None,
        },
        Alternative {
            id: "later".into(),
            text: "Decide later".into(),
            outcome: TopicStatus::Deferred,
            recommendation: None,
        },
    ]);
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    for (key, label) in [
        (Key::Down, "› 2. Change the policy"),
        (Key::Char('j'), "› 3. Decide later"),
        (Key::Up, "› 2. Change the policy"),
        (Key::Char('k'), "› 1. Keep resolved"),
        (Key::Char('k'), "› 1. Keep resolved"),
        (Key::Down, "› 2. Change the policy"),
    ] {
        let actions = fixture.app.update(UserInput::Key(key));
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, Action::Explore(_)))
        );
        assert!(fixture.text().contains(label), "{}", fixture.text());
    }
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    let option = request.answer.unwrap().option.unwrap();
    assert_eq!(option.id, "change");
    assert_eq!(option.text, "Change the policy");
    assert_eq!(option.outcome, TopicStatus::NeedsFollowUp);
}

#[test]
fn none_of_the_above_can_be_supplemented_without_turning_editor_keys_into_choices() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Key(Key::Down));
    fixture.app.update(UserInput::Key(Key::Down));
    assert!(fixture.text().contains("› 3. None of the above"));
    assert!(!fixture.text().contains("Or answer in your own words"));
    fixture.click("Your answer");
    for key in [
        Key::Char('j'),
        Key::Char('k'),
        Key::Down,
        Key::Up,
        Key::Escape,
    ] {
        assert!(
            !fixture
                .app
                .update(UserInput::Key(key))
                .iter()
                .any(|action| matches!(action, Action::Explore(_)))
        );
    }
    assert!(fixture.text().contains("› 3. None of the above"));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    let answer = request.answer.unwrap();
    assert_eq!(answer.text, "jk");
    let option = answer.option.unwrap();
    assert_eq!(option.id, "none-of-the-above");
    assert_eq!(option.text, "None of the above");
    assert_eq!(option.outcome, TopicStatus::Open);
}

#[test]
fn clicking_a_choice_selects_it_and_send_includes_the_supplement() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.click("2. Inspect the caller");
    assert!(fixture.text().contains("› 2. Inspect the caller"));
    assert!(
        !fixture
            .text()
            .contains("Waiting for the implementation agent")
    );
    fixture.click("Your answer");
    let supplement = "  Include the retry path.\nPreserve pending work — please.  ";
    fixture.app.update(UserInput::Paste(supplement.into()));
    assert!(fixture.text().contains("› 2. Inspect the caller"));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    let answer = request.answer.unwrap();
    assert_eq!(answer.option.unwrap().id, "inspect");
    assert_eq!(answer.text, supplement);
    assert_eq!(answer.question.unwrap().version, 1);
}

#[test]
fn changing_the_choice_keeps_typed_details_and_none_can_be_sent_without_text() {
    for details in ["", "Only after the regression test."] {
        let (mut fixture, request) = ExploreUi::new();
        fixture.respond(&request, 1);
        fixture.app.update(UserInput::Paste(details.into()));
        fixture.app.update(UserInput::Key(Key::Tab));
        assert!(fixture.text().contains("› 1. Keep resolved"));
        for _ in 0..5 {
            fixture.app.update(UserInput::Key(Key::Down));
        }
        assert!(fixture.text().contains("› 3. None of the above"));
        let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
        let answer = request.answer.unwrap();
        assert_eq!(answer.option.unwrap().outcome, TopicStatus::Open);
        assert_eq!(answer.text, details);
    }
}

#[test]
fn the_question_and_answer_controls_precede_evidence() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    for width in [70, 140] {
        fixture.app.update(UserInput::Resize { width, height: 100 });
        let buffer = fixture.buffer();
        let rows: Vec<String> = buffer
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(ratatui::buffer::Cell::symbol).collect())
            .collect();
        let question = rows
            .iter()
            .position(|row| row.contains("Question 1:"))
            .unwrap();
        let choice = rows
            .iter()
            .position(|row| row.contains("1. Keep resolved"))
            .unwrap();
        let evidence = rows
            .iter()
            .position(|row| row.contains("Evidence 2 · Supporting"))
            .unwrap();
        let editor = rows
            .iter()
            .position(|row| row.contains("Your answer"))
            .unwrap();
        assert!(question < choice);
        assert!(choice < editor);
        assert!(editor < evidence);
        assert!(fixture.text().contains("The source supports this context."));
        assert!(
            rows[question + 1..choice]
                .iter()
                .all(|row| row.trim_matches(['│', ' ']).is_empty())
        );
    }
}

#[test]
fn number_keys_select_without_sending_including_none_after_five_alternatives() {
    let (mut fixture, request) = ExploreUi::new();
    let mut response = fixture.response(&request, 1);
    response
        .next
        .as_mut()
        .unwrap()
        .alternatives
        .extend((3..=5).map(|index| Alternative {
            id: format!("investigate-{index}"),
            text: format!("Investigate path {index} and its recovery behavior in every caller"),
            outcome: TopicStatus::Open,
            recommendation: None,
        }));
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    fixture.app.update(UserInput::Resize {
        width: 45,
        height: 14,
    });
    for key in ['2', '6'] {
        let actions = fixture.app.update(UserInput::Key(Key::Char(key)));
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, Action::Explore(_)))
        );
    }
    assert!(fixture.text().contains("› 6. None of the above"));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    assert_eq!(
        request.answer.unwrap().option.unwrap().id,
        "none-of-the-above"
    );
}

#[test]
fn moving_selection_reveals_the_answer_in_a_short_viewport() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 60,
        height: 9,
    });
    let mut response = fixture.response(&request, 1);
    response.next.as_mut().unwrap().alternatives.truncate(1);
    response
        .next
        .as_mut()
        .unwrap()
        .alternatives
        .push(Alternative {
            id: "later".into(),
            text: "Decide later".into(),
            outcome: TopicStatus::Deferred,
            recommendation: None,
        });
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    assert!(!fixture.text().contains("› 2. Decide later"));
    fixture.app.update(UserInput::Key(Key::Down));
    assert!(
        fixture.text().contains("› 2. Decide later"),
        "{}",
        fixture.text()
    );
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    assert_eq!(request.answer.unwrap().option.unwrap().id, "later");
}
