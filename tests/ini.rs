//! INI parser and process-boundary conformance (`SRC-019`, `TST-002`, `TST-005`).
//!
//! Exact and section-wildcard resolution, safe diagnostics, and malformed
//! sources at a real adapter boundary. Setup cases live in `tests/setup.rs`.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use contextveil::testing::{Canary, assert_canary_absent, assert_canary_present};
use serde_json::{Value, json};

struct Machine {
    root: PathBuf,
}

impl Machine {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "contextveil-ini-{}-{}",
            std::process::id(),
            Canary::generate("FIXTURE").token()
        ));
        std::fs::create_dir_all(root.join("home").join("project")).expect("project directory");
        std::fs::create_dir_all(root.join("contextveil")).expect("config directory");
        std::fs::write(
            root.join("contextveil").join("config.toml"),
            "version = 1\n",
        )
        .expect("global config");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn project(&self) -> PathBuf {
        self.home().join("project")
    }

    fn project_config(&self, source: &str) {
        std::fs::write(
            self.project().join(".contextveil.toml"),
            format!("version = 1\n\n[[secret]]\n{source}"),
        )
        .expect("project config");
    }

    fn run_hook(&self, payload: &str) -> Output {
        assert!(self.root.join("contextveil/config.toml").exists());
        assert!(self.project().join(".contextveil.toml").exists());
        let mut command = Command::new(env!("CARGO_BIN_EXE_contextveil"));
        command
            .args(["hook", "claude"])
            .current_dir(self.project())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", &self.root)
            .env("CLAUDE_PROJECT_DIR", self.project())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("the hook runs");
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(payload.as_bytes())
            .expect("write payload");
        child.wait_with_output().expect("the hook exits")
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_contextveil"))
            .args(arguments)
            .current_dir(self.project())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", &self.root)
            .output()
            .expect("the command runs")
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn payload(value: &str) -> String {
    json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_response": {"stdout": format!("token={value}")},
    })
    .to_string()
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("valid Claude hook response")
}

#[test]
fn exact_ini_entry_is_redacted_at_the_claude_model_boundary() {
    let canary = Canary::generate("INI_TOKEN");
    let machine = Machine::new();
    std::fs::write(
        machine.project().join("credentials.ini"),
        format!("[Production]\nTOKEN=  {}  \n", canary.value()),
    )
    .expect("INI source");
    machine.project_config(
        "source = \"ini\"\nfile = \"credentials.ini\"\nsection = \"Production\"\nkey = \"TOKEN\"\n",
    );

    let input = payload(canary.value());
    assert_canary_present("INI payload", input.as_bytes(), &canary);
    let output = machine.run_hook(&input);
    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("INI stdout", &output.stdout, &canary);
    assert_canary_absent("INI stderr", &output.stderr, &canary);
    assert_eq!(
        stdout_json(&output)["hookSpecificOutput"]["updatedToolOutput"]["stdout"],
        json!("token=<SECRET:TOKEN>")
    );
}

#[test]
fn ini_section_wildcard_covers_sectionless_current_and_future_entries() {
    let sectionless = Canary::generate("INI_SECTIONLESS_TOKEN");
    let current = Canary::generate("INI_CURRENT_TOKEN");
    let future = Canary::generate("INI_FUTURE_TOKEN");
    let machine = Machine::new();
    let path = machine.project().join("credentials.ini");
    std::fs::write(
        &path,
        format!(
            "TOKEN={}\n[Production]\nTOKEN={}\n",
            sectionless.value(),
            current.value()
        ),
    )
    .expect("INI source");
    machine.project_config(
        "source = \"ini\"\nfile = \"credentials.ini\"\nall_sections = true\nkey = \"TOKEN\"\n",
    );

    for (label, value, canary) in [
        ("sectionless", sectionless.value().to_string(), &sectionless),
        ("current", current.value().to_string(), &current),
    ] {
        let output = machine.run_hook(&payload(&value));
        assert_eq!(output.status.code(), Some(0), "{label}");
        assert_canary_absent(&format!("{label} stdout"), &output.stdout, canary);
        assert_canary_absent(&format!("{label} stderr"), &output.stderr, canary);
        assert_eq!(
            stdout_json(&output)["hookSpecificOutput"]["updatedToolOutput"]["stdout"],
            json!("token=<SECRET:TOKEN>"),
            "{label} did not produce an intervention"
        );
    }

    // A section added after enrollment is protected on the next event.
    let mut contents = std::fs::read_to_string(&path).expect("read INI");
    contents.push_str(&format!("[Future]\nTOKEN={}\n", future.value()));
    std::fs::write(path, contents).expect("rotate INI");
    let output = machine.run_hook(&payload(future.value()));
    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("future section stdout", &output.stdout, &future);
    assert_canary_absent("future section stderr", &output.stderr, &future);
    assert_eq!(
        stdout_json(&output)["hookSpecificOutput"]["updatedToolOutput"]["stdout"],
        json!("token=<SECRET:TOKEN>"),
        "future section did not produce an intervention"
    );
}

#[test]
fn malformed_ini_passes_original_content_with_value_free_warning() {
    let canary = Canary::generate("VALID_DOTENV_TOKEN");
    let machine = Machine::new();
    std::fs::write(
        machine.project().join("valid.env"),
        format!("VALID_DOTENV_TOKEN={}\n", canary.value()),
    )
    .expect("valid companion source");
    std::fs::write(
        machine.project().join("broken.ini"),
        "[Production]\nTOKEN=ordinary-value\nmalformed assignment\n",
    )
    .expect("malformed INI source");
    std::fs::write(
        machine.project().join(".contextveil.toml"),
        "version = 1\n\n[[secret]]\nsource = \"dotenv\"\nfile = \"valid.env\"\nkey = \"VALID_DOTENV_TOKEN\"\n\n[[secret]]\nsource = \"ini\"\nfile = \"broken.ini\"\nsection = \"Production\"\nkey = \"TOKEN\"\n",
    )
    .expect("project config");

    let input = payload(canary.value());
    let output = machine.run_hook(&input);
    assert_eq!(output.status.code(), Some(0));
    assert_canary_absent("malfunction stdout", &output.stdout, &canary);
    assert_canary_absent("malfunction stderr", &output.stderr, &canary);
    let response = stdout_json(&output);
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
    let machine = Machine::new();
    std::fs::write(
        machine.project().join("credentials.ini"),
        format!(
            "[Production]\nAPI_TOKEN=first\n[Production]\nAPI_TOKEN={}\n",
            canary.value()
        ),
    )
    .expect("INI source");
    machine.project_config(
        "source = \"ini\"\nfile = \"credentials.ini\"\nsection = \"Production\"\nkey = \"API_TOKEN\"\n",
    );

    let output = machine.run(&["doctor"]);
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
    let machine = Machine::new();
    std::fs::write(
        machine.project().join("invalid.ini"),
        b"[section]\nTOKEN=\xff\n",
    )
    .expect("invalid UTF-8 INI source");
    machine.project_config(
        "source = \"ini\"\nfile = \"invalid.ini\"\nsection = \"section\"\nkey = \"TOKEN\"\n",
    );
    let output = machine.run_hook(&payload("ordinary-value"));
    assert_eq!(output.status.code(), Some(0));
    assert!(!output.stdout.contains(&0xff));
    assert!(stdout_json(&output)["systemMessage"].is_string());
}
