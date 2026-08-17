//! Large-input and recursion behavior (`SRC-008`, `LIM-010`, `RUN-004`).
//!
//! V1 imposes no SecretSieve-specific size cap, so these tests measure what large
//! input actually does instead of asserting a maximum. They also prove that deeply
//! nested payloads cannot exhaust the stack.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use secretsieve::testing::{Canary, assert_canary_absent};
use serde_json::{Value, json};

struct Machine {
    root: PathBuf,
}

impl Machine {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "secretsieve-limits-{}-{}",
            std::process::id(),
            Canary::generate("FIXTURE").token()
        ));
        std::fs::create_dir_all(root.join("home").join("project")).expect("project");
        std::fs::create_dir_all(root.join("home").join(".config").join("secretsieve"))
            .expect("config directory");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn project(&self) -> PathBuf {
        self.home().join("project")
    }

    fn write_global(&self, contents: &str) {
        std::fs::write(
            self.home()
                .join(".config")
                .join("secretsieve")
                .join("config.toml"),
            contents,
        )
        .expect("write global config");
    }

    fn run_hook(&self, payload: &str, variables: &[(&str, &str)]) -> (Output, Duration) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_secretsieve"));
        command
            .args(["hook", "claude"])
            .current_dir(self.project())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in variables {
            command.env(key, value);
        }

        let started = Instant::now();
        let mut child = command.spawn().expect("the binary runs");
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(payload.as_bytes())
            .expect("write the payload");
        let output = child.wait_with_output().expect("the hook finishes");
        (output, started.elapsed())
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_large_wildcard_dotenv_file_is_resolved_without_a_cap() {
    // `SRC-008`: no SecretSieve-specific dotenv size cap.
    let canary = Canary::generate("BIG_TOKEN");
    let machine = Machine::new();
    machine.write_global("version = 1\n");

    let mut dotenv = String::with_capacity(4 * 1024 * 1024);
    let mut key = 0;
    while dotenv.len() < 4 * 1024 * 1024 {
        dotenv.push_str(&format!("KEY_{key}=value-{key}-padding-padding-padding\n"));
        key += 1;
    }
    dotenv.push_str(&format!("BIG_TOKEN={}\n", canary.value()));
    std::fs::write(machine.project().join(".env"), &dotenv).expect("write dotenv");
    std::fs::write(
        machine.project().join(".secretsieve.toml"),
        "version = 1\n\n[[secret]]\nsource = \"dotenv\"\nfile = \".env\"\nall = true\n",
    )
    .expect("write project config");

    let payload = json!({
        "hook_event_name": "PostToolUse",
        "cwd": machine.project().to_string_lossy(),
        "tool_name": "Bash",
        "tool_response": {"stdout": format!("token={}", canary.value())},
    })
    .to_string();
    let (output, elapsed) = machine.run_hook(&payload, &[]);

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("hook stdout", &output.stdout, &canary);
    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(
        response["hookSpecificOutput"]["updatedToolOutput"]["stdout"],
        json!("token=<SECRET:BIG_TOKEN>")
    );
    // `RUN-004`: the host allows five seconds. This records the observation
    // rather than promising a maximum (`LIM-010`).
    assert!(
        elapsed < Duration::from_secs(5),
        "a 4 MiB dotenv file with ~{key} keys took {elapsed:?}"
    );
}

#[test]
fn a_deeply_nested_payload_cannot_exhaust_the_stack() {
    let machine = Machine::new();
    machine.write_global("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"TOKEN\"\n");

    // Far deeper than any real tool result, and deeper than the JSON parser's own
    // recursion limit, so the adapter must diagnose it rather than crash.
    let depth = 20_000;
    let payload = format!(
        "{{\"hook_event_name\":\"PostToolUse\",\"tool_response\":{}{}{}}}",
        "[".repeat(depth),
        "\"x\"",
        "]".repeat(depth)
    );
    let (output, _) = machine.run_hook(&payload, &[("TOKEN", "value")]);

    // Exit zero with valid protocol output, whatever the parser decided
    // (`CLI-007`, `RUN-006`).
    assert_eq!(output.status.code(), Some(0));
    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON on stdout");
    assert!(response.is_object());
}

#[test]
fn a_moderately_nested_payload_is_still_redacted() {
    let canary = Canary::generate("NESTED_TOKEN");
    let machine = Machine::new();
    machine.write_global("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"NESTED_TOKEN\"\n");

    // Nesting a real host result plausibly reaches, well inside the JSON parser's
    // default recursion limit.
    let depth = 40;
    let payload = format!(
        "{{\"hook_event_name\":\"PostToolUse\",\"tool_response\":{}\"{}\"{}}}",
        "[".repeat(depth),
        canary.value(),
        "]".repeat(depth)
    );
    let (output, _) = machine.run_hook(&payload, &[("NESTED_TOKEN", canary.value())]);

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("hook stdout", &output.stdout, &canary);
    let text = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(text.contains("<SECRET:NESTED_TOKEN>"));
}

#[test]
fn many_enrolled_values_stay_inside_the_host_timeout() {
    // The `RUN-005` benchmark measures this properly; this guards the wiring at
    // a size a real machine can hit.
    let canary = Canary::generate("LAST_TOKEN");
    let machine = Machine::new();

    let mut config = String::from("version = 1\n");
    let mut variables: Vec<(String, String)> = Vec::new();
    for index in 0..200 {
        config.push_str(&format!(
            "\n[[secret]]\nsource = \"env\"\nname = \"VALUE_{index}\"\n"
        ));
        variables.push((format!("VALUE_{index}"), format!("value-{index}-padding")));
    }
    config.push_str("\n[[secret]]\nsource = \"env\"\nname = \"LAST_TOKEN\"\n");
    machine.write_global(&config);

    let mut payload_text = String::with_capacity(512 * 1024);
    while payload_text.len() < 512 * 1024 {
        payload_text.push_str("ordinary output line with nothing sensitive in it\n");
    }
    payload_text.push_str(canary.value());
    let payload = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_response": {"stdout": payload_text},
    })
    .to_string();

    let borrowed: Vec<(&str, &str)> = variables
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .chain(std::iter::once(("LAST_TOKEN", canary.value())))
        .collect();
    let (output, elapsed) = machine.run_hook(&payload, &borrowed);

    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("hook stdout", &output.stdout, &canary);
    assert!(
        elapsed < Duration::from_secs(5),
        "201 values over 512 KiB took {elapsed:?}"
    );
}
