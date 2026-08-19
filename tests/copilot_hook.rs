//! End-to-end protocol fixtures for the GitHub Copilot CLI entry points.
//!
//! Copilot is experimental (`SUP-002`). Two covered events are exercised through
//! the built binary: the transformed prompt and a successful tool result
//! (`COP-002`). Failed results, which arrive on a different host event, are
//! covered by a negative fixture (`COP-004`).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use contextveil::testing::{Canary, assert_canary_absent};
use serde_json::{Value, json};

struct Home {
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "contextveil-copilot-e2e-{}-{}",
            std::process::id(),
            Canary::generate("HOME").token()
        ));
        std::fs::create_dir_all(root.join("contextveil")).expect("config directory");
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

    fn run_hook(&self, event: &str, payload: &str, variables: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_contextveil"));
        command
            .args(["hook", "copilot", event])
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
        // A usage error exits before the payload is read, so the pipe may already
        // be closed here. That is the child's answer, which the assertions below
        // check, rather than a harness failure.
        match child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(payload.as_bytes())
        {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(error) => panic!("write the hook payload: {error}"),
        }
        child.wait_with_output().expect("the hook finishes")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Splits progress lines from the final object, the way Copilot does.
fn split(output: &Output) -> (Vec<Value>, Value) {
    let text = String::from_utf8(output.stdout.clone()).expect("UTF-8 stdout");
    let mut progress = Vec::new();
    let mut final_object = Value::Null;
    for line in text.lines() {
        let value: Value = serde_json::from_str(line).expect("each stdout line is one JSON object");
        if value.get("type").and_then(Value::as_str) == Some("progress") {
            progress.push(value);
        } else {
            final_object = value;
        }
    }
    (progress, final_object)
}

fn prompt_payload(text: &str) -> String {
    json!({
        "sessionId": "s1",
        "timestamp": 1_760_000_000_000_u64,
        "cwd": "/home/user/project",
        "prompt": text,
        "transformedPrompt": text,
    })
    .to_string()
}

fn tool_payload(result_type: &str, text: &str) -> String {
    json!({
        "sessionId": "s1",
        "timestamp": 1_760_000_000_000_u64,
        "cwd": "/home/user/project",
        "toolName": "shell",
        "toolArgs": {"command": "printenv GITHUB_TOKEN"},
        "toolResult": {"resultType": result_type, "textResultForLlm": text},
    })
    .to_string()
}

#[test]
fn a_transformed_prompt_is_redacted_before_the_model_sees_it() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let payload = prompt_payload(&format!("please use {}", canary.value()));
    let output = home.run_hook("prompt", &payload, &[("GITHUB_TOKEN", canary.value())]);

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("copilot stdout", &output.stdout, &canary);
    assert_canary_absent("copilot stderr", &output.stderr, &canary);

    let (progress, final_object) = split(&output);
    // `COP-003`: exactly one safe progress summary.
    assert_eq!(progress.len(), 1);
    assert_eq!(
        final_object["modifiedTransformedPrompt"],
        json!("please use <SECRET:GITHUB_TOKEN>")
    );
}

#[test]
fn a_successful_tool_result_is_redacted_and_keeps_its_shape() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let payload = tool_payload("success", &format!("GITHUB_TOKEN={}", canary.value()));
    let output = home.run_hook("tool", &payload, &[("GITHUB_TOKEN", canary.value())]);

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("copilot stdout", &output.stdout, &canary);
    let (_, final_object) = split(&output);
    let result = &final_object["modifiedResult"];
    assert_eq!(result["resultType"], json!("success"));
    assert_eq!(
        result["textResultForLlm"],
        json!("GITHUB_TOKEN=<SECRET:GITHUB_TOKEN>")
    );
}

#[test]
fn a_failed_tool_result_is_not_touched() {
    // `COP-004`: failed tool errors are not covered.
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let output = home.run_hook(
        "tool",
        &tool_payload("failure", canary.value()),
        &[("GITHUB_TOKEN", canary.value())],
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty(), "an uncovered path stays silent");
}

#[test]
fn clean_events_produce_no_output() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    for (event, payload) in [
        ("prompt", prompt_payload("nothing sensitive")),
        ("tool", tool_payload("success", "nothing sensitive")),
    ] {
        let output = home.run_hook(event, &payload, &[("GITHUB_TOKEN", canary.value())]);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.is_empty(), "{event} was not silent");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn an_unresolved_source_is_silent() {
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");
    let output = home.run_hook("tool", &tool_payload("success", "text"), &[]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn a_malfunction_warns_through_stderr_and_mutates_nothing() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.write_global_config("version = 1\n\n[[secret]]\nsource = \"unknown\"\n");

    let output = home.run_hook(
        "tool",
        &tool_payload("success", canary.value()),
        &[("GITHUB_TOKEN", canary.value())],
    );
    assert!(output.stdout.is_empty(), "no mutation on malfunction");
    assert_canary_absent("copilot stderr", &output.stderr, &canary);
    let message = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(message.contains("doctor"));
    // Exit 2 is how Copilot surfaces a hook warning while continuing the run.
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn malformed_input_is_diagnosed_without_echoing_the_payload() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    for payload in [
        String::new(),
        String::from("{ not json"),
        String::from("[]"),
        format!("{{\"leak\": \"{}\"}}", canary.value()),
    ] {
        for event in ["prompt", "tool"] {
            let output = home.run_hook(event, &payload, &[("GITHUB_TOKEN", canary.value())]);
            assert!(output.stdout.is_empty());
            assert_canary_absent("copilot stderr", &output.stderr, &canary);
            assert!(!output.stderr.is_empty(), "{event} produced no warning");
        }
    }
}

#[test]
fn an_unknown_hook_event_is_a_usage_error() {
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");
    let output = home.run_hook("unsupported", &prompt_payload("text"), &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn a_project_registry_is_selected_from_the_event_cwd() {
    // `CFG-005`: Copilot may fall back to event `cwd`.
    let canary = Canary::generate("PROJECT_TOKEN");
    let home = Home::new();
    home.write_global_config("version = 1\n");
    let project = home.root.join("project");
    std::fs::create_dir_all(&project).expect("project directory");
    std::fs::write(
        project.join(".contextveil.toml"),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"PROJECT_TOKEN\"\n",
    )
    .expect("write project config");

    let payload = json!({
        "sessionId": "s1",
        "timestamp": 0,
        "cwd": project.to_string_lossy(),
        "toolName": "shell",
        "toolResult": {"resultType": "success", "textResultForLlm": canary.value()},
    })
    .to_string();
    let output = home.run_hook("tool", &payload, &[("PROJECT_TOKEN", canary.value())]);
    assert_canary_absent("copilot stdout", &output.stdout, &canary);
    let (_, final_object) = split(&output);
    assert_eq!(
        final_object["modifiedResult"]["textResultForLlm"],
        json!("<SECRET:PROJECT_TOKEN>")
    );
}

#[test]
fn a_large_result_is_answered_inside_the_host_timeout() {
    // `RUN-004`: the installed hook has a 5-second timeout, and a timeout is
    // fail-open on this host (`LIM-012`).
    let canary = Canary::generate("BULK_TOKEN");
    let home = Home::new();
    home.enroll_env("BULK_TOKEN");

    let mut bulk = String::with_capacity(1024 * 1024);
    while bulk.len() < 1024 * 1024 {
        bulk.push_str("ordinary tool output with nothing sensitive in it\n");
    }
    bulk.push_str(canary.value());

    let started = std::time::Instant::now();
    let output = home.run_hook(
        "tool",
        &tool_payload("success", &bulk),
        &[("BULK_TOKEN", canary.value())],
    );
    let elapsed = started.elapsed();

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("copilot stdout", &output.stdout, &canary);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "a 1 MiB result took {elapsed:?}, which the host would time out"
    );
}
