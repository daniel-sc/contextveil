//! End-to-end protocol fixtures for the Codex CLI `PostToolUse` entry point.
//!
//! Codex is experimental (`SUP-002`). It offers no shape-preserving replacement,
//! so an intervention blocks the original result and supplies a sanitized
//! textual rendering (`COD-002`, `LIM-014`). These tests drive the built binary
//! over stdin and stdout, the way Codex invokes it (`INT-003`).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use secretsieve::testing::{Canary, assert_canary_absent};
use serde_json::{Value, json};

struct Home {
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "secretsieve-codex-e2e-{}-{}",
            std::process::id(),
            Canary::generate("HOME").token()
        ));
        std::fs::create_dir_all(root.join("secretsieve")).expect("config directory");
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

    fn run_hook(&self, payload: &str, variables: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_secretsieve"));
        command
            .args(["hook", "codex"])
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
        "session_id": "0199c0de",
        "turn_id": "turn-1",
        "transcript_path": "/home/user/.codex/sessions/x.jsonl",
        "cwd": "/home/user/project",
        "hook_event_name": "PostToolUse",
        "model": "gpt-5",
        "permission_mode": "default",
        "tool_name": "shell",
        "tool_input": {"command": ["printenv", "GITHUB_TOKEN"]},
        "tool_response": tool_response,
        "tool_use_id": "call-1",
    })
    .to_string()
}

#[test]
fn a_matched_value_blocks_the_original_result() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let payload = post_tool_use(json!({
        "output": format!("GITHUB_TOKEN={}\n", canary.value()),
        "exit_code": 0,
    }));
    let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("codex stdout", &output.stdout, &canary);
    assert_canary_absent("codex stderr", &output.stderr, &canary);

    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON on stdout");
    assert_eq!(response["decision"], json!("block"));
    let reason = response["reason"].as_str().expect("a reason");
    assert!(reason.contains("<SECRET:GITHUB_TOKEN>"));
    // `COD-003`: the semantic degradation is disclosed to the model.
    assert!(reason.contains("did not fail"));
    assert!(response["systemMessage"].is_string());
}

#[test]
fn a_clean_event_produces_no_output_at_all() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let output = home.run_hook(
        &post_tool_use(json!({"output": "nothing sensitive\n", "exit_code": 0})),
        &[("GITHUB_TOKEN", canary.value())],
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty(), "clean events must be silent");
    assert!(output.stderr.is_empty());
}

#[test]
fn a_non_zero_exit_result_is_covered() {
    // Verified against openai/codex: a shell command that exits non-zero is
    // still a successful tool call, so the event fires and is covered.
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    let payload = post_tool_use(json!({
        "output": format!("authentication failed for {}\n", canary.value()),
        "exit_code": 1,
    }));
    let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);
    assert_canary_absent("codex stdout", &output.stdout, &canary);
    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(response["decision"], json!("block"));
}

#[test]
fn a_structured_mcp_result_is_rendered_as_sanitized_text() {
    // `COD-004`: MCP results are not shape-preserving; the rendering is textual.
    let canary = Canary::generate("MCP_TOKEN");
    let home = Home::new();
    home.enroll_env("MCP_TOKEN");

    let payload = post_tool_use(json!({
        "content": [{"type": "text", "text": format!("token {}", canary.value())}],
        "isError": false,
    }));
    let output = home.run_hook(&payload, &[("MCP_TOKEN", canary.value())]);
    assert_canary_absent("codex stdout", &output.stdout, &canary);
    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    let reason = response["reason"].as_str().expect("a reason");
    assert!(reason.contains("<SECRET:MCP_TOKEN>"));
    assert!(reason.contains("sanitized textual rendering"));
}

#[test]
fn an_unresolved_source_is_silent() {
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");
    let output = home.run_hook(&post_tool_use(json!({"output": "text"})), &[]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn invalid_input_is_diagnosed_without_echoing_the_payload() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.enroll_env("GITHUB_TOKEN");

    for payload in [
        String::new(),
        String::from("{ not json"),
        String::from("[1, 2, 3]"),
        json!({"hook_event_name": "PreToolUse"}).to_string(),
        format!("{{\"leak\": \"{}\"}}", canary.value()),
    ] {
        let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);
        // `CLI-007`: a diagnosed failure still exits zero with valid protocol
        // output so the host can present the warning.
        assert_eq!(output.status.code(), Some(0));
        assert_canary_absent("codex stdout", &output.stdout, &canary);
        let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
        assert!(
            response.get("decision").is_none(),
            "no block on malfunction"
        );
        assert!(response["systemMessage"].is_string());
    }
}

#[test]
fn a_malfunction_warns_and_leaves_the_result_alone() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let home = Home::new();
    home.write_global_config("version = 1\n\n[[secret]]\nsource = \"unknown\"\n");

    let payload = post_tool_use(json!({"output": canary.value()}));
    let output = home.run_hook(&payload, &[("GITHUB_TOKEN", canary.value())]);
    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("codex stdout", &output.stdout, &canary);
    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(response.get("decision").is_none());
    assert!(
        response["systemMessage"]
            .as_str()
            .expect("a warning")
            .contains("doctor")
    );
}

#[test]
fn a_project_registry_is_selected_from_the_event_cwd() {
    // `CFG-005`: Codex has no stable initial root, so `cwd` is used.
    let canary = Canary::generate("PROJECT_TOKEN");
    let home = Home::new();
    home.write_global_config("version = 1\n");
    let project = home.root.join("project");
    std::fs::create_dir_all(&project).expect("project directory");
    std::fs::write(
        project.join(".secretsieve.toml"),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"PROJECT_TOKEN\"\n",
    )
    .expect("write project config");

    let payload = json!({
        "hook_event_name": "PostToolUse",
        "cwd": project.to_string_lossy(),
        "tool_name": "shell",
        "tool_response": {"output": canary.value()},
    })
    .to_string();
    let output = home.run_hook(&payload, &[("PROJECT_TOKEN", canary.value())]);
    assert_canary_absent("codex stdout", &output.stdout, &canary);
    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(
        response["reason"]
            .as_str()
            .expect("a reason")
            .contains("<SECRET:PROJECT_TOKEN>")
    );
}

#[test]
fn timeout_mapping_stays_inside_the_host_bound() {
    // `RUN-004`: the installed hook has a 5-second timeout, and a timeout is
    // fail-open on this host (`LIM-012`). This measures the covered path rather
    // than promising a maximum (`LIM-010`).
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
        &post_tool_use(json!({"output": bulk, "exit_code": 0})),
        &[("BULK_TOKEN", canary.value())],
    );
    let elapsed = started.elapsed();

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("codex stdout", &output.stdout, &canary);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "a 1 MiB payload took {elapsed:?}, which the host would time out"
    );
}
