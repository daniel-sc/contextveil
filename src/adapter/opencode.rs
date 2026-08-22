//! OpenCode adapter: the Rust side of the plugin transport.
//!
//! `OCO-001`: the managed TypeScript plugin invokes this binary with one JSON
//! request on stdin and reads one JSON response from stdout. The request and
//! response shapes below are ContextVeil's own contract, not OpenCode's, so the
//! plugin stays a thin translator with no matcher or resolver semantics
//! (`architecture.md`, `OCO-004`).
//!
//! `RUN-003`: a malfunction must abort the covered operation, so a malfunction is
//! reported as a status the plugin turns into a thrown error rather than as
//! silently unchanged text. `RUN-006` applies the same rule to a malformed or
//! unknown request.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cli::Exit;
use crate::registry::{self, Outcome as RegistryOutcome};
use crate::source::Environment;

/// Protocol version of the request and response envelope.
pub const PROTOCOL_VERSION: u32 = 1;

/// The covered OpenCode hook that produced this request (`OCO-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// New textual parts of a user message.
    #[serde(rename = "chat.message")]
    ChatMessage,
    /// Successful standard textual tool output.
    #[serde(rename = "tool.execute.after")]
    ToolExecuteAfter,
}

/// One request from the plugin.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub event: Event,
    /// OpenCode's stable project or worktree root (`CFG-005`).
    #[serde(default)]
    pub project_root: Option<String>,
    /// The model-visible strings to redact, in plugin order.
    pub texts: Vec<String>,
}

/// One response to the plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Response {
    /// Redaction ran. `texts` is always the full list in request order.
    Ok {
        version: u32,
        changed: bool,
        texts: Vec<String>,
        /// Emit-safe named and count summary, present only on intervention
        /// (`OCO-003`, `RED-008`).
        #[serde(skip_serializing_if = "Option::is_none")]
        notification: Option<String>,
        /// Configuration warnings such as an incomplete global setup
        /// (`CFG-013`).
        #[serde(skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
    },
    /// The registry cannot be trusted; the plugin must abort (`RUN-003`).
    Malfunction { version: u32, message: String },
    /// The request itself was malformed or unknown (`RUN-006`).
    ProtocolError { version: u32, message: String },
}

impl Response {
    /// Serialized response, always exactly one JSON object on stdout.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            // Serializing counts and static text cannot fail in practice; if it
            // ever did, the plugin must still receive a valid abort signal.
            format!(
                r#"{{"status":"protocol-error","version":{PROTOCOL_VERSION},"message":"the response could not be serialized"}}"#
            )
        })
    }

    /// The process exit status. The plugin reads the status field, so the
    /// process itself always exits zero when it produced a response.
    pub fn exit(&self) -> Exit {
        Exit::Ok
    }
}

/// Handles one plugin request.
pub fn handle(payload: &str, environment: &Environment) -> Response {
    let request: Request = match serde_json::from_str(payload) {
        Ok(request) => request,
        Err(_) => {
            return protocol_error("the request could not be parsed");
        }
    };
    if request.version != PROTOCOL_VERSION {
        return protocol_error("the request uses an unsupported protocol version");
    }

    let project_root = request
        .project_root
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());

    let registry = match registry::build(environment, project_root.as_deref()) {
        RegistryOutcome::Ready(registry) => registry,
        RegistryOutcome::Malfunction(malfunction) => {
            return Response::Malfunction {
                version: PROTOCOL_VERSION,
                message: malfunction.message(),
            };
        }
    };

    let mut tally = registry.redactor.tally();
    let mut changed = false;
    let texts: Vec<String> = request
        .texts
        .iter()
        .map(|text| match registry.redactor.redact(text, &mut tally) {
            // Each string is matched independently (`RED-002`).
            Some(redacted) => {
                changed = true;
                redacted
            }
            None => text.clone(),
        })
        .collect();

    Response::Ok {
        version: PROTOCOL_VERSION,
        changed,
        texts,
        notification: registry
            .redactor
            .intervention(&tally)
            .map(|intervention| intervention.summary()),
        warnings: registry
            .warnings
            .iter()
            .map(|warning| warning.message().to_string())
            .collect(),
    }
}

fn protocol_error(message: &str) -> Response {
    Response::ProtocolError {
        version: PROTOCOL_VERSION,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Canary, assert_canary_absent};
    use serde_json::{Value, json};

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "contextveil-opencode-{}-{}",
                std::process::id(),
                Canary::generate("OPENCODE").token()
            ));
            std::fs::create_dir_all(root.join("contextveil")).expect("config directory");
            Self { root }
        }

        fn enroll(&self, name: &str) {
            std::fs::write(
                self.root.join("contextveil").join("config.toml"),
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

    fn request(event: &str, texts: Vec<String>) -> String {
        json!({
            "version": 1,
            "event": event,
            "project_root": "/absent/project",
            "texts": texts,
        })
        .to_string()
    }

    fn parsed(response: &Response) -> Value {
        serde_json::from_str(&response.to_json()).expect("valid JSON response")
    }

    #[test]
    fn user_text_is_redacted_and_reported() {
        let canary = Canary::generate("API_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("API_TOKEN");
        let environment = fixture.environment(&[("API_TOKEN", canary.value())]);

        let payload = request(
            "chat.message",
            vec![
                format!("here is my token {}", canary.value()),
                "and a clean part".to_string(),
            ],
        );
        let response = handle(&payload, &environment);
        assert_canary_absent("opencode response", response.to_json().as_bytes(), &canary);

        let value = parsed(&response);
        assert_eq!(value["status"], json!("ok"));
        assert_eq!(value["changed"], json!(true));
        assert_eq!(
            value["texts"][0],
            json!("here is my token <SECRET:API_TOKEN>")
        );
        assert_eq!(value["texts"][1], json!("and a clean part"));
        assert!(
            value["notification"]
                .as_str()
                .expect("a safe notification")
                .contains("API_TOKEN")
        );
    }

    #[test]
    fn tool_output_is_redacted_through_the_same_path() {
        let canary = Canary::generate("TOOL_SECRET");
        let fixture = Fixture::new();
        fixture.enroll("TOOL_SECRET");
        let environment = fixture.environment(&[("TOOL_SECRET", canary.value())]);

        let payload = request(
            "tool.execute.after",
            vec![format!("stdout: {}", canary.value())],
        );
        let value = parsed(&handle(&payload, &environment));
        assert_eq!(value["changed"], json!(true));
        assert_eq!(value["texts"][0], json!("stdout: <SECRET:TOOL_SECRET>"));
    }

    #[test]
    fn a_clean_request_reports_no_change_and_no_notification() {
        let canary = Canary::generate("API_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("API_TOKEN");
        let environment = fixture.environment(&[("API_TOKEN", canary.value())]);

        let payload = request("chat.message", vec!["nothing sensitive".to_string()]);
        let value = parsed(&handle(&payload, &environment));
        assert_eq!(value["changed"], json!(false));
        assert_eq!(value["texts"][0], json!("nothing sensitive"));
        assert!(value.get("notification").is_none());
        assert!(value.get("warnings").is_none());
    }

    #[test]
    fn an_unresolved_source_changes_nothing() {
        let fixture = Fixture::new();
        fixture.enroll("ABSENT_TOKEN");
        let environment = fixture.environment(&[]);
        let payload = request("chat.message", vec!["text".to_string()]);
        let value = parsed(&handle(&payload, &environment));
        assert_eq!(value["status"], json!("ok"));
        assert_eq!(value["changed"], json!(false));
    }

    #[test]
    fn a_malfunction_tells_the_plugin_to_abort() {
        // `RUN-003`: the plugin throws, aborting the covered operation.
        let canary = Canary::generate("API_TOKEN");
        let fixture = Fixture::new();
        std::fs::write(
            fixture.root.join("contextveil").join("config.toml"),
            "version = 1\n\n[[secret]]\nsource = \"nope\"\n",
        )
        .expect("write invalid config");
        let environment = fixture.environment(&[("API_TOKEN", canary.value())]);

        let payload = request("chat.message", vec![canary.value().to_string()]);
        let response = handle(&payload, &environment);
        assert_canary_absent("opencode response", response.to_json().as_bytes(), &canary);
        let value = parsed(&response);
        assert_eq!(value["status"], json!("malfunction"));
        assert!(
            value["message"]
                .as_str()
                .expect("message")
                .contains("doctor")
        );
        assert!(value.get("texts").is_none(), "no text is returned");
    }

    #[test]
    fn a_malformed_or_unknown_request_is_a_protocol_error() {
        // `RUN-006`: the plugin throws instead of silently continuing.
        let canary = Canary::generate("API_TOKEN");
        let fixture = Fixture::new();
        fixture.enroll("API_TOKEN");
        let environment = fixture.environment(&[("API_TOKEN", canary.value())]);

        for payload in [
            String::from("not json"),
            String::from("{version: 1, event: 'chat.message', texts: []}"),
            String::from("{}"),
            json!({"version": 2, "event": "chat.message", "texts": []}).to_string(),
            json!({"version": 1, "event": "session.idle", "texts": []}).to_string(),
            json!({"version": 1, "event": "chat.message", "texts": [], "extra": true}).to_string(),
            format!("{{\"version\":1,\"leak\":\"{}\"}}", canary.value()),
        ] {
            let response = handle(&payload, &environment);
            assert_canary_absent("opencode response", response.to_json().as_bytes(), &canary);
            let value = parsed(&response);
            assert_eq!(
                value["status"],
                json!("protocol-error"),
                "payload: {payload}"
            );
            assert!(value.get("texts").is_none());
        }
    }

    #[test]
    fn an_incomplete_global_setup_is_reported_as_a_warning() {
        let fixture = Fixture::new();
        let environment = fixture.environment(&[]);
        let payload = request("chat.message", vec!["text".to_string()]);
        let value = parsed(&handle(&payload, &environment));
        assert_eq!(value["status"], json!("ok"));
        assert!(
            value["warnings"][0]
                .as_str()
                .expect("a warning")
                .contains("setup is incomplete")
        );
    }

    #[test]
    fn text_order_is_preserved_so_the_plugin_can_write_parts_back() {
        let fixture = Fixture::new();
        fixture.enroll("TOKEN");
        let environment = fixture.environment(&[("TOKEN", "secret-value")]);
        let payload = request(
            "chat.message",
            vec![
                "first".to_string(),
                "secret-value".to_string(),
                "third".to_string(),
            ],
        );
        let value = parsed(&handle(&payload, &environment));
        assert_eq!(value["texts"][0], json!("first"));
        assert_eq!(value["texts"][1], json!("<SECRET:TOKEN>"));
        assert_eq!(value["texts"][2], json!("third"));
    }

    #[test]
    fn every_response_exits_zero_because_the_status_carries_the_outcome() {
        let fixture = Fixture::new();
        fixture.enroll("TOKEN");
        let environment = fixture.environment(&[]);
        for payload in [request("chat.message", vec![]), String::from("broken")] {
            assert_eq!(handle(&payload, &environment).exit(), Exit::Ok);
        }
    }
}
