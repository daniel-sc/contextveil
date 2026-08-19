//! Codex CLI `PostToolUse` adapter.
//!
//! Codex is experimental (`SUP-002`). The host provides no shape-preserving
//! result replacement: `updatedMCPToolOutput` is rejected outright, and the only
//! mechanism that changes what the model sees is a block decision whose `reason`
//! becomes the model-facing text (verified against openai/codex
//! `codex-rs/core/src/tools/registry.rs` and `codex-rs/hooks`).
//!
//! `COD-002`: on a match the adapter therefore blocks the original result and
//! supplies a sanitized textual rendering. `COD-003`: that rendering discloses
//! that a successful or structured result may now look error-like and may have
//! lost structure, images, or typed semantics (`LIM-014`).
//!
//! Failure policy is fail-open (`RUN-001`, `RUN-002`): a diagnosed malfunction
//! emits a `systemMessage` and no decision, so Codex keeps the original result.

use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::cli::Exit;
use crate::redact::redact_json;
use crate::registry::{self, Outcome as RegistryOutcome};
use crate::source::Environment;

/// Host protocol field names, kept together so a protocol change is one edit.
mod protocol {
    pub const EVENT_NAME: &str = "hook_event_name";
    pub const EVENT_POST_TOOL_USE: &str = "PostToolUse";
    pub const TOOL_RESPONSE: &str = "tool_response";
    pub const CWD: &str = "cwd";
    pub const DECISION: &str = "decision";
    pub const DECISION_BLOCK: &str = "block";
    pub const REASON: &str = "reason";
    pub const SYSTEM_MESSAGE: &str = "systemMessage";
}

/// What the adapter writes to stdout, plus the process exit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
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
pub fn handle(payload: &str, environment: &Environment) -> Response {
    let event = match parse_event(payload) {
        Ok(event) => event,
        // `RUN-006`: a malformed envelope or unknown event is a diagnosed
        // protocol malfunction. Warn without echoing the payload.
        Err(problem) => return warn(problem.message()),
    };

    let project_root = event
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let registry = match registry::build(environment, project_root.as_deref()) {
        RegistryOutcome::Ready(registry) => registry,
        // `RUN-001`: no partial redaction, and the host keeps the original.
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
    if !redact_json(&mut updated, &registry.redactor, &mut tally) {
        return finish(None, messages);
    }

    let intervention = registry.redactor.intervention(&tally);
    let summary = intervention
        .as_ref()
        .map(|intervention| intervention.summary())
        .unwrap_or_else(|| "ContextVeil redacted enrolled values".to_string());
    if let Some(intervention) = &intervention {
        messages.push(intervention.summary());
    }
    finish(Some(render(&updated, &summary)), messages)
}

/// Builds the host response.
fn finish(reason: Option<String>, messages: Vec<String>) -> Response {
    if reason.is_none() && messages.is_empty() {
        // `RED-009`: clean events with valid configuration are silent.
        return Response::silent();
    }
    let mut response = Map::new();
    if let Some(reason) = reason {
        response.insert(
            protocol::DECISION.to_string(),
            Value::String(protocol::DECISION_BLOCK.to_string()),
        );
        response.insert(protocol::REASON.to_string(), Value::String(reason));
    }
    if !messages.is_empty() {
        response.insert(
            protocol::SYSTEM_MESSAGE.to_string(),
            Value::String(messages.join(" ")),
        );
    }
    Response::json(Value::Object(response))
}

/// Renders a redacted result as the model-facing replacement text.
///
/// `COD-003`: the rendering states that the tool actually ran and succeeded and
/// that structure may have been lost, so the model does not treat the block as a
/// tool failure.
fn render(updated: &Value, summary: &str) -> String {
    let body = match updated {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    };
    format!(
        "{summary} before this result could reach you. The tool itself ran and did not fail. This \
         is a sanitized textual rendering, so structure, images, and typed fields may be missing.\n\n{body}"
    )
}

/// Emits a secret-safe warning without changing the host result.
fn warn(message: &str) -> Response {
    Response::json(json!({ protocol::SYSTEM_MESSAGE: message }))
}

/// The covered fields of one `PostToolUse` envelope.
struct Event {
    tool_response: Option<Value>,
    /// Codex has no stable project root field, so `cwd` is used (`CFG-005`).
    cwd: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Problem {
    NotAnObject,
    UnknownEvent,
}

impl Problem {
    fn message(&self) -> &'static str {
        match self {
            Problem::NotAnObject => {
                "ContextVeil could not parse the hook payload and made no change. \
                 Run `contextveil doctor`."
            }
            Problem::UnknownEvent => {
                "ContextVeil received an unexpected hook event and made no change. \
                 Run `contextveil doctor`."
            }
        }
    }
}

fn parse_event(payload: &str) -> Result<Event, Problem> {
    let value: Value = serde_json::from_str(payload).map_err(|_| Problem::NotAnObject)?;
    let object = value.as_object().ok_or(Problem::NotAnObject)?;
    match object.get(protocol::EVENT_NAME).and_then(Value::as_str) {
        Some(protocol::EVENT_POST_TOOL_USE) => {}
        _ => return Err(Problem::UnknownEvent),
    }
    Ok(Event {
        tool_response: object.get(protocol::TOOL_RESPONSE).cloned(),
        cwd: object
            .get(protocol::CWD)
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Canary, assert_canary_absent};

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "contextveil-codex-{}-{}",
                std::process::id(),
                Canary::generate("CODEX").token()
            ));
            std::fs::create_dir_all(root.join("contextveil")).expect("config directory");
            Self { root }
        }

        fn write_global(&self, contents: &str) {
            std::fs::write(self.root.join("contextveil").join("config.toml"), contents)
                .expect("write global config");
        }

        fn enroll(&self, name: &str) {
            self.write_global(&format!(
                "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"{name}\"\n"
            ));
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

    fn parsed(response: &Response) -> Value {
        serde_json::from_str(response.stdout.as_deref().expect("output")).expect("valid JSON")
    }

    #[test]
    fn a_match_blocks_the_original_and_supplies_sanitized_text() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);

        let payload = event(json!({"output": format!("{}\n", canary.value()), "exit_code": 0}));
        let response = handle(&payload, &environment);
        assert_eq!(response.exit, Exit::Ok);
        assert_canary_absent(
            "codex stdout",
            response.stdout.as_deref().expect("output").as_bytes(),
            &canary,
        );

        let value = parsed(&response);
        assert_eq!(value["decision"], json!("block"));
        let reason = value["reason"].as_str().expect("reason");
        assert!(reason.contains("<SECRET:GITHUB_TOKEN>"));
        // `COD-003`: the degradation is disclosed in the model-facing text.
        assert!(reason.contains("did not fail"));
        assert!(reason.contains("sanitized textual rendering"));
        assert!(value["systemMessage"].is_string());
    }

    #[test]
    fn a_string_result_is_rendered_directly() {
        let canary = Canary::generate("API_KEY");
        let fixture = Fixture::new();
        fixture.enroll("API_KEY");
        let environment = fixture.environment(&[("API_KEY", canary.value())]);

        let payload = event(Value::String(format!("key={}", canary.value())));
        let value = parsed(&handle(&payload, &environment));
        let reason = value["reason"].as_str().expect("reason");
        assert!(reason.ends_with("key=<SECRET:API_KEY>"));
    }

    #[test]
    fn a_non_zero_exit_result_is_still_covered() {
        // Verified against openai/codex: a shell command that exits non-zero is
        // still a successful tool call, so the event fires and is covered.
        let canary = Canary::generate("TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("TOKEN");
        let environment = fixture.environment(&[("TOKEN", canary.value())]);

        let payload = event(json!({
            "output": format!("error: bad token {}\n", canary.value()),
            "exit_code": 1,
        }));
        let value = parsed(&handle(&payload, &environment));
        assert_eq!(value["decision"], json!("block"));
        assert!(
            value["reason"]
                .as_str()
                .expect("reason")
                .contains("<SECRET:TOKEN>")
        );
    }

    #[test]
    fn structured_results_keep_their_shape_inside_the_rendering() {
        let canary = Canary::generate("MCP_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("MCP_TOKEN");
        let environment = fixture.environment(&[("MCP_TOKEN", canary.value())]);

        let payload = event(json!({
            "content": [{"type": "text", "text": format!("token {}", canary.value())}],
            "isError": false,
        }));
        let value = parsed(&handle(&payload, &environment));
        let reason = value["reason"].as_str().expect("reason");
        assert!(reason.contains("\"isError\": false"));
        assert!(reason.contains("<SECRET:MCP_TOKEN>"));
    }

    #[test]
    fn a_clean_event_is_silent() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);
        let response = handle(&event(json!({"output": "all clear"})), &environment);
        assert_eq!(response, Response::silent());
    }

    #[test]
    fn an_unresolved_source_stays_silent() {
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let response = handle(&event(json!({"output": "text"})), &fixture.environment(&[]));
        assert_eq!(response, Response::silent());
    }

    #[test]
    fn a_malfunction_warns_and_never_blocks() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new();
        fixture.write_global("version = 1\n\n[[secret]]\nsource = \"nope\"\n");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);

        let response = handle(&event(json!({"output": canary.value()})), &environment);
        let value = parsed(&response);
        assert!(value.get("decision").is_none());
        assert!(
            value["systemMessage"]
                .as_str()
                .expect("message")
                .contains("doctor")
        );
        assert_canary_absent(
            "codex stdout",
            response.stdout.as_deref().expect("output").as_bytes(),
            &canary,
        );
    }

    #[test]
    fn malformed_input_is_diagnosed_without_echoing_the_payload() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);

        for payload in [
            String::new(),
            String::from("not json"),
            String::from("[]"),
            json!({"hook_event_name": "PreToolUse"}).to_string(),
            format!("{{\"leak\":\"{}\"}}", canary.value()),
        ] {
            let response = handle(&payload, &environment);
            assert_eq!(response.exit, Exit::Ok);
            let stdout = response.stdout.as_deref().expect("output");
            assert_canary_absent("codex stdout", stdout.as_bytes(), &canary);
            let value: Value = serde_json::from_str(stdout).expect("valid JSON");
            assert!(value.get("decision").is_none());
            assert!(value["systemMessage"].is_string());
        }
    }

    #[test]
    fn an_event_without_covered_content_is_left_alone() {
        let fixture = Fixture::new();
        fixture.enroll("TOKEN");
        let environment = fixture.environment(&[("TOKEN", "value")]);
        let payload = json!({"hook_event_name": "PostToolUse", "tool_name": "shell"}).to_string();
        assert_eq!(handle(&payload, &environment), Response::silent());
    }

    #[test]
    fn unknown_envelope_fields_are_not_malformed() {
        let fixture = Fixture::new();
        fixture.enroll("TOKEN");
        let environment = fixture.environment(&[("TOKEN", "value")]);
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_response": {"output": "clean"},
            "future_field": [1, 2, 3],
        })
        .to_string();
        assert_eq!(handle(&payload, &environment), Response::silent());
    }
}
