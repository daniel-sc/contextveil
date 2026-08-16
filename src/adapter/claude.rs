//! Claude Code `PostToolUse` adapter.
//!
//! Claude is the production integration (`SUP-002`). The adapter parses the
//! native synchronous hook envelope on stdin, redacts every string value in
//! `tool_response`, and returns the result through
//! `hookSpecificOutput.updatedToolOutput` while preserving the host's exact
//! key and type shape (`CLA-002`).
//!
//! It never implements matching or resolution itself; those live in the core
//! (`architecture.md`). Failure policy is fail-open (`RUN-001`, `RUN-002`,
//! `LIM-012`): a diagnosed malfunction warns and leaves the original content in
//! place, and the process still exits zero so the host can present the warning
//! (`CLI-007`).

use serde_json::{Map, Value, json};

use crate::cli::Exit;
use crate::redact::redact_json;
use crate::registry::{self, Outcome as RegistryOutcome};
use crate::source::Environment;

/// Host protocol field names, kept together so a protocol change is one edit.
///
/// Verified against Claude Code 2.1.233: the host requires `hookEventName`
/// inside `hookSpecificOutput`, and for built-in tools it validates
/// `updatedToolOutput` against that tool's own result schema, reverting to the
/// original result when validation fails. Mutating only string leaves of the
/// received `tool_response` is therefore the only replacement that is accepted
/// for every tool (see `LIM-013` for the exposure that a rejection causes).
mod protocol {
    pub const EVENT_NAME: &str = "hook_event_name";
    pub const EVENT_POST_TOOL_USE: &str = "PostToolUse";
    pub const TOOL_RESPONSE: &str = "tool_response";
    pub const HOOK_SPECIFIC_OUTPUT: &str = "hookSpecificOutput";
    pub const HOOK_EVENT_NAME: &str = "hookEventName";
    pub const UPDATED_TOOL_OUTPUT: &str = "updatedToolOutput";
    pub const SYSTEM_MESSAGE: &str = "systemMessage";
}

/// What the adapter writes to stdout, plus the process exit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Complete stdout payload, or `None` for a silent clean event.
    pub stdout: Option<String>,
    pub exit: Exit,
}

impl Response {
    fn silent() -> Self {
        Self {
            stdout: None,
            exit: Exit::Ok,
        }
    }

    fn json(value: Value) -> Self {
        Self {
            stdout: Some(value.to_string()),
            exit: Exit::Ok,
        }
    }
}

/// Handles one `PostToolUse` event.
///
/// `payload` is the raw stdin text. Hook payloads always arrive on stdin and
/// responses always leave on structured stdout (`INT-003`).
pub fn handle(payload: &str, environment: &Environment) -> Response {
    let event = match parse_event(payload) {
        Ok(event) => event,
        // `RUN-006`: a malformed envelope or unknown event is a diagnosed
        // protocol malfunction. Warn without echoing the payload.
        Err(problem) => return warn(problem.message()),
    };

    let registry = match registry::build(environment) {
        RegistryOutcome::Ready(registry) => registry,
        // `RUN-001`: no partial redaction. The original content is passed
        // through by simply not returning a replacement.
        RegistryOutcome::Malfunction(malfunction) => return warn(&malfunction.message()),
    };

    let mut messages: Vec<String> = registry
        .warnings
        .iter()
        .map(|warning| warning.message().to_string())
        .collect();

    let Some(original) = event.tool_response else {
        // A valid event without covered content is not malformed (`RUN-006`).
        return finish(None, messages);
    };

    let mut updated = original.clone();
    let mut tally = registry.redactor.tally();
    let changed = redact_json(&mut updated, &registry.redactor, &mut tally);
    if !changed {
        return finish(None, messages);
    }

    let intervention = registry.redactor.intervention(&tally);
    if let Some(intervention) = &intervention {
        messages.push(intervention.summary());
    }
    finish(Some(updated), messages)
}

/// Builds the host response.
///
/// `CLA-003`: one safe `systemMessage` and never `additionalContext`.
fn finish(updated: Option<Value>, messages: Vec<String>) -> Response {
    if updated.is_none() && messages.is_empty() {
        // `RED-009`: clean events with valid configuration are silent.
        return Response::silent();
    }

    let mut response = Map::new();
    if let Some(updated) = updated {
        response.insert(
            protocol::HOOK_SPECIFIC_OUTPUT.to_string(),
            json!({
                protocol::HOOK_EVENT_NAME: protocol::EVENT_POST_TOOL_USE,
                protocol::UPDATED_TOOL_OUTPUT: updated,
            }),
        );
    }
    if !messages.is_empty() {
        response.insert(
            protocol::SYSTEM_MESSAGE.to_string(),
            Value::String(messages.join(" ")),
        );
    }
    Response::json(Value::Object(response))
}

/// Emits a secret-safe warning without mutating host content.
fn warn(message: &str) -> Response {
    Response::json(json!({ protocol::SYSTEM_MESSAGE: message }))
}

/// The covered fields of one `PostToolUse` envelope.
struct Event {
    tool_response: Option<Value>,
}

/// Why an envelope could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Problem {
    NotJson,
    NotAnObject,
    UnknownEvent,
}

impl Problem {
    fn message(&self) -> &'static str {
        match self {
            Problem::NotJson | Problem::NotAnObject => {
                "SecretSieve could not parse the hook payload and made no change. \
                 Run `secretsieve doctor`."
            }
            Problem::UnknownEvent => {
                "SecretSieve received an unexpected hook event and made no change. \
                 Run `secretsieve doctor`."
            }
        }
    }
}

fn parse_event(payload: &str) -> Result<Event, Problem> {
    let value: Value = serde_json::from_str(payload).map_err(|_| Problem::NotJson)?;
    let object = value.as_object().ok_or(Problem::NotAnObject)?;

    match object.get(protocol::EVENT_NAME).and_then(Value::as_str) {
        Some(protocol::EVENT_POST_TOOL_USE) => {}
        _ => return Err(Problem::UnknownEvent),
    }

    Ok(Event {
        tool_response: object.get(protocol::TOOL_RESPONSE).cloned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Canary, assert_canary_absent};

    struct Fixture {
        root: std::path::PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "secretsieve-claude-{}-{}",
                std::process::id(),
                Canary::generate("FIXTURE").token()
            ));
            std::fs::create_dir_all(root.join("secretsieve")).expect("fixture directory");
            Self { root }
        }

        fn enroll(&self, name: &str) {
            std::fs::write(
                self.root.join("secretsieve").join("config.toml"),
                format!("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"{name}\"\n"),
            )
            .expect("write global config");
        }

        fn environment(&self, pairs: &[(&str, &str)]) -> Environment {
            let mut variables = vec![(
                "XDG_CONFIG_HOME".to_string(),
                self.root.to_string_lossy().into_owned(),
            )];
            variables.extend(
                pairs
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string())),
            );
            Environment::from_pairs(variables)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn event(tool_response: Value) -> String {
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
    fn a_matched_value_is_replaced_and_reported() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);

        let payload = event(json!({
            "stdout": format!("{}\n", canary.value()),
            "stderr": "",
            "interrupted": false,
            "isImage": false,
        }));
        let response = handle(&payload, &environment);

        let stdout = response.stdout.expect("an intervention produces output");
        assert_eq!(response.exit, Exit::Ok);
        assert_canary_absent("claude stdout", stdout.as_bytes(), &canary);

        let value: Value = serde_json::from_str(&stdout).expect("valid JSON response");
        let updated = &value["hookSpecificOutput"]["updatedToolOutput"];
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert_eq!(updated["stdout"], json!("<SECRET:GITHUB_TOKEN>\n"));
        // Shape is preserved exactly: keys, types, and untouched values.
        assert_eq!(updated["stderr"], json!(""));
        assert_eq!(updated["interrupted"], json!(false));
        assert_eq!(updated["isImage"], json!(false));
        assert!(
            value["systemMessage"]
                .as_str()
                .expect("a safe system message")
                .contains("GITHUB_TOKEN")
        );
        assert!(value.get("additionalContext").is_none());
        assert!(
            value["hookSpecificOutput"]
                .get("additionalContext")
                .is_none()
        );
    }

    #[test]
    fn a_clean_event_is_silent() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);

        let response = handle(&event(json!({"stdout": "all clear\n"})), &environment);
        assert_eq!(response, Response::silent());
    }

    #[test]
    fn an_unresolved_source_stays_silent() {
        // `RED-009`: unresolved sources produce no runtime UI.
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[]);
        let response = handle(&event(json!({"stdout": "output\n"})), &environment);
        assert_eq!(response, Response::silent());
    }

    #[test]
    fn an_invalid_config_warns_without_mutating() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new();
        std::fs::write(
            fixture.root.join("secretsieve").join("config.toml"),
            "version = 1\n\n[[secret]]\nsource = \"nope\"\n",
        )
        .expect("write global config");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);

        let payload = event(json!({"stdout": canary.value()}));
        let response = handle(&payload, &environment);
        let stdout = response.stdout.expect("a warning is emitted");
        let value: Value = serde_json::from_str(&stdout).expect("valid JSON response");
        assert!(value.get("hookSpecificOutput").is_none());
        assert!(
            value["systemMessage"]
                .as_str()
                .expect("message")
                .contains("doctor")
        );
        assert_canary_absent("claude warning", stdout.as_bytes(), &canary);
        assert_eq!(response.exit, Exit::Ok);
    }

    #[test]
    fn a_missing_global_config_warns_but_keeps_project_behavior() {
        let fixture = Fixture::new();
        let environment = fixture.environment(&[]);
        let response = handle(&event(json!({"stdout": "output"})), &environment);
        let stdout = response.stdout.expect("a configuration warning is emitted");
        assert!(stdout.contains("setup is incomplete"));
        assert!(!stdout.contains("updatedToolOutput"));
    }

    #[test]
    fn malformed_input_is_diagnosed_without_echoing_the_payload() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);

        for payload in [
            String::from("not json at all"),
            String::from("[]"),
            json!({"hook_event_name": "PreToolUse"}).to_string(),
            json!({"tool_response": {"stdout": canary.value()}}).to_string(),
            format!("{{\"junk\": \"{}\"}}", canary.value()),
        ] {
            let response = handle(&payload, &environment);
            assert_eq!(response.exit, Exit::Ok, "hooks exit zero (`CLI-007`)");
            let stdout = response.stdout.expect("a warning is emitted");
            assert_canary_absent("claude malformed-input warning", stdout.as_bytes(), &canary);
            let value: Value = serde_json::from_str(&stdout).expect("valid JSON response");
            assert!(value.get("hookSpecificOutput").is_none());
            assert!(value["systemMessage"].is_string());
        }
    }

    #[test]
    fn an_event_without_covered_content_is_left_alone() {
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[("GITHUB_TOKEN", "value")]);
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "true"},
        })
        .to_string();
        assert_eq!(handle(&payload, &environment), Response::silent());
    }

    #[test]
    fn unknown_envelope_fields_are_not_malformed() {
        // `RUN-006`: uncovered content and future fields are preserved silently.
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[("GITHUB_TOKEN", "value")]);
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_response": {"stdout": "clean"},
            "future_field": {"nested": [1, 2, 3]},
        })
        .to_string();
        assert_eq!(handle(&payload, &environment), Response::silent());
    }

    #[test]
    fn string_tool_responses_are_redacted_in_place() {
        let canary = Canary::generate("API_KEY");
        let fixture = Fixture::new();
        fixture.enroll("API_KEY");
        let environment = fixture.environment(&[("API_KEY", canary.value())]);

        let payload = event(Value::String(format!("key={}", canary.value())));
        let response = handle(&payload, &environment);
        let stdout = response.stdout.expect("an intervention produces output");
        assert_canary_absent("claude stdout", stdout.as_bytes(), &canary);
        let value: Value = serde_json::from_str(&stdout).expect("valid JSON response");
        assert_eq!(
            value["hookSpecificOutput"]["updatedToolOutput"],
            json!("key=<SECRET:API_KEY>")
        );
    }
}
