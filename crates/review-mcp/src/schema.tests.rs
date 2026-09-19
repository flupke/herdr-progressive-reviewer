use super::Handler;
use serde_json::json;

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
        "Interpretation",
        "Assessments",
        "Consequence",
        "EvidenceRef",
        "CodeLocation",
        "ReviewCheckpoint",
        "GuideLineRange",
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
    assert!(update["properties"].get("conclusion").is_none());
    assert_eq!(update["additionalProperties"], false);
    let question = &update["properties"]["next"]["properties"];
    assert_eq!(question["alternatives"]["minItems"], 2);
    assert_eq!(question["alternatives"]["maxItems"], 5);
    assert_eq!(question["evidence"]["items"]["$ref"], "#/$defs/EvidenceRef");
    assert_eq!(
        definitions["EvidenceRef"]["properties"]["decision_relevance"]["type"],
        "string"
    );
    assert_eq!(
        definitions["TopicStatus"]["enum"],
        json!(["open", "accepted", "needs_follow_up", "deferred"])
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
}
