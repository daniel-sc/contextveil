//! Claude Code integration installation, inspection, and verification.
//!
//! `CLA-001`: setup manages one synchronous wildcard `PostToolUse` command hook
//! in `~/.claude/settings.json` with a 5-second timeout (`RUN-004`). The shared
//! JSON hooks editing lives in `crate::integration::hooks_json`; only the file
//! location, detection, policy check, and synthetic expectation are specific to
//! Claude.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::integration::hooks_json::{self, Installed, Spec};
use crate::integration::state::{Managed, State};
use crate::integration::{
    self as integration, Harness, Inspection, InstallError, SYNTHETIC_PLACEHOLDER,
    SYNTHETIC_VARIABLE, SyntheticConfig, Verification,
};

/// Hook timeout in seconds (`CLA-001`, `RUN-004`).
pub const TIMEOUT_SECONDS: u64 = 5;

/// The managed artifact (`CLA-001`).
pub const SPEC: Spec = Spec {
    event: "PostToolUse",
    arguments: "hook claude",
    timeout: TIMEOUT_SECONDS,
};

/// Path of the user settings file (`CLA-001`).
pub fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

/// Inspects detection, installation, and conflicts without changing anything.
pub fn inspect(
    environment: &crate::source::Environment,
    home: &Path,
    executable: Option<&Path>,
    state: &State,
) -> Inspection {
    let artifact_path = settings_path(home);
    let current = executable.map(|path| hooks_json::managed_command(path, SPEC));
    let file = hooks_json::read(&artifact_path);
    let (installed, conflicts) = match &file {
        Err(problem) => (problem.clone(), Vec::new()),
        Ok(None) => (Installed::Absent, Vec::new()),
        Ok(Some(file)) => hooks_json::classify(
            file,
            SPEC,
            current.as_deref(),
            &state.approved_conflicts(Harness::Claude),
        ),
    };
    let entry = file
        .as_ref()
        .ok()
        .and_then(|file| file.as_ref())
        .and_then(|file| hooks_json::managed_entry(file, SPEC));

    Inspection {
        harness: Harness::Claude,
        artifact_path,
        detection: integration::detect(environment, home, "claude", ".claude"),
        installed,
        conflicts,
        hook_executable: entry
            .as_ref()
            .and_then(|(command, _)| hooks_json::command_executable(command, SPEC)),
        hook_timeout: entry.and_then(|(_, timeout)| timeout),
        disabled_by_policy: hooks_disabled_by_policy(),
    }
}

/// Installs or updates the managed hook.
pub fn install(home: &Path, executable: &Path, state: &mut State) -> Result<(), InstallError> {
    hooks_json::install(&settings_path(home), executable, SPEC)?;
    let approved = state.approved_conflicts(Harness::Claude);
    state.set(
        Harness::Claude,
        Some(Managed {
            command: hooks_json::managed_command(executable, SPEC),
            approved_conflicts: approved,
        }),
    );
    Ok(())
}

/// Removes the managed hook.
pub fn remove(home: &Path, state: &mut State) -> Result<bool, InstallError> {
    let removed = hooks_json::remove(&settings_path(home), SPEC)?;
    state.set(Harness::Claude, None);
    Ok(removed)
}

/// True when a managed-policy file turns every hook off for this host.
///
/// Verified against Claude Code 2.1.233: policy settings carry a
/// `disableAllHooks` kill switch. The documented managed-settings locations are
/// checked for it, and both the top-level and nested spellings are accepted
/// because only the policy-settings shape is confirmed, not the file layout.
fn hooks_disabled_by_policy() -> bool {
    for path in [
        "/etc/claude-code/managed-settings.json",
        "/Library/Application Support/ClaudeCode/managed-settings.json",
    ] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let disabled = value.get("disableAllHooks").and_then(Value::as_bool) == Some(true)
            || value
                .get("policySettings")
                .and_then(|policy| policy.get("disableAllHooks"))
                .and_then(Value::as_bool)
                == Some(true);
        if disabled {
            return true;
        }
    }
    false
}

/// Runs the installed binary against a synthetic `PostToolUse` payload.
///
/// The check is offline and self-contained: it enrolls a generated
/// non-credential value through a temporary configuration, feeds it through the
/// real protocol path, and requires the value to be absent from the response
/// (`SEC-003`, `TST-005`).
pub fn verify_offline(executable: &Path) -> Verification {
    let Some(config) = SyntheticConfig::create("CLAUDE") else {
        return Verification::Failed("a temporary configuration could not be created");
    };
    let canary = integration::synthetic_canary("CLAUDE-VALUE");
    let payload = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "printenv SECRETSIEVE_VERIFY"},
        "tool_response": {"stdout": canary.clone(), "stderr": "", "interrupted": false},
    })
    .to_string();

    let Some(output) = run_hook(executable, "hook claude", config.root(), &canary, &payload) else {
        return Verification::Failed("the configured executable did not answer in time");
    };
    let Ok(response) = serde_json::from_slice::<Value>(&output) else {
        return Verification::Failed("the hook did not return valid protocol output");
    };
    match response["hookSpecificOutput"]["updatedToolOutput"]["stdout"].as_str() {
        Some(SYNTHETIC_PLACEHOLDER) => Verification::Passed,
        _ => Verification::Failed("the hook did not return the expected replacement"),
    }
}

/// Runs a hidden hook entry point with a synthetic payload.
///
/// Returns the captured stdout, or `None` when the value leaked, the process
/// failed, or it exceeded the host's 5-second bound (`RUN-004`).
pub(crate) fn run_hook(
    executable: &Path,
    arguments: &str,
    config_root: &Path,
    canary: &str,
    payload: &str,
) -> Option<Vec<u8>> {
    let mut command = Command::new(executable);
    for argument in arguments.split_whitespace() {
        command.arg(argument);
    }
    let mut child = command
        .env_clear()
        .env("XDG_CONFIG_HOME", config_root)
        .env(SYNTHETIC_VARIABLE, canary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(payload.as_bytes());
    }

    let deadline = Instant::now() + Duration::from_secs(TIMEOUT_SECONDS);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                return None;
            }
            Err(_) => return None,
        }
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let leaked = |bytes: &[u8]| bytes.windows(canary.len()).any(|w| w == canary.as_bytes());
    if leaked(&output.stdout) || leaked(&output.stderr) {
        return None;
    }
    Some(output.stdout)
}

/// What a live-canary reply shows about the covered path (`DIA-005`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanaryOutcome {
    /// The placeholder arrived and the generated value did not: it redacted.
    Redacted,
    /// Neither arrived, so the reply proves nothing about the covered path.
    Inconclusive,
    /// The generated value arrived: the covered path did not redact it.
    Disclosed,
}

/// Classifies a live-canary reply.
///
/// Absence of the generated value alone is not proof: a reply that declines the
/// request, or never runs the command, contains no value either. Requiring the
/// placeholder as well matches every offline synthetic check, which all demand
/// `SYNTHETIC_PLACEHOLDER` rather than mere absence, and keeps a request that
/// tested nothing from reading as a pass (`DIA-005`).
///
/// This is separated from the request so it is covered by tests without a paid,
/// networked call (`TST-008`, `DEV-001`).
pub fn classify_canary(reply: &[u8], canary: &str) -> CanaryOutcome {
    let contains = |needle: &str| {
        let needle = needle.as_bytes();
        !needle.is_empty() && reply.windows(needle.len()).any(|window| window == needle)
    };
    if contains(canary) {
        CanaryOutcome::Disclosed
    } else if contains(SYNTHETIC_PLACEHOLDER) {
        CanaryOutcome::Redacted
    } else {
        CanaryOutcome::Inconclusive
    }
}

/// Runs the optional paid, networked Claude canary (`DIA-005`).
///
/// It enrolls a conspicuous random non-credential value through a temporary
/// source configuration, asks Claude Code once to print that value with a
/// narrowly allowed `Bash` tool, and classifies the reply. It therefore tests
/// exactly one path: a successful `Bash` `PostToolUse` result flowing through
/// the installed hook.
///
/// This is the only network-capable workflow besides installation (`SEC-003`),
/// it is never enabled by default, and the request itself has no automated
/// coverage by design (`TST-008`, `DEV-001`).
pub fn live_canary(home: &Path) -> Result<CanaryOutcome, String> {
    let config =
        SyntheticConfig::create("LIVE").ok_or("a temporary configuration could not be created")?;
    let canary = integration::synthetic_canary("LIVE-VALUE");

    let prompt = format!(
        "Run exactly this shell command and reply with its output verbatim, nothing else: \
         printenv {SYNTHETIC_VARIABLE}"
    );
    let spawned = Command::new("claude")
        .args(["-p", &prompt, "--allowedTools", "Bash(printenv *)"])
        .current_dir(home)
        .env("XDG_CONFIG_HOME", config.root())
        .env(SYNTHETIC_VARIABLE, &canary)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = spawned.map_err(|_| "`claude` could not be run".to_string())?;

    // One model request can take a while; this bound is unrelated to the
    // 5-second hook timeout in `RUN-004`.
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Ok(None) => {
                let _ = child.kill();
                return Err("Claude did not answer within three minutes".to_string());
            }
            Err(_) => return Err("the Claude process could not be waited for".to_string()),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|_| "Claude's output could not be read".to_string())?;
    if !output.status.success() {
        return Err("Claude exited with a failure status".to_string());
    }
    Ok(classify_canary(&output.stdout, &canary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::Detection;
    use crate::source::Environment;
    use crate::testing::Canary;

    /// A reply carrying the placeholder and not the value is the only pass.
    #[test]
    fn a_live_canary_reply_passes_only_when_the_placeholder_arrived() {
        let value = "SSCANARY-LIVE-VALUE-1-0000000000000000";
        assert_eq!(
            classify_canary(
                format!("```\n{SYNTHETIC_PLACEHOLDER}\n```").as_bytes(),
                value
            ),
            CanaryOutcome::Redacted,
            "the host wraps a verbatim reply in a code fence, so this is a substring match"
        );
        assert_eq!(
            classify_canary(SYNTHETIC_PLACEHOLDER.as_bytes(), value),
            CanaryOutcome::Redacted
        );
    }

    /// Absence of the value alone proves nothing: a refusal contains no value
    /// either, and reporting that as a pass would be a false assurance.
    #[test]
    fn a_reply_that_never_ran_the_command_is_inconclusive_rather_than_a_pass() {
        let value = "SSCANARY-LIVE-VALUE-2-0000000000000000";
        for reply in [
            "I can't run that command.",
            "",
            "The environment variable is not set.",
            "printenv SECRETSIEVE_VERIFY",
        ] {
            assert_eq!(
                classify_canary(reply.as_bytes(), value),
                CanaryOutcome::Inconclusive,
                "`{reply}` exercised nothing"
            );
        }
    }

    #[test]
    fn a_reply_carrying_the_value_is_a_disclosure() {
        let value = "SSCANARY-LIVE-VALUE-3-0000000000000000";
        assert_eq!(
            classify_canary(format!("the value is {value}").as_bytes(), value),
            CanaryOutcome::Disclosed
        );
        // A disclosure outranks the placeholder: one replaced occurrence does
        // not excuse another that survived.
        assert_eq!(
            classify_canary(
                format!("{SYNTHETIC_PLACEHOLDER} and {value}").as_bytes(),
                value
            ),
            CanaryOutcome::Disclosed
        );
    }

    /// Invalid UTF-8 around the placeholder must not hide it, since the reply is
    /// classified as bytes.
    #[test]
    fn classification_reads_bytes_rather_than_text() {
        let value = "SSCANARY-LIVE-VALUE-4-0000000000000000";
        let mut reply = vec![0xff, 0xfe];
        reply.extend_from_slice(SYNTHETIC_PLACEHOLDER.as_bytes());
        reply.push(0x80);
        assert_eq!(classify_canary(&reply, value), CanaryOutcome::Redacted);
    }

    /// An empty needle would otherwise match everything and turn any reply into
    /// a disclosure.
    #[test]
    fn an_empty_value_cannot_match_every_reply() {
        assert_eq!(
            classify_canary(SYNTHETIC_PLACEHOLDER.as_bytes(), ""),
            CanaryOutcome::Redacted
        );
        assert_eq!(
            classify_canary(b"anything", ""),
            CanaryOutcome::Inconclusive
        );
    }

    struct Home {
        root: PathBuf,
    }

    impl Home {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "secretsieve-claude-integration-{}-{}",
                std::process::id(),
                Canary::generate("HOME").token()
            ));
            std::fs::create_dir_all(root.join(".claude")).expect("claude directory");
            Self { root }
        }

        fn executable(&self) -> PathBuf {
            let path = self.root.join("bin").join("secretsieve");
            std::fs::create_dir_all(path.parent().expect("parent")).expect("bin directory");
            if !path.exists() {
                std::fs::write(&path, "#!/bin/sh\n").expect("write fake executable");
            }
            path
        }

        fn settings(&self) -> PathBuf {
            settings_path(&self.root)
        }

        fn write_settings(&self, contents: &str) {
            std::fs::write(self.settings(), contents).expect("write settings");
        }

        fn read_settings(&self) -> String {
            std::fs::read_to_string(self.settings()).expect("read settings")
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
    fn a_clean_installation_creates_the_managed_hook() {
        let home = Home::new();
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");

        let settings: Value = serde_json::from_str(&home.read_settings()).expect("valid JSON");
        let groups = settings["hooks"]["PostToolUse"]
            .as_array()
            .expect("PostToolUse array");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"][0]["timeout"], json!(5));
        assert_eq!(
            state.get(Harness::Claude).expect("state").command,
            hooks_json::managed_command(&home.executable(), SPEC)
        );
        assert_eq!(home.inspect(&state).installed, Installed::Current);
    }

    #[test]
    fn unrelated_settings_are_preserved() {
        let home = Home::new();
        home.write_settings(
            r#"{"model": "opus", "env": {"FOO": "bar"}, "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/other/tool"}]}]}}"#,
        );
        install(&home.root, &home.executable(), &mut State::default()).expect("install");

        let settings: Value = serde_json::from_str(&home.read_settings()).expect("valid JSON");
        assert_eq!(settings["model"], json!("opus"));
        assert_eq!(settings["env"]["FOO"], json!("bar"));
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            json!("/other/tool")
        );
    }

    #[test]
    fn removal_by_deselection_removes_only_the_managed_entry() {
        let home = Home::new();
        home.write_settings(
            r#"{"hooks": {"PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/other/tool"}]}]}}"#,
        );
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");
        assert!(remove(&home.root, &mut state).expect("remove"));

        let settings: Value = serde_json::from_str(&home.read_settings()).expect("valid JSON");
        let groups = settings["hooks"]["PostToolUse"].as_array().expect("array");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"][0]["command"], json!("/other/tool"));
        assert!(state.get(Harness::Claude).is_none());
    }

    #[test]
    fn malformed_settings_are_never_overwritten() {
        let home = Home::new();
        let malformed = "{ this is not json";
        home.write_settings(malformed);
        let mut state = State::default();

        assert_eq!(
            install(&home.root, &home.executable(), &mut state),
            Err(InstallError::Unreadable)
        );
        assert_eq!(home.read_settings(), malformed);
        assert_eq!(
            remove(&home.root, &mut state),
            Err(InstallError::Unreadable)
        );
        assert_eq!(home.read_settings(), malformed);
        assert_eq!(home.inspect(&state).installed, Installed::Unreadable);
    }

    #[test]
    fn other_post_tool_use_command_hooks_are_reported_as_conflicts() {
        let home = Home::new();
        home.write_settings(
            r#"{"hooks": {"PostToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/other/mutator --rewrite"}]}]}}"#,
        );
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");

        let inspection = home.inspect(&state);
        assert_eq!(inspection.installed, Installed::Current);
        assert_eq!(inspection.conflicts.len(), 1);
        assert_eq!(inspection.conflicts[0].command, "/other/mutator --rewrite");
        assert!(!inspection.conflicts[0].approved);

        integration::approve_conflict(Harness::Claude, &mut state, "/other/mutator --rewrite");
        assert!(home.inspect(&state).conflicts[0].approved);
        // Approvals survive a reinstall (`INT-005`).
        install(&home.root, &home.executable(), &mut state).expect("reinstall");
        assert!(home.inspect(&state).conflicts[0].approved);
    }

    #[test]
    fn detection_uses_the_executable_or_the_configuration_directory() {
        let home = Home::new();
        assert_eq!(
            home.inspect(&State::default()).detection,
            Detection::Detected
        );

        let elsewhere = home.root.join("no-claude-here");
        std::fs::create_dir_all(&elsewhere).expect("directory");
        let undetected =
            super::inspect(&Environment::default(), &elsewhere, None, &State::default());
        assert_eq!(undetected.detection, Detection::NotDetected);
    }

    #[test]
    fn the_hook_executable_and_timeout_are_exposed_for_doctor() {
        let home = Home::new();
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");
        let inspection = home.inspect(&state);
        assert_eq!(inspection.hook_executable, Some(home.executable()));
        assert_eq!(inspection.hook_timeout, Some(5));
        assert_eq!(inspection.artifact_path, home.settings());
    }
}
