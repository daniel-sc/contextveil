//! Consolidated real-binary assurance for process-hook model boundaries.

mod common;

use common::ProcessFixture;
use contextveil::testing::{Canary, assert_canary_absent, assert_canary_present};
use serde_json::{Value, json};

struct Boundary {
    name: &'static str,
    arguments: &'static [&'static str],
    payload: fn(&str) -> String,
    model_visible: fn(&[u8]) -> Vec<u8>,
}

fn claude_payload(canary: &str) -> String {
    json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_response": {"stdout": format!("token={canary}")},
    })
    .to_string()
}

fn codex_payload(canary: &str) -> String {
    json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "shell",
        "tool_response": {"output": format!("token={canary}"), "exit_code": 0},
    })
    .to_string()
}

fn copilot_prompt_payload(canary: &str) -> String {
    json!({
        "cwd": "/home/user/project",
        "prompt": format!("use {canary}"),
        "transformedPrompt": format!("use {canary}"),
    })
    .to_string()
}

fn copilot_tool_payload(canary: &str) -> String {
    json!({
        "cwd": "/home/user/project",
        "toolName": "shell",
        "toolResult": {
            "resultType": "success",
            "textResultForLlm": format!("token={canary}"),
        },
    })
    .to_string()
}

fn json_stdout(stdout: &[u8]) -> Value {
    serde_json::from_slice(stdout).expect("one JSON response on stdout")
}

fn claude_model_visible(stdout: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&json_stdout(stdout)["hookSpecificOutput"]["updatedToolOutput"])
        .expect("serialize updated tool output")
}

fn codex_model_visible(stdout: &[u8]) -> Vec<u8> {
    json_stdout(stdout)["reason"]
        .as_str()
        .expect("Codex model-facing reason")
        .as_bytes()
        .to_vec()
}

fn copilot_final(stdout: &[u8]) -> Value {
    String::from_utf8(stdout.to_vec())
        .expect("UTF-8 stdout")
        .lines()
        .map(|line| serde_json::from_str(line).expect("one JSON object per line"))
        .find(|value: &Value| value.get("type").and_then(Value::as_str) != Some("progress"))
        .expect("final Copilot mutation")
}

fn copilot_prompt_model_visible(stdout: &[u8]) -> Vec<u8> {
    copilot_final(stdout)["modifiedTransformedPrompt"]
        .as_str()
        .expect("modified transformed prompt")
        .as_bytes()
        .to_vec()
}

fn copilot_tool_model_visible(stdout: &[u8]) -> Vec<u8> {
    copilot_final(stdout)["modifiedResult"]["textResultForLlm"]
        .as_str()
        .expect("modified text result")
        .as_bytes()
        .to_vec()
}

#[test]
fn enrolled_values_are_absent_at_every_process_boundary_after_intervention() {
    let cases = [
        Boundary {
            name: "Claude tool result",
            arguments: &["hook", "claude"],
            payload: claude_payload,
            model_visible: claude_model_visible,
        },
        Boundary {
            name: "Codex tool result",
            arguments: &["hook", "codex"],
            payload: codex_payload,
            model_visible: codex_model_visible,
        },
        Boundary {
            name: "Copilot prompt",
            arguments: &["hook", "copilot", "prompt"],
            payload: copilot_prompt_payload,
            model_visible: copilot_prompt_model_visible,
        },
        Boundary {
            name: "Copilot tool result",
            arguments: &["hook", "copilot", "tool"],
            payload: copilot_tool_payload,
            model_visible: copilot_tool_model_visible,
        },
    ];

    for case in cases {
        let canary = Canary::generate("BOUNDARY_TOKEN");
        let fixture = ProcessFixture::new(canary.label());
        let payload = (case.payload)(canary.value());
        assert_canary_present(case.name, payload.as_bytes(), &canary);

        let output = fixture.run(
            case.arguments,
            payload.as_bytes(),
            &[(canary.label(), canary.value())],
        );
        assert_eq!(output.status.code(), Some(0), "{}", case.name);
        assert_canary_absent(&format!("{} stdout", case.name), &output.stdout, &canary);
        assert_canary_absent(&format!("{} stderr", case.name), &output.stderr, &canary);
        assert!(
            !output.stdout.is_empty(),
            "{} produced no intervention",
            case.name
        );

        let model_visible = (case.model_visible)(&output.stdout);
        assert_canary_absent(case.name, &model_visible, &canary);
        let placeholder = format!("<SECRET:{}>", canary.label());
        assert!(
            model_visible
                .windows(placeholder.len())
                .any(|window| window == placeholder.as_bytes()),
            "{} did not put a placeholder in model-visible content",
            case.name
        );
    }
}

#[test]
fn non_utf8_stdin_is_diagnosed_without_raw_input_reaching_output() {
    let fixture = ProcessFixture::new("BOUNDARY_TOKEN");
    let input = [0xff, 0xfe, 0x00, 0x01];
    let output = fixture.run(&["hook", "claude"], &input, &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        !output.stdout.is_empty(),
        "malformed stdin was not diagnosed"
    );
    assert!(output.stderr.is_empty());
    assert!(
        !output
            .stdout
            .windows(input.len())
            .any(|window| window == input)
    );
    let response = json_stdout(&output.stdout);
    assert!(response["systemMessage"].is_string());
    assert!(response.get("hookSpecificOutput").is_none());
}
