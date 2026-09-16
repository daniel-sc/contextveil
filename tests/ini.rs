//! INI parser and process-boundary conformance (`SRC-019`, `TST-002`, `TST-005`).
//!
//! Safe diagnostics and malformed sources at a real adapter boundary. Exact and
//! wildcard coverage lives in `tests/process_boundaries.rs`; setup cases live in
//! `tests/setup.rs`.

mod common;

use common::ProcessFixture;
use contextveil::testing::{Canary, assert_canary_absent};
use serde_json::{Value, json};

fn payload(value: &str) -> String {
    json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_response": {"stdout": format!("token={value}")},
    })
    .to_string()
}

fn stdout_json(stdout: &[u8]) -> Value {
    serde_json::from_slice(stdout).expect("valid Claude hook response")
}

#[test]
fn malformed_ini_passes_original_content_with_value_free_warning() {
    let canary = Canary::generate("VALID_DOTENV_TOKEN");
    let fixture = ProcessFixture::new(None);
    fixture.write_project_file(
        "valid.env",
        &format!("VALID_DOTENV_TOKEN={}\n", canary.value()),
    );
    fixture.write_project_file(
        "broken.ini",
        "[Production]\nTOKEN=ordinary-value\nmalformed assignment\n",
    );
    fixture.write_project_config(
        "version = 1\n\n[[secret]]\nsource = \"dotenv\"\nfile = \"valid.env\"\nkey = \"VALID_DOTENV_TOKEN\"\n\n[[secret]]\nsource = \"ini\"\nfile = \"broken.ini\"\nsection = \"Production\"\nkey = \"TOKEN\"\n",
    );

    let input = payload(canary.value());
    let output = fixture.run(&["hook", "claude"], input.as_bytes(), &[]);
    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("malfunction stdout", &output.stdout, &canary);
    assert_canary_absent("malfunction stderr", &output.stderr, &canary);
    let response = stdout_json(&output.stdout);
    let warning = response["systemMessage"]
        .as_str()
        .expect("malfunction warning");
    assert!(warning.contains("malformed"));
    // A malformed enrolled INI file disables the complete effective registry;
    // the valid dotenv source must not be applied partially.
    assert!(response.get("hookSpecificOutput").is_none());
}

#[test]
fn doctor_reports_ini_duplicates_and_uses_a_safe_key_label() {
    let canary = Canary::generate("INI_DOCTOR_TOKEN");
    let fixture = ProcessFixture::new(None);
    fixture.write_project_file(
        "credentials.ini",
        &format!(
            "[Production]\nAPI_TOKEN=first\n[Production]\nAPI_TOKEN={}\n",
            canary.value()
        ),
    );
    fixture.write_project_config(
        "version = 1\n\n[[secret]]\nsource = \"ini\"\nfile = \"credentials.ini\"\nsection = \"Production\"\nkey = \"API_TOKEN\"\n",
    );

    let output = fixture.run(&["doctor"], &[], &[]);
    // No harness is installed in this focused fixture, so doctor may report
    // that independent health failure alongside the duplicate warning.
    assert_canary_absent("doctor stdout", &output.stdout, &canary);
    assert_canary_absent("doctor stderr", &output.stderr, &canary);
    let text = String::from_utf8(output.stdout).expect("UTF-8 doctor output");
    assert!(text.contains("ini "), "INI source label missing: {text}");
    assert!(text.contains("API_TOKEN"), "safe key label missing: {text}");
    assert!(
        text.contains("more than once"),
        "duplicate warning missing: {text}"
    );
}

#[test]
fn invalid_utf8_ini_is_a_malfunction_without_raw_bytes_in_diagnostics() {
    let fixture = ProcessFixture::new(None);
    std::fs::write(
        fixture.project_dir().join("invalid.ini"),
        b"[section]\nTOKEN=\xff\n",
    )
    .expect("invalid UTF-8 INI source");
    fixture.write_project_config(
        "version = 1\n\n[[secret]]\nsource = \"ini\"\nfile = \"invalid.ini\"\nsection = \"section\"\nkey = \"TOKEN\"\n",
    );
    let input = payload("ordinary-value");
    let output = fixture.run(&["hook", "claude"], input.as_bytes(), &[]);
    assert_eq!(output.status.code(), Some(0));
    assert!(!output.stdout.contains(&0xff));
    assert!(stdout_json(&output.stdout)["systemMessage"].is_string());
}
