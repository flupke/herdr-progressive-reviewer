use super::*;

#[test]
fn a_new_pass_saves_its_view_only_after_its_first_post_is_durable() {
    let (mut fixture, first) = ExploreUi::new();
    let mut previous = pass(&fixture, &first);
    previous.revision = 50;
    restore(&mut fixture, &previous, None);
    fixture.app.update(UserInput::Key(Key::Char('n')));
    fixture.app.update(UserInput::Key(Key::Char('n')));
    let actions = fixture.app.publish(ui_events::ExploreCaptured {
        result: Ok(fixture.comparison.clone()),
    });
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::SaveView(_))))
    );
    let kickoff = ExploreUi::request(actions);
    assert_ne!(kickoff.instance, first.instance);
    let mut exploration = review_explore::Exploration::new(fixture.comparison.clone());
    exploration.instance.clone_from(&kickoff.instance);
    let mut new = ExplorePass::new(exploration);
    new.post(&kickoff).unwrap();
    new.revision = 1;
    let view = saved(fixture.app.publish(ui_events::ExplorePosted {
        request: kickoff.clone(),
        result: Ok(Arc::new(new)),
    }));
    assert_eq!(view.instance, kickoff.instance);
}

#[test]
fn cancelling_before_post_ack_keeps_the_answer_and_allows_exact_retry() {
    let (mut fixture, first) = ExploreUi::new();
    let mut pass = pass(&fixture, &first);
    restore(&mut fixture, &pass, None);
    let answer = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    fixture.app.update(UserInput::Key(Key::Char('c')));
    pass.post(&answer).unwrap();
    pass.revision += 1;
    fixture.app.publish(ui_events::ExplorePosted {
        request: answer.clone(),
        result: Ok(Arc::new(pass)),
    });
    assert!(fixture.text().contains("You: Keep resolved"));
    let retry = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Char('r'))));
    assert_eq!(retry, answer);
}

#[test]
fn a_post_ack_reconciles_newer_history_without_losing_the_current_draft() {
    let (mut fixture, first) = ExploreUi::new();
    let mut pass = pass(&fixture, &first);
    let question = pass.exploration.questions[0].clone();
    let answer = pass
        .exploration
        .request(
            Some(review_explore::AnswerInput {
                option: Some(question.alternatives[0].id.clone()),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    let mut concluded = fixture.response(&answer, 2);
    concluded.interpretation.as_mut().unwrap().status = TopicStatus::Accepted;
    concluded.next = None;
    concluded.topics.clear();
    concluded.conclusion = Some(conclusion("Agreed tasks"));
    pass.exploration.submit(concluded).unwrap();
    restore(&mut fixture, &pass, None);
    fixture.click("[Previous]");
    fixture.click("[Reply]");
    fixture.app.update(UserInput::Paste("Local context".into()));
    let local = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture
        .app
        .update(UserInput::Paste("Next local draft".into()));
    let external = pass
        .exploration
        .request(
            Some(review_explore::AnswerInput {
                text: "External context".into(),
                ..Default::default()
            }),
            None,
        )
        .unwrap();
    let mut advanced = fixture.response(&external, 2);
    advanced.interpretation = None;
    advanced.topics[0].id = "another-policy".into();
    advanced.next.as_mut().unwrap().id = "another-question".into();
    advanced.next.as_mut().unwrap().topic = "another-policy".into();
    pass.exploration.submit(advanced).unwrap();
    pass.post(&local).unwrap();
    pass.revision += 1;
    fixture.app.publish(ui_events::ExplorePosted {
        request: local,
        result: Ok(Arc::new(pass.clone())),
    });
    assert!(fixture.text().contains("Question 1/2"));
    assert!(fixture.text().contains("Next local draft"));
    let (response, received) = std::sync::mpsc::channel();
    fixture.app.publish(ui_events::ExploreCommitted {
        pass: Arc::new(pass),
        applied: false,
        response,
    });
    assert!(!received.recv().unwrap().unwrap());
    fixture.click("[Latest]");
    assert!(fixture.text().contains("Question 2/2"));
    fixture.click("[Previous]");
    assert!(fixture.text().contains("Agreed tasks"));
    fixture.click("[Previous]");
    assert!(fixture.text().contains("Next local draft"));
}

#[test]
fn an_old_post_ack_cannot_replace_a_newer_local_post_or_its_status() {
    let (mut fixture, first) = ExploreUi::new();
    let mut pass = pass(&fixture, &first);
    let question = pass.exploration.questions[0].clone();
    let context = pass
        .exploration
        .request(
            Some(review_explore::AnswerInput {
                text: "Initial context".into(),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    pass.exploration
        .submit(fixture.response(&context, 2))
        .unwrap();
    restore(&mut fixture, &pass, None);
    let old = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    fixture.app.update(UserInput::Key(Key::Char('c')));
    fixture.click("[Previous]");
    fixture.click("[Reply]");
    fixture
        .app
        .update(UserInput::Paste("New contribution".into()));
    let new = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    pass.post(&old).unwrap();
    pass.revision += 1;
    fixture.app.publish(ui_events::ExplorePosted {
        request: old.clone(),
        result: Ok(Arc::new(pass.clone())),
    });
    fixture.app.publish(ui_events::ExplorePosted {
        request: old,
        result: Err("Obsolete failure".into()),
    });
    assert!(!fixture.text().contains("Obsolete failure"));
    pass.exploration.cancel();
    pass.post(&new).unwrap();
    pass.revision += 1;
    fixture.app.publish(ui_events::ExplorePosted {
        request: new.clone(),
        result: Ok(Arc::new(pass)),
    });
    fixture.app.update(UserInput::Key(Key::Char('c')));
    assert_eq!(
        ExploreUi::request(fixture.app.update(UserInput::Key(Key::Char('r')))),
        new
    );
}

#[test]
fn edits_during_posting_are_queued_for_save_before_normal_close() {
    let (mut fixture, first) = ExploreUi::new();
    let mut pass = pass(&fixture, &first);
    restore(&mut fixture, &pass, None);
    let answer = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    let view = saved(
        fixture
            .app
            .update(UserInput::Paste("Next unposted draft".into())),
    );
    assert!(
        view.state
            .drafts
            .iter()
            .any(|(_, draft)| draft.editor.text == "Next unposted draft")
    );
    fixture.app.update(UserInput::Key(Key::Escape));
    assert!(
        fixture
            .app
            .update(UserInput::Key(Key::Char('q')))
            .contains(&Action::Quit)
    );
    pass.post(&answer).unwrap();
    pass.turns.get_mut(&answer.request).unwrap().editor_sequence = Some(view.sequence - 1);
    no_post(&restore(&mut fixture, &pass, Some(view)));
    assert!(fixture.text().contains("Next unposted draft"));
}

#[test]
fn a_combined_external_refresh_keeps_every_conclusion_and_existing_editor() {
    let (mut fixture, first) = ExploreUi::new();
    let mut pass = pass(&fixture, &first);
    restore(&mut fixture, &pass, None);
    for version in 2..=3 {
        let question = pass.exploration.questions.last().cloned();
        let request = pass
            .exploration
            .request(
                Some(review_explore::AnswerInput {
                    text: "Conclude".into(),
                    ..Default::default()
                }),
                question.as_ref(),
            )
            .unwrap();
        let mut update = fixture.response(&request, version);
        update.next = None;
        update.topics.clear();
        update.conclusion = Some(review_explore::Conclusion {
            summary: format!("Summary {version}"),
            to_be_implemented: format!("Tasks {version}"),
            future_work: String::new(),
        });
        pass.exploration.submit(update).unwrap();
        let request = pass
            .exploration
            .request(
                Some(review_explore::AnswerInput {
                    text: "Follow up".into(),
                    ..Default::default()
                }),
                None,
            )
            .unwrap();
        let mut update = fixture.response(&request, version + 2);
        update.interpretation = None;
        pass.exploration.submit(update).unwrap();
    }
    pass.revision += 1;
    let (response, received) = std::sync::mpsc::channel();
    fixture.app.publish(ui_events::ExploreCommitted {
        pass: Arc::new(pass.clone()),
        applied: true,
        response,
    });
    received.recv().unwrap().unwrap();
    fixture.click("[Previous]");
    assert!(fixture.text().contains("Tasks 3"));
    assert!(!fixture.text().contains("[Implement]"));
    fixture.click("To be implemented");
    fixture
        .app
        .update(UserInput::Paste("My existing edit: ".into()));
    fixture.app.update(UserInput::Key(Key::Escape));
    let (response, received) = std::sync::mpsc::channel();
    fixture.app.publish(ui_events::ExploreCommitted {
        pass: Arc::new(pass),
        applied: false,
        response,
    });
    received.recv().unwrap().unwrap();
    assert!(fixture.text().contains("My existing edit: Tasks 3"));
    fixture.click("[Previous]");
    fixture.click("[Previous]");
    assert!(fixture.text().contains("Tasks 2"));
    assert!(!fixture.text().contains("[Implement]"));
}
