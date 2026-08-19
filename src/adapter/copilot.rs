//! GitHub Copilot CLI adapter.
//!
//! Copilot is experimental (`SUP-002`). `COP-002`: the adapter redacts
//! `userPromptTransformed` model-facing text and successful `postToolUse`
//! `toolResult.textResultForLlm` text, preserving each documented host result
//! shape. `COP-003`: on intervention it emits one safe progress summary before
//! its final mutation object.
//!
//! Verified against the Copilot CLI hooks reference (CLI 1.0.80): the covered
//! events are separate hooks with their own payloads, `modifiedResult` is honored
//! for command hooks, a progress line is any single-line JSON object with
//! `"type": "progress"`, exit 0 carries the mutation, and exit 2 surfaces stderr
//! as a warning while the run continues. Failed tool results arrive on the
//! separate `postToolUseFailure` event and are not covered (`COP-004`,
//! `LIM-015`).

use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::cli::Exit;
use crate::registry::{self, Outcome as RegistryOutcome};
use crate::source::Environment;

/// Host protocol field names, kept together so a protocol change is one edit.
mod protocol {
    pub const CWD: &str = "cwd";
    pub const TRANSFORMED_PROMPT: &str = "transformedPrompt";
    pub const MODIFIED_TRANSFORMED_PROMPT: &str = "modifiedTransformedPrompt";
    pub const TOOL_RESULT: &str = "toolResult";
    pub const MODIFIED_RESULT: &str = "modifiedResult";
    pub const RESULT_TYPE: &str = "resultType";
    pub const RESULT_SUCCESS: &str = "success";
    pub const TEXT_RESULT: &str = "textResultForLlm";
}

/// Which covered event this invocation serves.
///
/// The host payloads carry no event name, so the installed command names the
/// event in its own arguments and the payload shape is validated against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    TransformedPrompt,
    PostToolUse,
}

impl Event {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "prompt" => Some(Event::TransformedPrompt),
            "tool" => Some(Event::PostToolUse),
            _ => None,
        }
    }
}

/// What the adapter writes, plus the process exit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Progress lines followed by the final mutation object, if any.
    pub stdout: Option<String>,
    /// Secret-safe warning text for stderr, if any.
    pub stderr: Option<String>,
    pub exit: Exit,
}

impl Response {
    fn silent() -> Self {
        Self {
            stdout: None,
            stderr: None,
            exit: Exit::Ok,
        }
    }

    /// A diagnosed malfunction.
    ///
    /// Copilot surfaces stderr as a warning when a hook exits 2 and continues
    /// the run with the original content, which is the only channel this host
    /// offers for a warning (`RUN-001`, `CLI-007`).
    fn warn(message: &str) -> Self {
        Self {
            stdout: None,
            stderr: Some(message.to_string()),
            exit: Exit::Usage,
        }
    }
}

/// Handles one covered event.
pub fn handle(event: Event, payload: &str, environment: &Environment) -> Response {
    let Ok(object) = serde_json::from_str::<Value>(payload) else {
        return Response::warn(MALFORMED);
    };
    let Some(object) = object.as_object() else {
        return Response::warn(MALFORMED);
    };

    let project_root = object
        .get(protocol::CWD)
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());

    match event {
        Event::TransformedPrompt => {
            let Some(prompt) = object
                .get(protocol::TRANSFORMED_PROMPT)
                .and_then(Value::as_str)
            else {
                // `RUN-006`: the payload does not match the event this command
                // serves, so it is a diagnosed protocol malfunction.
                return Response::warn(MALFORMED);
            };
            redact_one(
                prompt,
                project_root,
                environment,
                |replaced| json!({ protocol::MODIFIED_TRANSFORMED_PROMPT: replaced }),
            )
        }
        Event::PostToolUse => {
            let Some(result) = object.get(protocol::TOOL_RESULT).and_then(Value::as_object) else {
                return Response::warn(MALFORMED);
            };
            // `COP-002`, `COP-004`: successful textual results only. A failed
            // result arrives on a different event and is not covered.
            if result.get(protocol::RESULT_TYPE).and_then(Value::as_str)
                != Some(protocol::RESULT_SUCCESS)
            {
                return Response::silent();
            }
            let Some(text) = result.get(protocol::TEXT_RESULT).and_then(Value::as_str) else {
                return Response::silent();
            };
            let original = result.clone();
            redact_one(text, project_root, environment, move |replaced| {
                // Preserve every documented field of the host result shape and
                // change only the model-facing text.
                let mut updated: Map<String, Value> = original.clone();
                updated.insert(
                    protocol::TEXT_RESULT.to_string(),
                    Value::String(replaced.to_string()),
                );
                json!({ protocol::MODIFIED_RESULT: Value::Object(updated) })
            })
        }
    }
}

const MALFORMED: &str =
    "ContextVeil could not use this hook payload and made no change. Run `contextveil doctor`.";

/// Redacts one string and builds the host mutation object around it.
fn redact_one(
    text: &str,
    project_root: Option<PathBuf>,
    environment: &Environment,
    mutation: impl FnOnce(&str) -> Value,
) -> Response {
    let registry = match registry::build(environment, project_root.as_deref()) {
        RegistryOutcome::Ready(registry) => registry,
        // `RUN-001`: no partial redaction; the host keeps the original content.
        RegistryOutcome::Malfunction(malfunction) => return Response::warn(&malfunction.message()),
    };

    let mut tally = registry.redactor.tally();
    let Some(replaced) = registry.redactor.redact(text, &mut tally) else {
        // `RED-009`: clean events are silent. A configuration warning still
        // reaches the user through the host's warning channel.
        return match registry.warnings.first() {
            None => Response::silent(),
            Some(warning) => Response::warn(warning.message()),
        };
    };

    let mut lines = Vec::new();
    if let Some(intervention) = registry.redactor.intervention(&tally) {
        // `COP-003`: one safe progress summary before the final object.
        lines.push(json!({"type": "progress", "message": intervention.summary()}).to_string());
    }
    lines.push(mutation(&replaced).to_string());

    Response {
        stdout: Some(lines.join("\n")),
        stderr: None,
        exit: Exit::Ok,
    }
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
                "contextveil-copilot-{}-{}",
                std::process::id(),
                Canary::generate("COPILOT").token()
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

    fn prompt_payload(text: &str) -> String {
        json!({
            "sessionId": "s1",
            "timestamp": 0,
            "cwd": "/home/user/project",
            "prompt": text,
            "transformedPrompt": text,
        })
        .to_string()
    }

    fn tool_payload(result_type: &str, text: &str) -> String {
        json!({
            "sessionId": "s1",
            "timestamp": 0,
            "cwd": "/home/user/project",
            "toolName": "shell",
            "toolArgs": {"command": "printenv GITHUB_TOKEN"},
            "toolResult": {"resultType": result_type, "textResultForLlm": text},
        })
        .to_string()
    }

    /// Splits progress lines from the final object, the way Copilot does.
    fn split(response: &Response) -> (Vec<Value>, Value) {
        let stdout = response.stdout.as_deref().expect("output");
        let mut progress = Vec::new();
        let mut final_object = Value::Null;
        for line in stdout.lines() {
            let value: Value = serde_json::from_str(line).expect("each line is JSON");
            if value.get("type").and_then(Value::as_str) == Some("progress") {
                progress.push(value);
            } else {
                final_object = value;
            }
        }
        (progress, final_object)
    }

    #[test]
    fn a_transformed_prompt_is_redacted_with_one_progress_line() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("GITHUB_TOKEN");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);

        let payload = prompt_payload(&format!("deploy with {}", canary.value()));
        let response = handle(Event::TransformedPrompt, &payload, &environment);
        assert_eq!(response.exit, Exit::Ok);
        assert_eq!(response.stderr, None);
        assert_canary_absent(
            "copilot stdout",
            response.stdout.as_deref().expect("output").as_bytes(),
            &canary,
        );

        let (progress, final_object) = split(&response);
        assert_eq!(progress.len(), 1);
        assert!(
            progress[0]["message"]
                .as_str()
                .expect("message")
                .contains("GITHUB_TOKEN")
        );
        assert_eq!(
            final_object["modifiedTransformedPrompt"],
            json!("deploy with <SECRET:GITHUB_TOKEN>")
        );
    }

    #[test]
    fn a_successful_tool_result_keeps_its_shape() {
        let canary = Canary::generate("API_KEY");
        let fixture = Fixture::new();
        fixture.enroll("API_KEY");
        let environment = fixture.environment(&[("API_KEY", canary.value())]);

        let payload = tool_payload("success", &format!("key={}", canary.value()));
        let response = handle(Event::PostToolUse, &payload, &environment);
        let (_, final_object) = split(&response);
        let result = &final_object["modifiedResult"];
        assert_eq!(result["resultType"], json!("success"));
        assert_eq!(result["textResultForLlm"], json!("key=<SECRET:API_KEY>"));
    }

    #[test]
    fn extra_result_fields_are_preserved() {
        let canary = Canary::generate("API_KEY");
        let fixture = Fixture::new();
        fixture.enroll("API_KEY");
        let environment = fixture.environment(&[("API_KEY", canary.value())]);

        let payload = json!({
            "cwd": "/home/user/project",
            "toolName": "shell",
            "toolResult": {
                "resultType": "success",
                "textResultForLlm": canary.value(),
                "durationMs": 12,
                "extra": {"nested": true},
            },
        })
        .to_string();
        let (_, final_object) = split(&handle(Event::PostToolUse, &payload, &environment));
        let result = &final_object["modifiedResult"];
        assert_eq!(result["durationMs"], json!(12));
        assert_eq!(result["extra"]["nested"], json!(true));
        assert_eq!(result["textResultForLlm"], json!("<SECRET:API_KEY>"));
    }

    #[test]
    fn a_failed_tool_result_is_not_covered() {
        // `COP-004`: failed errors are outside V1 coverage.
        let canary = Canary::generate("API_KEY");
        let fixture = Fixture::new();
        fixture.enroll("API_KEY");
        let environment = fixture.environment(&[("API_KEY", canary.value())]);

        let payload = tool_payload("failure", canary.value());
        assert_eq!(
            handle(Event::PostToolUse, &payload, &environment),
            Response::silent()
        );
    }

    #[test]
    fn clean_events_are_silent() {
        let canary = Canary::generate("API_KEY");
        let fixture = Fixture::new();
        fixture.enroll("API_KEY");
        let environment = fixture.environment(&[("API_KEY", canary.value())]);

        assert_eq!(
            handle(
                Event::TransformedPrompt,
                &prompt_payload("nothing sensitive"),
                &environment
            ),
            Response::silent()
        );
        assert_eq!(
            handle(
                Event::PostToolUse,
                &tool_payload("success", "nothing sensitive"),
                &environment
            ),
            Response::silent()
        );
    }

    #[test]
    fn a_malfunction_warns_through_the_hosts_warning_channel() {
        let canary = Canary::generate("API_KEY");
        let fixture = Fixture::new();
        fixture.write_global("version = 1\n\n[[secret]]\nsource = \"nope\"\n");
        let environment = fixture.environment(&[("API_KEY", canary.value())]);

        let response = handle(
            Event::PostToolUse,
            &tool_payload("success", canary.value()),
            &environment,
        );
        assert_eq!(response.stdout, None, "no mutation on malfunction");
        let stderr = response.stderr.expect("a warning");
        assert!(stderr.contains("doctor"));
        assert_canary_absent("copilot stderr", stderr.as_bytes(), &canary);
        // Exit 2 is how this host surfaces a warning while continuing the run.
        assert_eq!(response.exit, Exit::Usage);
    }

    #[test]
    fn a_payload_that_does_not_match_the_event_is_diagnosed() {
        let fixture = Fixture::new();
        fixture.enroll("API_KEY");
        let environment = fixture.environment(&[("API_KEY", "value")]);

        // A tool payload sent to the prompt entry point, and the reverse.
        let response = handle(
            Event::TransformedPrompt,
            &tool_payload("success", "text"),
            &environment,
        );
        assert_eq!(response.exit, Exit::Usage);
        assert!(response.stderr.expect("warning").contains("doctor"));

        let response = handle(Event::PostToolUse, &prompt_payload("text"), &environment);
        assert_eq!(response.exit, Exit::Usage);
        assert!(response.stderr.is_some());
    }

    #[test]
    fn malformed_input_never_echoes_the_payload() {
        let canary = Canary::generate("API_KEY");
        let fixture = Fixture::new();
        fixture.enroll("API_KEY");
        let environment = fixture.environment(&[("API_KEY", canary.value())]);

        for payload in [
            String::new(),
            String::from("not json"),
            String::from("[]"),
            format!("{{\"leak\":\"{}\"}}", canary.value()),
        ] {
            for event in [Event::TransformedPrompt, Event::PostToolUse] {
                let response = handle(event, &payload, &environment);
                assert_eq!(response.stdout, None);
                let stderr = response.stderr.expect("a warning");
                assert_canary_absent("copilot stderr", stderr.as_bytes(), &canary);
            }
        }
    }

    #[test]
    fn event_names_map_to_the_installed_arguments() {
        assert_eq!(Event::parse("prompt"), Some(Event::TransformedPrompt));
        assert_eq!(Event::parse("tool"), Some(Event::PostToolUse));
        assert_eq!(Event::parse("other"), None);
    }
}
