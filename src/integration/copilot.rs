//! GitHub Copilot CLI integration installation, inspection, and verification.
//!
//! Copilot is experimental (`SUP-002`, `SUP-003`). `COP-001`: setup manages a
//! dedicated SecretSieve hook file under `~/.copilot/hooks/` with a 5-second
//! timeout and never modifies an unrelated hook file.
//!
//! Verified against the GitHub Copilot CLI hooks reference (CLI 1.0.80): every
//! `*.json` file in that directory is loaded and merged, a file is
//! `{"version": 1, "hooks": {"<event>": [ handlers ]}}` where each handler is a
//! flat object with `type`, `bash`, and a seconds-valued `timeoutSec`, the events
//! are camelCase, and a structurally invalid file is rejected as a whole. Copilot
//! also merges repository hooks and inline `settings.json` hooks, which
//! SecretSieve neither writes nor inspects (`LIM-015`).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::integration::hooks_json::{Conflict, Installed};
use crate::integration::state::{Managed, State};
use crate::integration::{
    self as integration, Harness, Inspection, InstallError, SYNTHETIC_PLACEHOLDER, SyntheticConfig,
    Verification, claude, shell_quote,
};
use crate::sanitize;

/// Hook timeout in seconds (`COP-001`, `RUN-004`).
pub const TIMEOUT_SECONDS: u64 = 5;

/// The dedicated file SecretSieve owns (`COP-001`).
pub const FILENAME: &str = "secretsieve.json";

/// Covered events and the hidden arguments that serve them (`COP-002`).
const EVENTS: [(&str, &str); 2] = [
    ("userPromptTransformed", "hook copilot prompt"),
    ("postToolUse", "hook copilot tool"),
];

/// Directory holding Copilot's user hook files.
pub fn hooks_directory(home: &Path) -> PathBuf {
    home.join(".copilot").join("hooks")
}

/// Path of the dedicated SecretSieve hook file.
pub fn hook_file(home: &Path) -> PathBuf {
    hooks_directory(home).join(FILENAME)
}

/// The file contents SecretSieve installs for `executable`.
fn managed_file(executable: &Path) -> Value {
    let quoted = shell_quote(&executable.to_string_lossy());
    let mut hooks = Map::new();
    for (event, arguments) in EVENTS {
        hooks.insert(
            event.to_string(),
            json!([{
                "type": "command",
                "bash": format!("{quoted} {arguments}"),
                "timeoutSec": TIMEOUT_SECONDS,
            }]),
        );
    }
    json!({"version": 1, "hooks": Value::Object(hooks)})
}

fn render(executable: &Path) -> String {
    let mut rendered =
        serde_json::to_string_pretty(&managed_file(executable)).unwrap_or_else(|_| String::new());
    rendered.push('\n');
    rendered
}

/// True when `command` is a SecretSieve Copilot hook command.
fn is_managed_command(command: &str) -> bool {
    EVENTS.iter().any(|(_, arguments)| {
        command
            .strip_suffix(&format!(" {arguments}"))
            .map(|path| {
                let path = path.trim_matches('\'');
                !path.is_empty() && Path::new(path).is_absolute()
            })
            .unwrap_or(false)
    })
}

/// Inspects detection, installation, and conflicts without changing anything.
pub fn inspect(
    environment: &crate::source::Environment,
    home: &Path,
    executable: Option<&Path>,
    state: &State,
) -> Inspection {
    let artifact_path = hook_file(home);
    let installed = classify(&artifact_path, executable);
    let (hook_executable, hook_timeout) = managed_entry(&artifact_path);

    Inspection {
        harness: Harness::Copilot,
        artifact_path,
        detection: integration::detect(environment, home, "copilot", ".copilot"),
        installed,
        conflicts: conflicts(home, &state.approved_conflicts(Harness::Copilot)),
        hook_executable,
        hook_timeout,
        // Copilot has no documented host-level hook kill switch to inspect.
        disabled_by_policy: false,
    }
}

/// Classifies the dedicated file (`INT-004`).
///
/// `Outdated` means only the embedded binary path differs, which SecretSieve may
/// rewrite. Any other difference, such as a hand-edited timeout, means the file
/// was edited and is preserved rather than reverted or deleted.
fn classify(path: &Path, executable: Option<&Path>) -> Installed {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Installed::Absent,
        Err(_) => return Installed::Unreadable,
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Installed::Unreadable;
    };
    let commands = event_commands(&value);
    let modified = || Installed::Modified {
        command: sanitize::text(&commands.join(", ")),
    };
    if commands.is_empty() || !commands.iter().all(|command| is_managed_command(command)) {
        // The dedicated name is SecretSieve's, but this content is not ours.
        return modified();
    }
    if executable.is_some_and(|executable| text == render(executable)) {
        return Installed::Current;
    }
    // Rebuild the file from the path it records; only an exact match means the
    // binary path is the sole difference.
    match recorded_executable_in(&value) {
        Some(recorded) if text == render(&recorded) => Installed::Outdated {
            command: sanitize::text(&commands.join(", ")),
        },
        _ => modified(),
    }
}

/// The binary path embedded in a managed command.
fn recorded_executable_in(value: &Value) -> Option<PathBuf> {
    event_commands(value).into_iter().find_map(|command| {
        EVENTS.iter().find_map(|(_, arguments)| {
            command
                .strip_suffix(&format!(" {arguments}"))
                .map(|path| PathBuf::from(path.trim_matches('\'')))
        })
    })
}

/// The executable and timeout recorded in the dedicated file.
fn managed_entry(path: &Path) -> (Option<PathBuf>, Option<u64>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return (None, None);
    };
    let executable = recorded_executable_in(&value);
    let timeout = value
        .get("hooks")
        .and_then(Value::as_object)
        .and_then(|hooks| {
            hooks.values().find_map(|handlers| {
                handlers
                    .as_array()?
                    .first()?
                    .get("timeoutSec")
                    .and_then(Value::as_u64)
            })
        });
    (executable, timeout)
}

/// Every command string declared for a covered event in one file.
fn event_commands(value: &Value) -> Vec<String> {
    let Some(hooks) = value.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut commands = Vec::new();
    for (event, _) in EVENTS {
        let Some(handlers) = hooks.get(event).and_then(Value::as_array) else {
            continue;
        };
        for handler in handlers {
            if handler.get("type").and_then(Value::as_str) != Some("command") {
                continue;
            }
            // `bash` is the Unix command field; `powershell` is Windows-only and
            // V1 does not support Windows (`SUP-001`).
            if let Some(command) = handler.get("bash").and_then(Value::as_str) {
                commands.push(command.to_string());
            }
        }
    }
    commands
}

/// Other hook files that also act on a covered event (`INT-005`, `LIM-017`).
fn conflicts(home: &Path, approved: &[String]) -> Vec<Conflict> {
    let Ok(entries) = std::fs::read_dir(hooks_directory(home)) else {
        return Vec::new();
    };
    let mut conflicts = Vec::new();
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            // Only real files are read. A symlink or a special file in this
            // directory must not be followed: a FIFO or a character device would
            // block the read, and a symlink could point anywhere.
            let is_regular = std::fs::symlink_metadata(path)
                .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                .unwrap_or(false);
            is_regular
                && path.file_name().is_some_and(|name| name != FILENAME)
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();

    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        for command in event_commands(&value) {
            conflicts.push(Conflict {
                approved: approved.contains(&command),
                command: sanitize::text(&command),
            });
        }
    }
    conflicts
}

/// Installs or updates the dedicated hook file.
pub fn install(home: &Path, executable: &Path, state: &mut State) -> Result<(), InstallError> {
    let path = hook_file(home);
    if matches!(
        classify(&path, Some(executable)),
        Installed::Modified { .. }
    ) {
        // `INT-004`: a file whose content is not ours stays untouched.
        return Err(InstallError::Unexpected);
    }
    std::fs::create_dir_all(hooks_directory(home)).map_err(|_| InstallError::Write)?;
    crate::setup::write::write_text(&path, &render(executable), false)
        .map_err(|_| InstallError::Write)?;

    let approved = state.approved_conflicts(Harness::Copilot);
    state.set(
        Harness::Copilot,
        Some(Managed {
            command: path.to_string_lossy().into_owned(),
            approved_conflicts: approved,
        }),
    );
    Ok(())
}

/// Removes the dedicated hook file.
///
/// Returns `Ok(false)` when the file was preserved because its content is not
/// SecretSieve's (`INT-004`). Unrelated hook files are never touched
/// (`COP-001`).
pub fn remove(home: &Path, state: &mut State) -> Result<bool, InstallError> {
    let path = hook_file(home);
    let removable = match classify(&path, None) {
        Installed::Absent => true,
        Installed::Modified { .. } | Installed::Unreadable | Installed::Unexpected => false,
        Installed::Current | Installed::Outdated { .. } => {
            std::fs::remove_file(&path).map_err(|_| InstallError::Write)?;
            true
        }
    };
    state.set(Harness::Copilot, None);
    Ok(removable)
}

/// Runs the installed binary against synthetic payloads for both events.
pub fn verify_offline(executable: &Path) -> Verification {
    let Some(config) = SyntheticConfig::create("COPILOT") else {
        return Verification::Failed("a temporary configuration could not be created");
    };

    let canary = integration::synthetic_canary("COPILOT-PROMPT");
    let prompt = json!({
        "sessionId": "synthetic",
        "timestamp": 0,
        "cwd": "/nonexistent-synthetic-project",
        "prompt": format!("use {canary}"),
        "transformedPrompt": format!("use {canary}"),
    })
    .to_string();
    let Some(output) = claude::run_hook(
        executable,
        "hook copilot prompt",
        config.root(),
        &canary,
        &prompt,
    ) else {
        return Verification::Failed("the configured executable did not answer in time");
    };
    let Ok(response) = parse_last_object(&output) else {
        return Verification::Failed("the prompt hook did not return valid protocol output");
    };
    match response["modifiedTransformedPrompt"].as_str() {
        Some(text) if text.contains(SYNTHETIC_PLACEHOLDER) => {}
        _ => return Verification::Failed("the prompt hook did not return a redacted prompt"),
    }

    let canary = integration::synthetic_canary("COPILOT-TOOL");
    let tool = json!({
        "sessionId": "synthetic",
        "timestamp": 0,
        "cwd": "/nonexistent-synthetic-project",
        "toolName": "shell",
        "toolArgs": {"command": "printenv SECRETSIEVE_VERIFY"},
        "toolResult": {"resultType": "success", "textResultForLlm": canary.clone()},
    })
    .to_string();
    let Some(output) = claude::run_hook(
        executable,
        "hook copilot tool",
        config.root(),
        &canary,
        &tool,
    ) else {
        return Verification::Failed("the configured executable did not answer in time");
    };
    let Ok(response) = parse_last_object(&output) else {
        return Verification::Failed("the tool hook did not return valid protocol output");
    };
    match response["modifiedResult"]["textResultForLlm"].as_str() {
        Some(SYNTHETIC_PLACEHOLDER) => Verification::Passed,
        _ => Verification::Failed("the tool hook did not return a redacted result"),
    }
}

/// Parses the final JSON object, skipping progress lines (`COP-003`).
fn parse_last_object(output: &[u8]) -> Result<Value, ()> {
    let text = std::str::from_utf8(output).map_err(|_| ())?;
    let body: String = text
        .lines()
        .filter(|line| {
            serde_json::from_str::<Value>(line.trim())
                .ok()
                .and_then(|value| {
                    value
                        .get("type")
                        .and_then(Value::as_str)
                        .map(|kind| kind == "progress")
                })
                != Some(true)
        })
        .collect::<Vec<&str>>()
        .join("\n");
    serde_json::from_str(body.trim()).map_err(|_| ())
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
                "secretsieve-copilot-integration-{}-{}",
                std::process::id(),
                Canary::generate("HOME").token()
            ));
            std::fs::create_dir_all(hooks_directory(&root)).expect("hooks directory");
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
    fn installation_writes_one_dedicated_file_for_both_events() {
        let home = Home::new();
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");

        let file: Value = serde_json::from_str(
            &std::fs::read_to_string(hook_file(&home.root)).expect("read hook file"),
        )
        .expect("valid JSON");
        assert_eq!(file["version"], json!(1));
        for event in ["userPromptTransformed", "postToolUse"] {
            let handler = &file["hooks"][event][0];
            assert_eq!(handler["type"], json!("command"));
            assert_eq!(handler["timeoutSec"], json!(5));
            assert!(
                handler["bash"]
                    .as_str()
                    .expect("bash")
                    .contains(" hook copilot ")
            );
        }
        assert_eq!(home.inspect(&state).installed, Installed::Current);
    }

    #[test]
    fn unrelated_hook_files_are_never_touched() {
        // `COP-001`: only the dedicated file is managed.
        let home = Home::new();
        let other = hooks_directory(&home.root).join("team-policy.json");
        let contents = r#"{"version": 1, "hooks": {"postToolUse": [{"type": "command", "bash": "/other/tool"}]}}"#;
        std::fs::write(&other, contents).expect("write other hook file");

        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");
        assert!(remove(&home.root, &mut state).expect("remove"));

        assert_eq!(
            std::fs::read_to_string(&other).expect("read other hook file"),
            contents
        );
        assert!(!hook_file(&home.root).exists());
    }

    #[test]
    fn other_hook_files_on_covered_events_become_conflicts() {
        let home = Home::new();
        std::fs::write(
            hooks_directory(&home.root).join("team-policy.json"),
            r#"{"version": 1, "hooks": {"postToolUse": [{"type": "command", "bash": "/other/mutator"}]}}"#,
        )
        .expect("write other hook file");
        // A hook on an uncovered event is not a conflict.
        std::fs::write(
            hooks_directory(&home.root).join("logger.json"),
            r#"{"version": 1, "hooks": {"sessionStart": [{"type": "command", "bash": "/other/logger"}]}}"#,
        )
        .expect("write logger hook file");

        let mut state = State::default();
        let inspection = home.inspect(&state);
        assert_eq!(inspection.conflicts.len(), 1);
        assert_eq!(inspection.conflicts[0].command, "/other/mutator");
        assert!(!inspection.conflicts[0].approved);

        integration::approve_conflict(Harness::Copilot, &mut state, "/other/mutator");
        assert!(home.inspect(&state).conflicts[0].approved);
    }

    #[test]
    #[cfg(unix)]
    fn the_conflict_scan_never_follows_a_symlink_or_reads_a_special_file() {
        let home = Home::new();
        let hooks = hooks_directory(&home.root);
        // A FIFO would block a read forever, and a symlink could point anywhere.
        let fifo = hooks.join("blocking.json");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .expect("mkfifo runs")
                .success()
        );
        std::os::unix::fs::symlink("/etc/passwd", hooks.join("linked.json")).expect("symlink");
        std::fs::write(
            hooks.join("real.json"),
            r#"{"version": 1, "hooks": {"postToolUse": [{"type": "command", "bash": "/other/tool"}]}}"#,
        )
        .expect("write real hook file");

        let inspection = home.inspect(&State::default());
        assert_eq!(inspection.conflicts.len(), 1);
        assert_eq!(inspection.conflicts[0].command, "/other/tool");
    }

    #[test]
    fn hostile_file_shapes_are_classified_without_panicking() {
        let home = Home::new();
        for contents in [
            "[]",
            "null",
            r#"{"hooks": []}"#,
            r#"{"hooks": {"postToolUse": {}}}"#,
            r#"{"hooks": {"postToolUse": [null, 3, "x", {"type": "command"}]}}"#,
            r#"{"hooks": {"postToolUse": [{"type": "command", "bash": 5}]}}"#,
            r#"{"version": "one", "hooks": {"postToolUse": [{"bash": "/x hook copilot tool"}]}}"#,
        ] {
            std::fs::write(hook_file(&home.root), contents).expect("write file");
            let inspection = home.inspect(&State::default());
            // Whatever the shape, the file is never ours to rewrite.
            assert!(
                matches!(
                    inspection.installed,
                    Installed::Modified { .. } | Installed::Unreadable | Installed::Absent
                ),
                "unexpected classification for {contents}"
            );
            assert!(!remove(&home.root, &mut State::default()).expect("remove"));
            assert_eq!(
                std::fs::read_to_string(hook_file(&home.root)).expect("read back"),
                contents,
                "the file was changed"
            );
        }
    }

    #[test]
    fn a_hand_written_file_with_that_name_is_preserved() {
        let home = Home::new();
        let contents = r#"{"version": 1, "hooks": {"postToolUse": [{"type": "command", "bash": "/mine/tool"}]}}"#;
        std::fs::write(hook_file(&home.root), contents).expect("write file");
        let mut state = State::default();

        assert!(matches!(
            home.inspect(&state).installed,
            Installed::Modified { .. }
        ));
        assert_eq!(
            install(&home.root, &home.executable(), &mut state),
            Err(InstallError::Unexpected)
        );
        assert!(!remove(&home.root, &mut state).expect("remove reports preservation"));
        assert_eq!(
            std::fs::read_to_string(hook_file(&home.root)).expect("read back"),
            contents
        );
    }

    #[test]
    fn a_hand_edited_managed_file_is_preserved_not_reverted() {
        // `INT-004`: only a stale binary path may be rewritten. An edit to
        // anything else, such as the timeout, means the file is no longer ours to
        // revert or delete.
        let home = Home::new();
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");

        let edited = std::fs::read_to_string(hook_file(&home.root))
            .expect("read")
            .replace("\"timeoutSec\": 5", "\"timeoutSec\": 30");
        std::fs::write(hook_file(&home.root), &edited).expect("write edit");

        assert!(matches!(
            home.inspect(&state).installed,
            Installed::Modified { .. }
        ));
        assert_eq!(
            install(&home.root, &home.executable(), &mut state),
            Err(InstallError::Unexpected),
            "an edited file must not be rewritten"
        );
        assert!(
            !remove(&home.root, &mut state).expect("remove"),
            "an edited file must not be deleted"
        );
        assert_eq!(
            std::fs::read_to_string(hook_file(&home.root)).expect("read back"),
            edited
        );
    }

    #[test]
    fn an_outdated_file_is_updated_in_place() {
        let home = Home::new();
        let mut state = State::default();
        let other = home.root.join("bin").join("other-secretsieve");
        std::fs::create_dir_all(other.parent().expect("parent")).expect("bin directory");
        std::fs::write(&other, "#!/bin/sh\n").expect("write other executable");

        install(&home.root, &other, &mut state).expect("install");
        assert!(matches!(
            home.inspect(&state).installed,
            Installed::Outdated { .. }
        ));
        install(&home.root, &home.executable(), &mut state).expect("update");
        assert_eq!(home.inspect(&state).installed, Installed::Current);
    }

    #[test]
    fn copilot_is_experimental() {
        assert_eq!(Harness::Copilot.tier_label(), "EXPERIMENTAL");
    }

    #[test]
    fn progress_lines_are_skipped_when_parsing_a_response() {
        let output = b"{\"type\":\"progress\",\"message\":\"working\"}\n{\"modifiedResult\":{\"textResultForLlm\":\"clean\"}}\n";
        let value = parse_last_object(output).expect("parsed");
        assert_eq!(value["modifiedResult"]["textResultForLlm"], json!("clean"));
    }
}
