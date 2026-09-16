//! Consolidated real-binary assurance for process-hook model boundaries.

mod common;

use common::ProcessFixture;
use contextveil::testing::{Canary, assert_canary_absent, assert_canary_present};
use serde_json::{Value, json};

struct Boundary {
    name: &'static str,
    arguments: &'static [&'static str],
    payload: fn(&str, &str) -> String,
    model_visible: fn(&[u8]) -> Vec<u8>,
}

fn boundaries() -> [Boundary; 4] {
    [
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
    ]
}

fn claude_payload(canary: &str, cwd: &str) -> String {
    json!({
        "hook_event_name": "PostToolUse",
        "cwd": cwd,
        "tool_name": "Bash",
        "tool_response": {"stdout": format!("token={canary}")},
    })
    .to_string()
}

fn codex_payload(canary: &str, cwd: &str) -> String {
    json!({
        "hook_event_name": "PostToolUse",
        "cwd": cwd,
        "tool_name": "shell",
        "tool_response": {"output": format!("token={canary}"), "exit_code": 0},
    })
    .to_string()
}

fn copilot_prompt_payload(canary: &str, cwd: &str) -> String {
    json!({
        "cwd": cwd,
        "prompt": format!("use {canary}"),
        "transformedPrompt": format!("use {canary}"),
    })
    .to_string()
}

fn copilot_tool_payload(canary: &str, cwd: &str) -> String {
    json!({
        "cwd": cwd,
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
    for case in boundaries() {
        let canary = Canary::generate("BOUNDARY_TOKEN");
        let fixture = ProcessFixture::new(Some(canary.label()));
        let project_dir = fixture.project_dir();
        let payload = (case.payload)(canary.value(), &project_dir.to_string_lossy());
        let resolved = format!("  {}  ", canary.value());
        assert_canary_present(case.name, payload.as_bytes(), &canary);

        let output = fixture.run(
            case.arguments,
            payload.as_bytes(),
            &[(canary.label(), &resolved)],
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
fn exact_ini_entries_are_absent_at_every_process_boundary() {
    for case in boundaries() {
        let canary = Canary::generate("TOKEN");
        let other = Canary::generate("UNSELECTED");
        let fixture = ProcessFixture::new(Some(canary.label()));
        fixture.write_project_file(
            "credentials.ini",
            &format!(
                "[Production]\nTOKEN=  {}  \n[Other]\nTOKEN={}\n",
                canary.value(),
                other.value()
            ),
        );
        fixture.write_project_config(
            "version = 1\n\n[[secret]]\nsource = \"ini\"\nfile = \"credentials.ini\"\nsection = \"Production\"\nkey = \"TOKEN\"\n",
        );

        let payload_value = format!("{} {}", canary.value(), other.value());
        let project_dir = fixture.project_dir();
        let payload = (case.payload)(&payload_value, &project_dir.to_string_lossy());
        let output = fixture.run(case.arguments, payload.as_bytes(), &[]);
        assert_eq!(output.status.code(), Some(0), "{}", case.name);
        assert_canary_absent(&format!("{} stdout", case.name), &output.stdout, &canary);
        assert_canary_absent(&format!("{} stderr", case.name), &output.stderr, &canary);
        let model_visible = (case.model_visible)(&output.stdout);
        assert_canary_absent(case.name, &model_visible, &canary);
        assert!(
            model_visible
                .windows(b"<SECRET:TOKEN>".len())
                .any(|window| window == b"<SECRET:TOKEN>"),
            "{} did not put the exact INI placeholder in model-visible content",
            case.name
        );
        assert!(
            model_visible
                .windows(other.value().len())
                .any(|window| window == other.value().as_bytes()),
            "{} did not preserve the unselected INI section",
            case.name
        );
    }
}

#[test]
fn ini_section_wildcards_are_absent_at_every_process_boundary_and_follow_new_sections() {
    for case in boundaries() {
        let canary = Canary::generate("TOKEN");
        let future = Canary::generate("FUTURE_TOKEN");
        let fixture = ProcessFixture::new(Some("UNUSED_ENV"));
        let path = fixture.write_project_file(
            "credentials.ini",
            &format!(
                "TOKEN={}\n[Production]\nTOKEN={}\n",
                canary.value(),
                canary.value()
            ),
        );
        fixture.write_project_config(
            "version = 1\n\n[[secret]]\nsource = \"ini\"\nfile = \"credentials.ini\"\nall_sections = true\nkey = \"TOKEN\"\n",
        );

        let project_dir = fixture.project_dir();
        let payload = (case.payload)(canary.value(), &project_dir.to_string_lossy());
        let output = fixture.run(case.arguments, payload.as_bytes(), &[]);
        assert_eq!(output.status.code(), Some(0), "{} current", case.name);
        assert!(
            !output.stdout.is_empty(),
            "{} current produced no protocol response (stderr={:?})",
            case.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_canary_absent(
            &format!("{} current stdout", case.name),
            &output.stdout,
            &canary,
        );
        assert_canary_absent(
            &format!("{} current stderr", case.name),
            &output.stderr,
            &canary,
        );
        let model_visible = (case.model_visible)(&output.stdout);
        assert_canary_absent(&format!("{} current", case.name), &model_visible, &canary);
        assert!(
            model_visible
                .windows(b"<SECRET:TOKEN>".len())
                .any(|window| window == b"<SECRET:TOKEN>"),
            "{} did not put the wildcard placeholder in model-visible content",
            case.name
        );

        let mut contents = std::fs::read_to_string(&path).expect("read INI source");
        contents.push_str(&format!("[Future]\nTOKEN={}\n", future.value()));
        std::fs::write(path, contents).expect("rotate INI source");
        let project_dir = fixture.project_dir();
        let payload = (case.payload)(future.value(), &project_dir.to_string_lossy());
        let output = fixture.run(case.arguments, payload.as_bytes(), &[]);
        assert_eq!(output.status.code(), Some(0), "{} future", case.name);
        assert!(
            !output.stdout.is_empty(),
            "{} future produced no protocol response (stderr={:?})",
            case.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_canary_absent(
            &format!("{} future stdout", case.name),
            &output.stdout,
            &future,
        );
        assert_canary_absent(
            &format!("{} future stderr", case.name),
            &output.stderr,
            &future,
        );
        let model_visible = (case.model_visible)(&output.stdout);
        assert_canary_absent(&format!("{} future", case.name), &model_visible, &future);
        assert!(
            model_visible
                .windows(b"<SECRET:TOKEN>".len())
                .any(|window| window == b"<SECRET:TOKEN>"),
            "{} did not protect a future wildcard section",
            case.name
        );
    }
}

#[test]
fn non_utf8_stdin_is_diagnosed_without_raw_input_reaching_output() {
    let fixture = ProcessFixture::new(Some("BOUNDARY_TOKEN"));
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
