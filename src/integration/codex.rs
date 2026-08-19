//! Codex CLI integration installation, inspection, and verification.
//!
//! Codex is experimental (`SUP-002`, `SUP-003`). `COD-001`: setup manages one
//! synchronous wildcard `PostToolUse` command hook in `~/.codex/hooks.json` with
//! a 5-second timeout and the host's required trust workflow.
//!
//! Verified against openai/codex (`codex-rs/hooks`, `codex-rs/config`): the file
//! is `{"hooks": {"PostToolUse": [ matcher groups ]}}`, `timeout` is seconds, an
//! omitted/empty/`*` matcher matches every tool, and a newly added or changed
//! hook stays `Untrusted` — and therefore does not run — until the user trusts
//! it. Codex also accepts hooks inline in `config.toml`; ContextVeil neither
//! writes nor inspects that form (`LIM-014`).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::integration::hooks_json::{self, Installed, Spec};
use crate::integration::state::{Managed, State};
use crate::integration::{
    self as integration, Harness, Inspection, InstallError, SYNTHETIC_PLACEHOLDER, SyntheticConfig,
    Verification, claude,
};

/// Hook timeout in seconds (`COD-001`, `RUN-004`).
pub const TIMEOUT_SECONDS: u64 = 5;

/// The managed artifact (`COD-001`).
pub const SPEC: Spec = Spec {
    event: "PostToolUse",
    arguments: "hook codex",
    timeout: TIMEOUT_SECONDS,
};

/// Path of the user hooks file (`COD-001`).
pub fn hooks_path(home: &Path) -> PathBuf {
    home.join(".codex").join("hooks.json")
}

/// Inspects detection, installation, and conflicts without changing anything.
pub fn inspect(
    environment: &crate::source::Environment,
    home: &Path,
    executable: Option<&Path>,
    state: &State,
) -> Inspection {
    let artifact_path = hooks_path(home);
    let current = executable.map(|path| hooks_json::managed_command(path, SPEC));
    let file = hooks_json::read(&artifact_path);
    let (installed, conflicts) = match &file {
        Err(problem) => (problem.clone(), Vec::new()),
        Ok(None) => (Installed::Absent, Vec::new()),
        Ok(Some(file)) => hooks_json::classify(
            file,
            SPEC,
            current.as_deref(),
            &state.approved_conflicts(Harness::Codex),
        ),
    };
    let entry = file
        .as_ref()
        .ok()
        .and_then(|file| file.as_ref())
        .and_then(|file| hooks_json::managed_entry(file, SPEC));

    Inspection {
        harness: Harness::Codex,
        artifact_path,
        detection: integration::detect(environment, home, "codex", ".codex"),
        installed,
        conflicts,
        hook_executable: entry
            .as_ref()
            .and_then(|(command, _)| hooks_json::command_executable(command, SPEC)),
        hook_timeout: entry.and_then(|(_, timeout)| timeout),
        // Codex has an administrator switch (`allow_managed_hooks_only`) that can
        // ignore user hooks, but it lives in a requirements file ContextVeil does
        // not read; nothing is claimed here rather than guessing.
        disabled_by_policy: false,
    }
}

/// Installs or updates the managed hook.
pub fn install(home: &Path, executable: &Path, state: &mut State) -> Result<(), InstallError> {
    hooks_json::install(&hooks_path(home), executable, SPEC)?;
    let approved = state.approved_conflicts(Harness::Codex);
    state.set(
        Harness::Codex,
        Some(Managed {
            command: hooks_json::managed_command(executable, SPEC),
            approved_conflicts: approved,
        }),
    );
    Ok(())
}

/// Removes the managed hook.
pub fn remove(home: &Path, state: &mut State) -> Result<bool, InstallError> {
    let removed = hooks_json::remove(&hooks_path(home), SPEC)?;
    state.set(Harness::Codex, None);
    Ok(removed)
}

/// Runs the installed binary against a synthetic `PostToolUse` payload.
///
/// Codex cannot replace a result in place, so the expected outcome is a block
/// decision whose sanitized reason carries the placeholder (`COD-002`).
pub fn verify_offline(executable: &Path) -> Verification {
    let Some(config) = SyntheticConfig::create("CODEX") else {
        return Verification::Failed("a temporary configuration could not be created");
    };
    let canary = integration::synthetic_canary("CODEX-VALUE");
    let payload = json!({
        "hook_event_name": "PostToolUse",
        "session_id": "synthetic",
        "cwd": "/nonexistent-synthetic-project",
        "tool_name": "shell",
        "tool_input": {"command": ["printenv", "CONTEXTVEIL_VERIFY"]},
        "tool_response": {"output": canary.clone(), "exit_code": 0},
        "tool_use_id": "synthetic",
    })
    .to_string();

    let Some(output) = claude::run_hook(executable, "hook codex", config.root(), &canary, &payload)
    else {
        return Verification::Failed("the configured executable did not answer in time");
    };
    let Ok(response) = serde_json::from_slice::<Value>(&output) else {
        return Verification::Failed("the hook did not return valid protocol output");
    };
    if response["decision"].as_str() != Some("block") {
        return Verification::Failed("the hook did not block the original result");
    }
    match response["reason"].as_str() {
        Some(reason) if reason.contains(SYNTHETIC_PLACEHOLDER) => Verification::Passed,
        _ => Verification::Failed("the sanitized replacement did not contain the placeholder"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Environment;
    use crate::testing::Canary;

    struct Home {
        root: PathBuf,
    }

    impl Home {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "contextveil-codex-integration-{}-{}",
                std::process::id(),
                Canary::generate("HOME").token()
            ));
            std::fs::create_dir_all(root.join(".codex")).expect("codex directory");
            Self { root }
        }

        fn executable(&self) -> PathBuf {
            let path = self.root.join("bin").join("contextveil");
            std::fs::create_dir_all(path.parent().expect("parent")).expect("bin directory");
            if !path.exists() {
                std::fs::write(&path, "#!/bin/sh\n").expect("write fake executable");
            }
            path
        }

        fn read(&self) -> String {
            std::fs::read_to_string(hooks_path(&self.root)).expect("read hooks file")
        }

        fn inspect(&self, state: &State) -> Inspection {
            super::inspect(
                &Environment::default(),
                &self.root,
                Some(&self.executable()),
                state,
            )
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn installation_writes_the_documented_codex_shape() {
        let home = Home::new();
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");

        let file: Value = serde_json::from_str(&home.read()).expect("valid JSON");
        let group = &file["hooks"]["PostToolUse"][0];
        assert_eq!(group["matcher"], json!("*"));
        assert_eq!(group["hooks"][0]["type"], json!("command"));
        // Codex expresses the timeout in seconds.
        assert_eq!(group["hooks"][0]["timeout"], json!(5));
        assert!(
            group["hooks"][0]["command"]
                .as_str()
                .expect("command")
                .ends_with(" hook codex")
        );
        assert_eq!(home.inspect(&state).installed, Installed::Current);
    }

    #[test]
    fn the_codex_and_claude_installers_never_claim_each_others_hooks() {
        let home = Home::new();
        std::fs::create_dir_all(home.root.join(".claude")).expect("claude directory");
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install codex");
        claude::install(&home.root, &home.executable(), &mut state).expect("install claude");

        // Different files, and each sees only its own entry.
        assert_eq!(home.inspect(&state).installed, Installed::Current);
        assert!(home.inspect(&state).conflicts.is_empty());
        let claude = claude::inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &state,
        );
        assert_eq!(claude.installed, Installed::Current);
        assert!(claude.conflicts.is_empty());

        assert!(remove(&home.root, &mut state).expect("remove codex"));
        assert!(state.get(Harness::Codex).is_none());
        assert!(state.get(Harness::Claude).is_some());
    }

    #[test]
    fn codex_is_experimental_and_carries_a_trust_note() {
        assert_eq!(Harness::Codex.tier_label(), "EXPERIMENTAL");
        let note = Harness::Codex.post_install_note().expect("a trust note");
        assert!(note.contains("Trust all and continue"));
    }

    #[test]
    fn unrelated_hooks_and_keys_survive_installation_and_removal() {
        let home = Home::new();
        std::fs::write(
            hooks_path(&home.root),
            r#"{"description": "mine", "hooks": {"PreToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/other/tool"}]}]}}"#,
        )
        .expect("write hooks file");

        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");
        assert!(remove(&home.root, &mut state).expect("remove"));

        let file: Value = serde_json::from_str(&home.read()).expect("valid JSON");
        assert_eq!(file["description"], json!("mine"));
        assert_eq!(
            file["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            json!("/other/tool")
        );
        assert!(file["hooks"].get("PostToolUse").is_none());
    }

    #[test]
    fn a_malformed_hooks_file_is_never_overwritten() {
        let home = Home::new();
        let malformed = "{ not json";
        std::fs::write(hooks_path(&home.root), malformed).expect("write hooks file");
        let mut state = State::default();
        assert_eq!(
            install(&home.root, &home.executable(), &mut state),
            Err(InstallError::Unreadable)
        );
        assert_eq!(home.read(), malformed);
        assert_eq!(home.inspect(&state).installed, Installed::Unreadable);
    }
}
