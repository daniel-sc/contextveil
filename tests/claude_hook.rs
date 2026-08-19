//! End-to-end tests for the Claude `PostToolUse` entry point.
//!
//! These drive the built binary over stdin and stdout, the way Claude Code
//! invokes it (`INT-003`), instead of calling adapter functions directly
//! (`architecture.md`, test architecture).
//!
//! Together with the Codex, Copilot, and OpenCode suites they are the `TST-004`
//! protocol fixtures: clean, intervened, unresolved, malformed-input,
//! diagnosed-malfunction, timeout-adjacent, and conflicting-installation states.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use contextveil::testing::{Canary, assert_canary_absent};
use serde_json::{Value, json};

/// An isolated home so a developer's real configuration is never read.
struct Home {
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "contextveil-e2e-{}-{}",
            std::process::id(),
            Canary::generate("HOME").token()
        ));
        std::fs::create_dir_all(root.join("contextveil")).expect("fixture directory");
        Self { root }
    }

    fn write_global_config(&self, contents: &str) {
        std::fs::write(self.root.join("contextveil").join("config.toml"), contents)
            .expect("write global config");
    }

    fn enroll_env(&self, name: &str) {
        self.write_global_config(&format!(
            "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"{name}\"\n"
        ));
    }

    /// Runs `contextveil hook claude` with a controlled environment.
    fn run_hook(&self, payload: &str, variables: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_contextveil"));
        command
            .args(["hook", "claude"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_CONFIG_HOME", &self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in variables {
            command.env(key, value);
        }

        let mut child = command.spawn().expect("the contextveil binary runs");
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(payload.as_bytes())
            .expect("write the hook payload");
        child.wait_with_output().expect("the hook finishes")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn post_tool_use(tool_response: Value) -> String {
    json!({
        "session_id": "0199c0de-0000-4000-8000-000000000000",
        "transcript_path": "/home/user/.claude/projects/demo/transcript.jsonl",
        "cwd": "/home/user/project",
        "permission_mode": "default",
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "printenv GITHUB_TOKEN"},
        "tool_response": tool_response,
    })
    .to_string()
}

#[test]
fn a_matched_value_is_replaced_before_the_model_visible_boundary() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let payload = post_tool_use(json!({
        "stdout": format!("{}\n", canary.value()),
        "stderr": "",
        "interrupted": false,
        "isImage": false,
    }));
    let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("hook stdout", &output.stdout, &canary);
    assert_canary_absent("hook stderr", &output.stderr, &canary);
    assert!(output.stderr.is_empty());

    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON on stdout");
    let updated = &response["hookSpecificOutput"]["updatedToolOutput"];
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        json!("PostToolUse")
    );
    assert_eq!(updated["stdout"], json!("<SECRET:GITHUB_TOKEN>\n"));
    assert_eq!(updated["stderr"], json!(""));
    assert_eq!(updated["interrupted"], json!(false));
    assert_eq!(updated["isImage"], json!(false));
    assert!(response["systemMessage"].is_string());
}

#[test]
fn a_clean_event_produces_no_output_at_all() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let payload = post_tool_use(json!({"stdout": "nothing sensitive\n", "stderr": ""}));
    let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty(), "clean events must be silent");
    assert!(output.stderr.is_empty());
}

#[test]
fn an_unresolved_source_is_silent_and_does_not_fail_the_event() {
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let payload = post_tool_use(json!({"stdout": "tool output\n"}));
    let output = home.run_hook(&payload, &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn invalid_input_is_diagnosed_without_echoing_the_payload() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    for payload in [
        String::new(),
        String::from("{ this is not json"),
        String::from("[1, 2, 3]"),
        json!({"hook_event_name": "SessionStart"}).to_string(),
        format!("{{\"leak\": \"{}\"}}", canary.value()),
    ] {
        let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);

        // `CLI-007`: a diagnosed failure still exits zero with valid protocol
        // output so the host can present the warning.
        assert_eq!(output.status.code(), Some(0));
        assert_canary_absent("hook stdout", &output.stdout, &canary);
        assert_canary_absent("hook stderr", &output.stderr, &canary);

        let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON on stdout");
        assert!(response["systemMessage"].is_string());
        assert!(
            response.get("hookSpecificOutput").is_none(),
            "a malfunction must not mutate host content"
        );
    }
}

#[test]
fn an_invalid_global_config_disables_redaction_and_warns() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.write_global_config("version = 1\n\n[[secret]]\nsource = \"unknown\"\n");

    let payload = post_tool_use(json!({"stdout": canary.value()}));
    let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);

    assert_eq!(output.status.code(), Some(0));
    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON on stdout");
    assert!(response.get("hookSpecificOutput").is_none());
    assert!(
        response["systemMessage"]
            .as_str()
            .expect("a warning")
            .contains("doctor")
    );
    assert_canary_absent("hook stdout", &output.stdout, &canary);
}

/// Asserts that two values have identical structure, keys, and value types.
///
/// `CLA-002` requires the returned `updatedToolOutput` to keep the host's exact
/// shape, because Claude validates it against the tool's own result schema and
/// reverts to the original result on mismatch (`LIM-013`). Structure is compared
/// rather than pinned to a captured schema, which keeps the assertion meaningful
/// across host versions (`LIM-018`).
fn assert_same_shape(original: &Value, updated: &Value, path: &str) {
    match (original, updated) {
        (Value::Object(left), Value::Object(right)) => {
            let left_keys: Vec<&String> = left.keys().collect();
            let right_keys: Vec<&String> = right.keys().collect();
            assert_eq!(left_keys, right_keys, "keys changed at {path}");
            for (key, value) in left {
                assert_same_shape(value, &right[key], &format!("{path}.{key}"));
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            assert_eq!(left.len(), right.len(), "array length changed at {path}");
            for (index, value) in left.iter().enumerate() {
                assert_same_shape(value, &right[index], &format!("{path}[{index}]"));
            }
        }
        (Value::String(_), Value::String(_)) => {}
        (left, right) => assert_eq!(left, right, "non-string value changed at {path}"),
    }
}

/// Representative successful `tool_response` shapes.
///
/// Each is a documented Claude result shape that accepts replacement, plus the
/// two MCP shapes. `{}` stands in for the canary.
fn tool_response_shapes(canary: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "Bash",
            json!({
                "stdout": format!("GITHUB_TOKEN={canary}\n"),
                "stderr": "",
                "interrupted": false,
                "isImage": false,
            }),
        ),
        (
            "Read",
            json!({
                "type": "text",
                "file": {
                    "filePath": "/project/.env",
                    "content": format!("GITHUB_TOKEN={canary}\n"),
                    "numLines": 1,
                    "startLine": 1,
                    "totalLines": 1,
                },
            }),
        ),
        (
            "Edit",
            json!({
                "filePath": "/project/.env",
                "oldString": format!("TOKEN={canary}"),
                "newString": "TOKEN=redacted-by-user",
                "originalFile": format!("TOKEN={canary}\n"),
                "userModified": false,
                "structuredPatch": [{
                    "oldStart": 1,
                    "oldLines": 1,
                    "newStart": 1,
                    "newLines": 1,
                    "lines": [format!("-TOKEN={canary}"), "+TOKEN=redacted-by-user"],
                }],
            }),
        ),
        (
            "Write",
            json!({
                "type": "update",
                "filePath": "/project/.env",
                "content": format!("TOKEN={canary}\n"),
                "structuredPatch": [],
            }),
        ),
        (
            "Grep",
            json!({
                "mode": "content",
                "numFiles": 1,
                "filenames": ["/project/.env"],
                "content": format!("/project/.env:1:TOKEN={canary}"),
                "numLines": 1,
            }),
        ),
        (
            "WebFetch",
            json!({
                "bytes": 512,
                "code": 200,
                "codeText": "OK",
                "result": format!("the response mentioned {canary}"),
                "durationMs": 42,
                "url": "https://example.test/",
            }),
        ),
        (
            "mcp__server__tool",
            json!([{ "type": "text", "text": format!("token: {canary}") }]),
        ),
        (
            "mcp__server__structured",
            json!({
                "content": [
                    {"type": "text", "text": format!("token: {canary}")},
                    {"type": "image", "data": "aGVsbG8=", "mimeType": "image/png"},
                ],
                "isError": false,
            }),
        ),
    ]
}

#[test]
fn every_supported_result_shape_is_redacted_without_changing_its_shape() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    for (tool, original) in tool_response_shapes(canary.value()) {
        let payload = json!({
            "session_id": "0199c0de-0000-4000-8000-000000000000",
            "cwd": "/home/user/project",
            "permission_mode": "default",
            "hook_event_name": "PostToolUse",
            "tool_name": tool,
            "tool_input": {},
            "tool_response": original.clone(),
        })
        .to_string();

        let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);
        assert_eq!(output.status.code(), Some(0), "tool {tool}");
        assert_canary_absent(&format!("{tool} stdout"), &output.stdout, &canary);
        assert_canary_absent(&format!("{tool} stderr"), &output.stderr, &canary);

        let response: Value =
            serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("tool {tool}"));
        let updated = &response["hookSpecificOutput"]["updatedToolOutput"];
        assert!(!updated.is_null(), "tool {tool} produced no replacement");
        assert_same_shape(&original, updated, tool);
        assert_eq!(
            response["hookSpecificOutput"]["hookEventName"],
            json!("PostToolUse")
        );
        assert!(response["systemMessage"].is_string(), "tool {tool}");
        assert!(response.get("additionalContext").is_none());
    }
}

#[test]
fn a_failed_tool_result_event_is_not_claimed_as_covered() {
    // `CLA-004`: failed tool results are outside V1 coverage. Claude reports them
    // through a different event, which this adapter treats as an unknown event:
    // it warns and changes nothing (`RUN-006`).
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let payload = json!({
        "session_id": "0199c0de-0000-4000-8000-000000000000",
        "hook_event_name": "PostToolUseFailure",
        "tool_name": "Bash",
        "tool_input": {"command": "printenv GITHUB_TOKEN"},
        "tool_response": {"stdout": canary.value(), "is_interrupt": false},
    })
    .to_string();

    let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);
    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("failure-event stdout", &output.stdout, &canary);
    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(response.get("hookSpecificOutput").is_none());
    assert!(response["systemMessage"].is_string());
}

#[test]
fn uncovered_fields_and_non_string_content_are_preserved_exactly() {
    // `LIM-003`: object keys, numbers, and binary content stay unchanged, and a
    // value split across two fields is intentionally not matched (`RED-002`).
    let canary = Canary::generate("SPLIT_TOKEN");
    let home = Home::new();
    home.enroll_env("SPLIT_TOKEN");

    let half = canary.value().len() / 2;
    let original = json!({
        "first": canary.value()[..half].to_string(),
        "second": canary.value()[half..].to_string(),
        canary.value(): "a secret used as a key stays visible",
        "count": 7,
        "binary": [1, 2, 3],
    });
    let payload = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_response": original.clone(),
    })
    .to_string();

    let output = home.run_hook(&payload, &[("SPLIT_TOKEN", canary.value())]);
    assert_eq!(output.status.code(), Some(0));
    // Nothing matched, so the hook stays silent and the host keeps the original.
    assert!(output.stdout.is_empty());
}

#[test]
fn a_large_payload_is_answered_well_inside_the_host_timeout() {
    // `RUN-004`: the host allows five seconds. `LIM-010`: there is no product
    // size cap, so this measures rather than promises.
    let canary = Canary::generate("BULK_TOKEN");
    let home = Home::new();
    home.enroll_env("BULK_TOKEN");

    let mut bulk = String::with_capacity(2 * 1024 * 1024);
    while bulk.len() < 2 * 1024 * 1024 {
        bulk.push_str("ordinary tool output line with nothing sensitive in it\n");
    }
    bulk.push_str(canary.value());
    let payload = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_response": {"stdout": bulk, "stderr": "", "interrupted": false},
    })
    .to_string();

    let started = std::time::Instant::now();
    let output = home.run_hook(&payload, &[("BULK_TOKEN", canary.value())]);
    let elapsed = started.elapsed();

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("bulk stdout", &output.stdout, &canary);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "a 2 MiB payload took {elapsed:?}, which the host would time out"
    );
}

#[test]
fn non_utf8_stdin_is_diagnosed_safely() {
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let mut command = Command::new(env!("CARGO_BIN_EXE_contextveil"));
    command
        .args(["hook", "claude"])
        .env_clear()
        .env("XDG_CONFIG_HOME", &home.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("the contextveil binary runs");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(&[0xff, 0xfe, 0x00, 0x01])
        .expect("write bytes");
    let output = child.wait_with_output().expect("the hook finishes");

    assert_eq!(output.status.code(), Some(0));
    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON on stdout");
    assert!(response["systemMessage"].is_string());
    assert!(response.get("hookSpecificOutput").is_none());
}
