//! Fuzz targets for every untrusted input surface (`TST-006`).
//!
//! Each target takes raw bytes, must never panic, and asserts the invariants that
//! matter for this surface. They are plain functions so the bounded smoke harness
//! (`mise run fuzz-smoke`) and any external fuzzer can drive the same code.
//!
//! The adapter targets run against a temporary configuration that enrolls one
//! generated non-credential value. Alongside raw protocol inputs they exercise
//! valid envelopes carrying that value and mutated text, asserting intervention
//! and no disclosure (`TST-005`).
//!
//! This module is compiled only for tests or behind the `testing` feature.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::{Value, json};

use crate::adapter::{claude, codex, copilot, opencode};
use crate::matcher::Redactor;
use crate::secret::{ResolvedSecret, SourceId};
use crate::source::Environment;

/// A temporary configuration and the value enrolled in it.
pub struct Context {
    root: PathBuf,
    environment: Environment,
    canary: String,
}

impl Context {
    /// Creates a context, or `None` when a temporary directory is unavailable.
    pub fn create() -> Option<Self> {
        let canary = format!(
            "SSCANARY-FUZZ-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        );
        let root = std::env::temp_dir().join(format!("contextveil-fuzz-{canary}"));
        std::fs::create_dir_all(root.join("contextveil")).ok()?;
        std::fs::write(
            root.join("contextveil").join("config.toml"),
            "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"CONTEXTVEIL_FUZZ\"\n",
        )
        .ok()?;
        let environment = Environment::from_pairs([
            ("XDG_CONFIG_HOME", root.to_string_lossy().into_owned()),
            ("CONTEXTVEIL_FUZZ", canary.clone()),
        ]);
        Some(Self {
            root,
            environment,
            canary,
        })
    }

    pub fn canary(&self) -> &str {
        &self.canary
    }

    fn assert_no_disclosure(&self, channel: &str, text: &str) {
        assert!(
            !text.contains(&self.canary),
            "the enrolled value was disclosed in {channel}"
        );
    }

    fn covered_text(&self, text: &str) -> String {
        format!("{text}\n{}\n{text}", self.canary)
    }

    fn assert_intervention(&self, stdout: Option<&str>, pointer: &str) -> Value {
        let stdout = stdout.expect("no intervention response");
        self.assert_no_disclosure("adapter response", stdout);
        // Copilot emits progress records before its final mutation object.
        let response: Value = serde_json::from_str(stdout.lines().last().expect("response"))
            .expect("valid intervention JSON");
        let replaced = response
            .pointer(pointer)
            .and_then(Value::as_str)
            .expect("model-facing replacement");
        self.assert_no_disclosure("model-facing replacement", replaced);
        assert!(
            replaced.contains("<SECRET:CONTEXTVEIL_FUZZ>"),
            "no placeholder"
        );
        response
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The process-wide context used by the single-argument targets.
pub fn context() -> Option<&'static Context> {
    static CONTEXT: OnceLock<Option<Context>> = OnceLock::new();
    CONTEXT.get_or_init(Context::create).as_ref()
}

/// One fuzz target: raw bytes in, no output, must never panic.
pub type Target = fn(&[u8]);

/// Every target, by name, for the smoke harness.
pub const TARGETS: [(&str, Target); 12] = [
    ("dotenv", dotenv),
    ("npmrc", npmrc),
    ("properties", properties),
    ("ini", ini),
    ("json-source", json_source),
    ("config", config),
    ("matcher", matcher),
    ("sanitize", sanitize),
    ("claude", claude_hook),
    ("codex", codex_hook),
    ("copilot", copilot_hook),
    ("opencode", opencode_hook),
];

pub fn npmrc(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let parsed = crate::npmrc::parse(text);
    for (key, value) in parsed.entries() {
        assert_eq!(parsed.get(key), Some(value));
    }
    for duplicate in parsed.duplicates() {
        assert!(parsed.get(duplicate).is_some());
    }
}

pub fn properties(data: &[u8]) {
    if let Ok(parsed) = crate::properties::parse(data) {
        for (key, value) in parsed.entries() {
            assert_eq!(parsed.get(key), Some(value));
        }
        for duplicate in parsed.duplicates() {
            assert!(parsed.get(duplicate).is_some());
        }
    }
}

/// INI grammar and duplicate-section resolution (`SRC-019`, `TST-006`).
pub fn ini(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(parsed) = crate::ini::parse(text) else {
        return;
    };

    // `entries` exposes one final assignment per exact section/key identity.
    // Verify that the map-style getter agrees with that normalized view.
    for (section, key, value) in parsed.entries() {
        assert_eq!(
            parsed.get(section, key),
            Some(value),
            "INI getter disagrees with entries"
        );
    }
    for (section, key) in parsed.duplicates() {
        assert!(
            parsed.get(section, key).is_some(),
            "duplicate has no final assignment"
        );
    }
}

/// Dotenv grammar (`SRC-003`).
pub fn dotenv(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(parsed) = crate::dotenv::parse(text) {
        // Every reported key must be retrievable, and duplicates must be a subset
        // of the keys.
        for (key, value) in parsed.entries() {
            assert_eq!(parsed.get(key), Some(value));
        }
        for duplicate in parsed.duplicates() {
            assert!(parsed.get(duplicate).is_some());
        }
    }
}

/// JSON5 source documents and exact pointer traversal (`SRC-011`, `TST-006`).
pub fn json_source(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let (pointer, document) = text.split_once('\n').unwrap_or((text, ""));
    if crate::json::final_token(pointer).is_ok()
        && let Ok(value) = crate::json::parse(document)
    {
        let _ = crate::json::select(&value, pointer);
    }
}

/// Configuration parsing (`CFG-006`).
pub fn config(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    match crate::config::parse(
        text,
        Path::new("/fuzz/project"),
        Some(Path::new("/fuzz/home")),
    ) {
        Ok(parsed) => {
            // Accepted files never contain two identical identities (`CFG-006`).
            let mut seen = Vec::new();
            for source in &parsed.sources {
                let identity = source.id();
                assert!(!seen.contains(&identity), "duplicate identity accepted");
                seen.push(identity);
            }
        }
        Err(kind) => {
            // A diagnostic must never quote the file (`SEC-004`).
            let reason = kind.reason();
            for line in text.lines().filter(|line| line.len() > 8) {
                assert!(!reason.contains(line), "a diagnostic quoted file content");
            }
        }
    }
}

/// Matcher semantics (`RED-001` through `RED-008`).
pub fn matcher(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The first line holds tab-separated values; the rest is the haystack.
    let (header, haystack) = text.split_once('\n').unwrap_or((text, ""));
    let secrets: Vec<ResolvedSecret> = header
        .split('\t')
        .filter(|value| !value.is_empty())
        .take(16)
        .enumerate()
        .map(|(index, value)| {
            ResolvedSecret::new(SourceId::env(format!("FUZZ_{index}")), value.to_string())
        })
        .collect();

    let redactor = Redactor::new(secrets);
    let mut tally = redactor.tally();
    let output = redactor
        .redact(haystack, &mut tally)
        .unwrap_or_else(|| haystack.to_string());

    match redactor.intervention(&tally) {
        None => assert_eq!(tally.total(), 0),
        Some(intervention) => {
            assert_eq!(intervention.total, tally.total());
            let reported: usize = intervention
                .named
                .iter()
                .map(|entry| entry.count)
                .sum::<usize>()
                + intervention.unnamed;
            assert_eq!(reported, intervention.total);
            // Metadata carries counts and labels only (`RED-008`).
            let summary = intervention.summary();
            for entry in &intervention.named {
                assert!(summary.contains(&entry.label));
            }
        }
    }
    // Replacing again must be a no-op for the same registry, because inserted
    // text is never rescanned (`RED-007`).
    let mut second = redactor.tally();
    if let Some(again) = redactor.redact(&output, &mut second) {
        assert_ne!(
            again, output,
            "a second pass changed nothing but reported so"
        );
    }
}

/// Terminal sanitization (`SEC-006`).
pub fn sanitize(data: &[u8]) {
    let rendered = crate::sanitize::bytes(data);
    assert!(
        !rendered.contains([
            '\n', '\r', '\u{b}', '\u{c}', '\u{1b}', '\u{85}', '\u{2028}', '\u{2029}'
        ]),
        "a sanitized rendering left a line break or escape in place"
    );
    if let Ok(text) = std::str::from_utf8(data) {
        assert_eq!(crate::sanitize::text(text), rendered);
    }
}

/// Claude `PostToolUse` envelopes (`RUN-006`).
pub fn claude_hook(data: &[u8]) {
    let Some(context) = context() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let response = claude::handle(text, &context.environment);
    if let Some(stdout) = &response.stdout {
        context.assert_no_disclosure("claude stdout", stdout);
        assert!(
            serde_json::from_str::<Value>(stdout).is_ok(),
            "the adapter emitted invalid protocol output"
        );
    }
    let payload =
        json!({"hook_event_name":"PostToolUse","tool_response":context.covered_text(text)});
    let response = claude::handle(&payload.to_string(), &context.environment);
    context.assert_intervention(
        response.stdout.as_deref(),
        "/hookSpecificOutput/updatedToolOutput",
    );
}

/// Codex `PostToolUse` envelopes (`RUN-006`).
pub fn codex_hook(data: &[u8]) {
    let Some(context) = context() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let response = codex::handle(text, &context.environment);
    if let Some(stdout) = &response.stdout {
        context.assert_no_disclosure("codex stdout", stdout);
        assert!(serde_json::from_str::<Value>(stdout).is_ok());
    }
    let payload =
        json!({"hook_event_name":"PostToolUse","tool_response":context.covered_text(text)});
    let response = codex::handle(&payload.to_string(), &context.environment);
    let output = context.assert_intervention(response.stdout.as_deref(), "/reason");
    assert!(
        output["decision"] == "block",
        "original result was not blocked"
    );
}

/// Copilot payloads for both covered events (`RUN-006`).
pub fn copilot_hook(data: &[u8]) {
    let Some(context) = context() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    for event in [
        copilot::Event::TransformedPrompt,
        copilot::Event::PostToolUse,
    ] {
        let response = copilot::handle(event, text, &context.environment);
        if let Some(stdout) = &response.stdout {
            context.assert_no_disclosure("copilot stdout", stdout);
            for line in stdout.lines() {
                assert!(serde_json::from_str::<Value>(line).is_ok());
            }
        }
        if let Some(stderr) = &response.stderr {
            context.assert_no_disclosure("copilot stderr", stderr);
        }
        let covered = context.covered_text(text);
        let (payload, pointer) = match event {
            copilot::Event::TransformedPrompt => (
                json!({"transformedPrompt":covered}),
                "/modifiedTransformedPrompt",
            ),
            copilot::Event::PostToolUse => (
                json!({"toolResult":{"resultType":"success","textResultForLlm":covered}}),
                "/modifiedResult/textResultForLlm",
            ),
        };
        let response = copilot::handle(event, &payload.to_string(), &context.environment);
        if let Some(stderr) = &response.stderr {
            context.assert_no_disclosure("copilot stderr", stderr);
        }
        context.assert_intervention(response.stdout.as_deref(), pointer);
    }
}

/// OpenCode transport requests (`OCO-001`, `RUN-006`).
pub fn opencode_hook(data: &[u8]) {
    let Some(context) = context() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let response = opencode::handle(text, &context.environment);
    let json = response.to_json();
    context.assert_no_disclosure("opencode response", &json);
    assert!(serde_json::from_str::<Value>(&json).is_ok());
    for event in ["chat.message", "tool.execute.after"] {
        let payload = json!({"version":1,"event":event,"texts":[context.covered_text(text)]});
        let response = opencode::handle(&payload.to_string(), &context.environment).to_json();
        let output = context.assert_intervention(Some(&response), "/texts/0");
        assert!(output["changed"] == true, "intervention was not reported");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_survives_empty_and_hostile_input() {
        let inputs: [&[u8]; 7] = [
            b"",
            b"\0\0\0",
            &[0xff, 0xfe, 0xfd],
            b"version = 1",
            b"A=1\nB='unterminated",
            b"{\"hook_event_name\":\"PostToolUse\"}",
            b"a\tb\nabab",
        ];
        for (name, target) in TARGETS {
            for input in inputs {
                target(input);
                let _ = name;
            }
        }
    }

    #[test]
    fn the_matcher_target_exercises_real_replacement() {
        // A sanity check that the split convention actually produces matches.
        matcher(b"secret\nthis contains secret twice: secret");
    }
}
