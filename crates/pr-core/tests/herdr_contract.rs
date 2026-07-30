use pr_core::herdr::PluginContext;

#[test]
fn plugin_context_accepts_unknown_herdr_fields() {
    let context: PluginContext = serde_json::from_str(
        r#"{
            "workspace_id": "w1",
            "tab_id": "w1:t1",
            "focused_pane_id": "w1:p1",
            "focused_pane_cwd": "/tmp/repository",
            "future_field": {"is_allowed": true}
        }"#,
    )
    .unwrap();

    assert_eq!(
        context.workspace_id.as_ref().map(|id| id.0.as_str()),
        Some("w1")
    );
    assert_eq!(
        context.focused_pane_id.as_ref().map(|id| id.0.as_str()),
        Some("w1:p1")
    );
}
