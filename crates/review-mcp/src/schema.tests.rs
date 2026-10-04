use super::Handler;
use serde_json::json;

fn explore_result(shown_as: Option<usize>) -> serde_json::Value {
    let result = Handler::result(super::Response::Explore {
        applied: false,
        shown_as: shown_as.map(Into::into),
    });
    let text = result.content[0].as_text().unwrap();
    serde_json::from_str(&text.text).unwrap()
}

#[test]
fn a_conclusion_result_says_only_whether_the_turn_was_new() {
    assert_eq!(
        explore_result(None),
        json!({"accepted": true, "applied": false})
    );
}

#[test]
fn a_question_result_gives_the_number_the_reviewer_sees() {
    assert_eq!(
        explore_result(Some(3)),
        json!({"accepted": true, "applied": false, "shown_as": "Q3"})
    );
}

#[test]
fn a_round_conversation_names_its_round_and_each_message_its_question_and_quote() {
    let mut book = review_threads::ReviewThreads::new("change".into());
    book.post(review_threads::Post::to_round(
        "round-1",
        "Why a lock?".into(),
        Some(review_threads::AskedUnder::Question {
            question: "q-lock".into(),
            version: 2,
            number: None,
        }),
        Some("the store takes a lock".into()),
    ))
    .unwrap();
    let thread = book.round_conversation("round-1").unwrap().clone();
    let result = Handler::result(super::Response::Threads(vec![thread.clone()]));
    let text = result.content[0].as_text().unwrap();
    let value: serde_json::Value = serde_json::from_str(&text.text).unwrap();

    let fetched = &value["threads"][0];
    assert_eq!(fetched["thread_id"], json!(thread.id));
    assert_eq!(fetched["explore_round"], "round-1");
    assert_eq!(fetched["in_reply_to"], json!(thread.messages[0].id));
    assert_eq!(fetched.get("code_context"), None);
    let message = &fetched["messages"][0];
    assert_eq!(
        message["asked_under"],
        json!({"stage": "question", "question": "q-lock", "version": 2})
    );
    assert_eq!(message["quote"], "the store takes a lock");
}

#[test]
fn explore_tool_schemas_describe_the_full_submission_without_a_kickoff_example() {
    let tools = Handler::tools();
    let question = tools
        .iter()
        .find(|tool| tool.name == "submit_question")
        .unwrap();
    let schema = &question.input_schema;
    assert_eq!(
        schema["properties"]["update"]["$ref"],
        "#/$defs/InterviewUpdate"
    );
    let definitions = &schema["$defs"];
    for name in [
        "InterviewUpdate",
        "Alternative",
        "Topic",
        "AgendaChange",
        "Reply",
        "Design",
        "DesignPart",
        "Interpretation",
        "Assessments",
        "Consequence",
        "EvidenceRef",
        "CodeLocation",
        "ReviewCheckpoint",
        "SourceLineRange",
    ] {
        assert_eq!(definitions[name]["type"], "object", "{name}");
        assert!(
            !definitions[name]["properties"]
                .as_object()
                .unwrap()
                .is_empty(),
            "{name}"
        );
    }
    let update = &definitions["InterviewUpdate"];
    assert!(
        update["required"]
            .as_array()
            .unwrap()
            .contains(&json!("next"))
    );
    assert_eq!(update["properties"]["next"]["type"], "object");
    for marks in ["reviewed", "reopened"] {
        assert_eq!(
            update["properties"][marks]["items"]["$ref"],
            "#/$defs/CodeLocation"
        );
    }
    assert!(update["properties"].get("inspections").is_none());
    assert!(update["properties"].get("conclusion").is_none());
    assert_eq!(update["additionalProperties"], false);
    let question = &update["properties"]["next"]["properties"];
    assert_eq!(question["alternatives"]["minItems"], 2);
    assert_eq!(question["alternatives"]["maxItems"], 5);
    assert_eq!(question["evidence"]["items"]["$ref"], "#/$defs/EvidenceRef");
    assert_eq!(
        definitions["EvidenceRef"]["properties"]["notes"]["type"],
        "string"
    );
    assert_eq!(
        definitions["TopicStatus"]["enum"],
        json!(["open", "accepted", "needs_follow_up"])
    );
    assert_eq!(definitions["SourceSide"]["enum"], json!(["old", "new"]));
    assert_eq!(definitions["ReviewUnit"]["type"], "string");
    let paths = definitions["PathInput"]["anyOf"].as_array().unwrap();
    assert!(paths.iter().any(|path| path["type"] == "string"));
    assert!(
        paths
            .iter()
            .any(|path| path["type"] == "array" && path["items"]["type"] == "integer")
    );
}

#[test]
fn the_conclusion_tool_takes_its_sections_and_marks() {
    let tools = Handler::tools();
    let conclusion = tools
        .iter()
        .find(|tool| tool.name == "submit_conclusion")
        .unwrap();
    let schema = &conclusion.input_schema;
    assert_eq!(
        schema["properties"]["checkpoint"]["$ref"],
        "#/$defs/ReviewCheckpoint"
    );
    assert_eq!(
        schema["$defs"]["ReviewCheckpoint"]["properties"]["checkpoint"]["type"],
        "string"
    );
    let interpretation = schema["properties"]["interpretation"]["anyOf"]
        .as_array()
        .unwrap();
    assert!(
        interpretation
            .iter()
            .any(|shape| shape["$ref"] == "#/$defs/Interpretation")
    );
    assert_eq!(
        schema["$defs"]["Interpretation"]["properties"]["follow_ups"]["items"]["type"],
        "string"
    );
    for section in ["summary", "to_be_implemented", "future_work"] {
        assert_eq!(schema["properties"][section]["type"], "string");
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!(section))
        );
    }
    assert_eq!(
        schema["properties"]["reviewed"]["items"]["$ref"],
        "#/$defs/CodeLocation"
    );
    assert_eq!(
        schema["properties"]["quiz_empty_reason"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(
        schema["properties"]["quiz"]["items"]["$ref"],
        "#/$defs/QuizItem"
    );
    let item = &schema["$defs"]["QuizItem"];
    for field in ["question", "answers", "correct", "why", "proof", "level"] {
        assert!(
            item["required"].as_array().unwrap().contains(&json!(field)),
            "{field}"
        );
    }
    assert!(schema["properties"].get("inspections").is_none());
    assert!(!tools.iter().any(|tool| tool.name == "get_coverage_gaps"));
}

#[test]
fn both_explore_tools_ask_each_not_relevant_mark_for_its_reason_and_test() {
    let tools = Handler::tools();
    for (name, marks) in [
        ("submit_question", "/properties/update/$ref"),
        ("submit_conclusion", ""),
    ] {
        let schema = serde_json::Value::Object(
            (*tools
                .iter()
                .find(|tool| tool.name == name)
                .unwrap()
                .input_schema)
                .clone(),
        );
        let submission = match marks {
            "" => &schema,
            pointer => {
                let reference = schema.pointer(pointer).unwrap().as_str().unwrap();
                schema.pointer(&reference[1..]).unwrap()
            }
        };
        assert_eq!(
            submission["properties"]["not_relevant"]["items"]["$ref"], "#/$defs/NotRelevantMark",
            "{name}"
        );
        let definitions = &schema["$defs"];
        let mark = &definitions["NotRelevantMark"];
        for field in ["path", "side", "reason"] {
            assert!(
                mark["required"].as_array().unwrap().contains(&json!(field)),
                "{name}: {field}"
            );
        }
        let reason = mark["properties"]["reason"].to_string();
        assert!(reason.contains("\"tested_mechanics\""), "{reason}");
        assert!(!reason.contains("null"), "{reason}");
        let test = &definitions["TestLocation"];
        for field in ["path", "lines"] {
            assert!(test["required"].as_array().unwrap().contains(&json!(field)));
        }
    }
}

#[test]
fn both_explore_tools_take_the_challengers_proposals_with_their_results() {
    let tools = Handler::tools();
    for (name, pointer) in [
        ("submit_question", Some("/properties/update/$ref")),
        ("submit_conclusion", None),
    ] {
        let schema = serde_json::Value::Object(
            (*tools
                .iter()
                .find(|tool| tool.name == name)
                .unwrap()
                .input_schema)
                .clone(),
        );
        let submission = match pointer {
            None => &schema,
            Some(pointer) => {
                let reference = schema.pointer(pointer).unwrap().as_str().unwrap();
                schema.pointer(&reference[1..]).unwrap()
            }
        };
        assert_eq!(
            submission["properties"]["challenger_proposals"]["items"]["$ref"],
            "#/$defs/ChallengerProposal",
            "{name}"
        );
        let required = submission["required"].as_array().unwrap();
        assert!(!required.contains(&json!("challenger_proposals")), "{name}");
        let proposal = &schema["$defs"]["ChallengerProposal"];
        let required = proposal["required"].as_array().unwrap();
        assert!(required.contains(&json!("title")) && required.contains(&json!("result")));
        assert!(!required.contains(&json!("reason")), "{name}");
        let results = schema["$defs"]["ProposalResult"].to_string();
        for result in ["asked", "merged", "retired", "kept"] {
            assert!(results.contains(&format!("\"{result}\"")), "{results}");
        }
    }
}

#[test]
fn the_question_tool_asks_for_a_thesis_for_the_change_and_for_each_part_of_its_design() {
    let tools = Handler::tools();
    let question = tools
        .iter()
        .find(|tool| tool.name == "submit_question")
        .unwrap();
    let definitions = &question.input_schema["$defs"];
    // A part is never a bare string, as it was in a round saved before theses.
    for (name, required) in [
        (
            "Design",
            &[
                "thesis",
                "overview",
                "data_flow",
                "algorithm",
                "alternatives",
            ][..],
        ),
        ("DesignPart", &["thesis", "body"][..]),
    ] {
        let mut fields: Vec<_> = definitions[name]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap())
            .collect();
        fields.sort_unstable();
        let mut expected = required.to_vec();
        expected.sort_unstable();
        assert_eq!(fields, expected, "{name}");
        assert_eq!(definitions[name]["additionalProperties"], false, "{name}");
    }
    assert_eq!(
        definitions["DesignPart"]["properties"]["thesis"]["type"],
        "string"
    );
}
