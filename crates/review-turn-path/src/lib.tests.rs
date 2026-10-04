use std::collections::HashSet;

use super::*;

fn plain(reason: PlainReason) -> TurnPath {
    TurnPath::Plain { reason }
}

#[test]
fn each_reason_the_reviewer_can_use_has_a_line_of_its_own() {
    let shown = [
        TurnPath::Prepared {
            session: "fork".into(),
        },
        plain(PlainReason::Comment),
        plain(PlainReason::NoneOfTheAbove),
        plain(PlainReason::NoForks { why: None }),
        plain(PlainReason::NoForks {
            why: Some("the agent is Working".into()),
        }),
        plain(PlainReason::NotForked),
        plain(PlainReason::StillWorking),
        plain(PlainReason::NoTurn),
        plain(PlainReason::ChatMessage),
        plain(PlainReason::AgentBusy),
        plain(PlainReason::InputNotEmpty),
        plain(PlainReason::SessionMoved),
        plain(PlainReason::UnreviewedChanged),
        plain(PlainReason::PromptChanged),
    ];

    let lines: Vec<_> = shown.iter().map(TurnPath::line).collect();

    assert!(lines.iter().all(Option::is_some), "{lines:?}");
    let distinct: HashSet<_> = lines.iter().collect();
    // The two `NoForks` say the same.
    assert_eq!(distinct.len(), shown.len() - 1);
}

#[test]
fn the_tool_s_own_faults_and_a_withdrawn_answer_show_nothing() {
    for reason in [
        PlainReason::Unchecked {
            error: "the agent's pane is gone".into(),
        },
        PlainReason::SwitchFailed {
            error: "the agent is working".into(),
        },
        PlainReason::Withdrawn,
    ] {
        assert_eq!(plain(reason.clone()).line(), None, "{reason:?}");
    }
}
