use super::*;

fn arguments(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn a_fork_keeps_the_options_that_shape_the_agent_and_drops_those_that_pick_a_session() {
    let (kept, model) = kept(&arguments(
        "--model claude-sonnet-5-5 --resume abc --strict-mcp-config --mcp-config a.json b.json \
         --permission-mode=dontAsk -c --session-id s1 --verbose --allowedTools Read Grep \
         --add-dir /tmp",
    ))
    .unwrap();

    assert_eq!(
        kept,
        arguments(
            "--strict-mcp-config --mcp-config a.json b.json --permission-mode=dontAsk \
             --allowedTools Read Grep --add-dir /tmp"
        )
    );
    assert_eq!(model.as_deref(), Some("claude-sonnet-5-5"));
}

#[test]
fn a_prompt_on_the_command_line_does_not_reach_a_fork() {
    let (kept, model) = kept(&arguments(
        "fix the bug --model=opus --resume --effort high",
    ))
    .unwrap();

    assert_eq!(kept, arguments("--effort high"));
    assert_eq!(model.as_deref(), Some("opus"));
}

#[test]
fn an_agent_with_an_unknown_option_or_its_own_settings_is_not_forked() {
    assert!(kept(&arguments("--some-new-option value")).is_err());
    assert!(kept(&arguments("--settings custom.json")).is_err());
}
