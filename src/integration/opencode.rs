//! OpenCode integration installation, inspection, and verification.
//!
//! OpenCode is experimental (`SUP-002`, `SUP-003`). `OCO-001`: setup manages one
//! SecretSieve-owned TypeScript plugin file under `~/.config/opencode/plugins/`
//! that invokes the absolute Rust binary with one JSON request on stdin and reads
//! one JSON response from stdout.
//!
//! Verified against the installed OpenCode 1.18.18: plugin files are discovered
//! one level deep in `plugin/` or `plugins/` under each config scope, `*.ts` and
//! `*.js` are loaded, any exported function of the plugin type is used, plugins
//! run inside OpenCode's own Bun process so `Bun.spawn` is available, and a plugin
//! that fails to load is skipped with a surfaced error rather than crashing the
//! host (`LIM-016`).

use std::path::{Path, PathBuf};

use crate::integration::hooks_json::{Conflict, Installed};
use crate::integration::state::{Managed, State};
use crate::integration::{
    self as integration, Harness, Inspection, InstallError, SYNTHETIC_PLACEHOLDER, SyntheticConfig,
    Verification, claude,
};
use crate::sanitize;

/// The plugin source SecretSieve installs, with the binary path substituted.
const TEMPLATE: &str = include_str!("../../assets/opencode/plugin.ts");

/// Placeholder replaced by the absolute binary path (`INT-003`).
const BINARY_PLACEHOLDER: &str = "__SECRETSIEVE_BINARY__";

/// First line of every managed plugin file, used to establish ownership.
const MARKER: &str = "// SecretSieve managed plugin.";

/// The file SecretSieve owns (`OCO-001`).
pub const FILENAME: &str = "secretsieve.ts";

/// Directory holding OpenCode's global plugins (`OCO-001`).
pub fn plugins_directory(home: &Path) -> PathBuf {
    home.join(".config").join("opencode").join("plugins")
}

/// Path of the managed plugin file.
pub fn plugin_file(home: &Path) -> PathBuf {
    plugins_directory(home).join(FILENAME)
}

/// The plugin source for `executable`.
///
/// The path is embedded as a JSON string so a quote or backslash in it cannot
/// break out of the literal (`INT-003`: no shell interpolation, and here no
/// source-code injection either).
pub fn render(executable: &Path) -> String {
    let literal = serde_json::to_string(&executable.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "\"\"".to_string());
    let quoted = format!("\"{BINARY_PLACEHOLDER}\"");
    TEMPLATE.replace(&quoted, &literal)
}

/// Inspects detection, installation, and conflicts without changing anything.
pub fn inspect(
    environment: &crate::source::Environment,
    home: &Path,
    executable: Option<&Path>,
    state: &State,
) -> Inspection {
    let artifact_path = plugin_file(home);
    let installed = classify(&artifact_path, executable);
    // The 5-second bound lives inside the installed plugin source rather than in
    // host configuration, so it is reported only when a plugin is present.
    let hook_timeout = match installed {
        Installed::Current | Installed::Outdated { .. } => Some(claude::TIMEOUT_SECONDS),
        _ => None,
    };

    Inspection {
        harness: Harness::OpenCode,
        artifact_path,
        detection: detect(environment, home),
        installed,
        conflicts: conflicts(home, &state.approved_conflicts(Harness::OpenCode)),
        hook_executable: recorded_executable(&plugin_file(home)),
        hook_timeout,
        disabled_by_policy: false,
    }
}

fn detect(environment: &crate::source::Environment, home: &Path) -> integration::Detection {
    if integration::find_executable(environment.get_str("PATH"), "opencode").is_some()
        || home.join(".config").join("opencode").is_dir()
    {
        integration::Detection::Detected
    } else {
        integration::Detection::NotDetected
    }
}

/// Classifies the managed plugin file (`INT-004`).
fn classify(path: &Path, executable: Option<&Path>) -> Installed {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Installed::Absent,
        Err(_) => return Installed::Unreadable,
    };
    if !text.starts_with(MARKER) {
        // A file with that name that SecretSieve did not write is not ours to
        // rewrite or remove.
        return Installed::Modified {
            command: sanitize::path(path),
        };
    }
    match executable {
        Some(executable) if text == render(executable) => Installed::Current,
        _ => Installed::Outdated {
            command: sanitize::path(path),
        },
    }
}

/// The binary path embedded in an installed plugin file.
fn recorded_executable(path: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(path).ok()?;
    if !text.starts_with(MARKER) {
        return None;
    }
    let line = text
        .lines()
        .find(|line| line.starts_with("const SECRETSIEVE_BINARY = "))?;
    let literal = line
        .trim_end_matches(';')
        .split_once('=')
        .map(|(_, value)| value.trim())?;
    serde_json::from_str::<String>(literal)
        .ok()
        .map(PathBuf::from)
}

/// Other plugin files that could also mutate the same content.
///
/// Every plugin in the directory is a potential mutator, and OpenCode gives no
/// static way to tell which hooks a plugin registers, so they are listed by name
/// for individual approval (`INT-005`, `LIM-017`).
fn conflicts(home: &Path, approved: &[String]) -> Vec<Conflict> {
    let mut names: Vec<String> = Vec::new();
    for directory in ["plugin", "plugins"] {
        let path = home.join(".config").join("opencode").join(directory);
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let file = entry.path();
            let is_plugin = file
                .extension()
                .is_some_and(|extension| extension == "ts" || extension == "js");
            let is_ours = file.file_name().is_some_and(|name| name == FILENAME);
            if is_plugin && !is_ours {
                names.push(file.to_string_lossy().into_owned());
            }
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|name| Conflict {
            approved: approved.contains(&name),
            command: sanitize::text(&name),
        })
        .collect()
}

/// Installs or updates the managed plugin file.
pub fn install(home: &Path, executable: &Path, state: &mut State) -> Result<(), InstallError> {
    let path = plugin_file(home);
    if matches!(
        classify(&path, Some(executable)),
        Installed::Modified { .. }
    ) {
        return Err(InstallError::Unexpected);
    }
    std::fs::create_dir_all(plugins_directory(home)).map_err(|_| InstallError::Write)?;
    crate::setup::write::write_text(&path, &render(executable), false)
        .map_err(|_| InstallError::Write)?;

    let approved = state.approved_conflicts(Harness::OpenCode);
    state.set(
        Harness::OpenCode,
        Some(Managed {
            command: path.to_string_lossy().into_owned(),
            approved_conflicts: approved,
        }),
    );
    Ok(())
}

/// Removes the managed plugin file.
///
/// Returns `Ok(false)` when a same-named file was preserved because SecretSieve
/// did not write it (`INT-004`).
pub fn remove(home: &Path, state: &mut State) -> Result<bool, InstallError> {
    let path = plugin_file(home);
    let removed = match classify(&path, None) {
        Installed::Absent => true,
        Installed::Modified { .. } | Installed::Unreadable | Installed::Unexpected => false,
        Installed::Current | Installed::Outdated { .. } => {
            std::fs::remove_file(&path).map_err(|_| InstallError::Write)?;
            true
        }
    };
    state.set(Harness::OpenCode, None);
    Ok(removed)
}

/// Runs the installed binary against a synthetic transport request.
///
/// This verifies SecretSieve's side of the plugin transport offline. The plugin
/// side is covered by its own test suite, which OpenCode's Bun runtime executes
/// (`DIA-006`).
pub fn verify_offline(executable: &Path) -> Verification {
    let Some(config) = SyntheticConfig::create("OPENCODE") else {
        return Verification::Failed("a temporary configuration could not be created");
    };
    let canary = integration::synthetic_canary("OPENCODE-VALUE");
    let request = serde_json::json!({
        "version": 1,
        "event": "tool.execute.after",
        "project_root": "/nonexistent-synthetic-project",
        "texts": [canary.clone()],
    })
    .to_string();

    let Some(output) = claude::run_hook(
        executable,
        "hook opencode",
        config.root(),
        &canary,
        &request,
    ) else {
        return Verification::Failed("the configured executable did not answer in time");
    };
    let Ok(response) = serde_json::from_slice::<serde_json::Value>(&output) else {
        return Verification::Failed("the transport did not return valid protocol output");
    };
    if response["status"].as_str() != Some("ok") {
        return Verification::Failed("the transport did not report a usable registry");
    }
    match response["texts"][0].as_str() {
        Some(SYNTHETIC_PLACEHOLDER) => Verification::Passed,
        _ => Verification::Failed("the transport did not return the expected replacement"),
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
                "secretsieve-opencode-integration-{}-{}",
                std::process::id(),
                Canary::generate("HOME").token()
            ));
            std::fs::create_dir_all(plugins_directory(&root)).expect("plugins directory");
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
    fn installation_writes_one_owned_plugin_file() {
        let home = Home::new();
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");

        let source = std::fs::read_to_string(plugin_file(&home.root)).expect("plugin file");
        assert!(source.starts_with(MARKER));
        assert!(!source.contains(BINARY_PLACEHOLDER));
        assert!(source.contains(&home.executable().to_string_lossy().into_owned()));
        assert!(source.contains("chat.message"));
        assert!(source.contains("tool.execute.after"));
        assert_eq!(home.inspect(&state).installed, Installed::Current);
    }

    #[test]
    fn the_plugin_carries_no_matcher_or_resolver_logic() {
        // `OCO-004`: security semantics stay in Rust.
        let source = render(Path::new("/opt/secretsieve"));
        for forbidden in ["SECRET:", "leftmost", "dotenv", "placeholder", "regex"] {
            assert!(
                !source.contains(forbidden),
                "the plugin must not contain `{forbidden}`"
            );
        }
        // It also must not reach for a V2 API.
        assert!(!source.contains("/v2/"));
    }

    #[test]
    fn an_awkward_binary_path_cannot_break_out_of_the_source_literal() {
        let source = render(Path::new("/tmp/a\"b\\c/secretsieve"));
        assert!(source.contains(r#""/tmp/a\"b\\c/secretsieve""#));
    }

    #[test]
    fn an_outdated_plugin_is_updated_in_place() {
        let home = Home::new();
        let mut state = State::default();
        let other = home.root.join("bin").join("old-secretsieve");
        std::fs::create_dir_all(other.parent().expect("parent")).expect("bin directory");
        std::fs::write(&other, "#!/bin/sh\n").expect("write other executable");

        install(&home.root, &other, &mut state).expect("install");
        assert!(matches!(
            home.inspect(&state).installed,
            Installed::Outdated { .. }
        ));
        install(&home.root, &home.executable(), &mut state).expect("update");
        assert_eq!(home.inspect(&state).installed, Installed::Current);
        assert_eq!(
            home.inspect(&state).hook_executable,
            Some(home.executable())
        );
    }

    #[test]
    fn a_hand_written_file_with_that_name_is_preserved() {
        let home = Home::new();
        let contents = "export const Mine = async () => ({})\n";
        std::fs::write(plugin_file(&home.root), contents).expect("write plugin");
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
            std::fs::read_to_string(plugin_file(&home.root)).expect("read back"),
            contents
        );
    }

    #[test]
    fn removal_deletes_only_the_managed_plugin() {
        let home = Home::new();
        let other = plugins_directory(&home.root).join("other.ts");
        std::fs::write(&other, "export const Other = async () => ({})\n").expect("write other");

        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");
        assert!(remove(&home.root, &mut state).expect("remove"));
        assert!(!plugin_file(&home.root).exists());
        assert!(other.exists());
    }

    #[test]
    fn other_plugins_are_listed_for_approval() {
        let home = Home::new();
        std::fs::write(
            plugins_directory(&home.root).join("other.ts"),
            "export const Other = async () => ({})\n",
        )
        .expect("write other plugin");
        // The singular directory name is also discovered by the host.
        let singular = home.root.join(".config").join("opencode").join("plugin");
        std::fs::create_dir_all(&singular).expect("singular directory");
        std::fs::write(
            singular.join("legacy.js"),
            "export const Legacy = () => ({})\n",
        )
        .expect("write legacy plugin");

        let mut state = State::default();
        let conflicts = home.inspect(&state).conflicts;
        assert_eq!(conflicts.len(), 2);
        assert!(conflicts.iter().all(|conflict| !conflict.approved));

        let name = conflicts[0].command.clone();
        integration::approve_conflict(Harness::OpenCode, &mut state, &name);
        assert!(
            home.inspect(&state)
                .conflicts
                .iter()
                .any(|conflict| conflict.approved)
        );
    }

    #[test]
    fn the_plugin_bounds_the_subprocess_at_five_seconds() {
        // `RUN-004`: the bound lives in the plugin source rather than in host
        // configuration, so it is asserted against the shared constant.
        let source = render(Path::new("/opt/secretsieve"));
        let expected = claude::TIMEOUT_SECONDS * 1000;
        assert!(
            source.contains(&format!("TIMEOUT_MS = {expected}")),
            "the plugin no longer bounds the subprocess at {expected} ms"
        );
        assert!(source.contains("setTimeout"), "the bound is never armed");
        assert!(source.contains("kill()"), "the bound never kills the subprocess");
    }

    #[test]
    fn opencode_is_experimental_and_needs_no_extra_host_step() {
        assert_eq!(Harness::OpenCode.tier_label(), "EXPERIMENTAL");
        assert_eq!(Harness::OpenCode.post_install_note(), None);
    }
}
