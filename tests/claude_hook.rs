//! End-to-end tests for the Claude `PostToolUse` entry point.
//!
//! These drive the built binary over stdin and stdout, the way Claude Code
//! invokes it (`INT-003`), instead of calling adapter functions directly
//! (`architecture.md`, test architecture).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use secretsieve::testing::{Canary, assert_canary_absent};
use serde_json::{Value, json};

/// An isolated home so a developer's real configuration is never read.
struct Home {
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "secretsieve-e2e-{}-{}",
            std::process::id(),
            Canary::generate("HOME").token()
        ));
        std::fs::create_dir_all(root.join("secretsieve")).expect("fixture directory");
        Self { root }
    }

    fn write_global_config(&self, contents: &str) {
        std::fs::write(self.root.join("secretsieve").join("config.toml"), contents)
            .expect("write global config");
    }

    fn enroll_env(&self, name: &str) {
        self.write_global_config(&format!(
            "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"{name}\"\n"
        ));
    }

    /// Runs `secretsieve hook claude` with a controlled environment.
    fn run_hook(&self, payload: &str, variables: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_secretsieve"));
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

        let mut child = command.spawn().expect("the secretsieve binary runs");
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

#[test]
fn non_utf8_stdin_is_diagnosed_safely() {
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let mut command = Command::new(env!("CARGO_BIN_EXE_secretsieve"));
    command
        .args(["hook", "claude"])
        .env_clear()
        .env("XDG_CONFIG_HOME", &home.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("the secretsieve binary runs");
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
