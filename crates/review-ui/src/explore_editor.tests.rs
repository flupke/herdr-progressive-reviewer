use super::*;

impl ExploreUi {
    fn assert_editor_key(&mut self, key: Key, mode: &str) {
        let actions = self.app.update(UserInput::Key(key));
        assert!(
            !actions.iter().any(|action| matches!(
                action,
                Action::Explore(_) | Action::Quit | Action::Thread(_) | Action::SetReviewed { .. }
            )),
            "editor key {key:?} must not run review commands"
        );
        let text = self.text();
        assert!(text.contains(mode), "{key:?}: {text}");
    }

    fn exercise_vim_escape(&mut self) {
        for (key, mode) in [
            (Key::EditorMode, "NORMAL"),
            (Key::Char('i'), "INSERT"),
            (Key::Escape, "NORMAL"),
            (Key::Escape, "NORMAL"),
            (Key::Char('v'), "VISUAL"),
            (Key::Escape, "NORMAL"),
            (Key::Char('/'), "SEARCH"),
            (Key::Escape, "NORMAL"),
            (Key::Char('d'), "NORMAL"),
            (Key::Escape, "NORMAL"),
            (Key::Char('q'), "NORMAL"),
            (Key::Escape, "NORMAL"),
            (Key::Enter, "NORMAL"),
        ] {
            self.assert_editor_key(key, &format!("Vim · {mode}"));
        }
    }
}

#[test]
fn escape_uses_shared_editor_modes_without_submitting_or_changing_choices() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Paste("Answer draft".into()));
    fixture.exercise_vim_escape();
    assert!(fixture.text().contains("› 1. Keep resolved"));
    fixture.assert_editor_key(Key::EditorMode, "Regular editing");
    fixture.assert_editor_key(Key::Escape, "Regular editing");
    fixture.assert_editor_key(Key::Last, "Regular editing");
    fixture.assert_editor_key(Key::Char('j'), "Regular editing");
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Down));
    assert!(fixture.text().contains("› 2. Inspect the caller"));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    let answer = request.answer.unwrap();
    assert_eq!(answer.text, "Answer draftj");
    assert_eq!(answer.option.unwrap().id, "inspect");
}

#[test]
fn conclusion_task_and_reply_editors_keep_vim_escape_inside_the_active_field() {
    for reply in [false, true] {
        let (mut fixture, request) = ExploreUi::new();
        let mut response = fixture.response(&request, 1);
        response.next = None;
        response.conclusion = Some(review_explore::Conclusion {
            summary: "Review complete; human Files inspection remains required.".into(),
            to_be_implemented: "Agreed task".into(),
            future_work: String::new(),
        });
        fixture.app.publish(ExploreFinished {
            instance: request.instance,
            request: request.request,
            result: Ok(response),
        });
        fixture.click(if reply {
            "[Reply]"
        } else {
            "To be implemented"
        });
        if reply {
            fixture
                .app
                .update(UserInput::Paste("Conclusion question".into()));
        }
        fixture.exercise_vim_escape();
        let actions = fixture.app.update(UserInput::Key(Key::ControlEnter));
        if reply {
            let answer = ExploreUi::request(actions).answer.unwrap();
            assert!(answer.question.is_none());
            assert_eq!(answer.text, "Conclusion question");
        } else {
            let request = actions
                .into_iter()
                .find_map(|action| match action {
                    Action::Explore(Command::Implement(request)) => Some(request),
                    _ => None,
                })
                .expect("explicit implementation submission");
            assert_eq!(request.text, "Agreed task");
        }
    }
}
