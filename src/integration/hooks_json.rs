//! Shared editing of a host JSON hooks file.
//!
//! Claude Code (`~/.claude/settings.json`) and Codex CLI (`~/.codex/hooks.json`)
//! both describe hooks as `hooks.<Event>` arrays of matcher groups, each holding
//! command handlers with a seconds-valued `timeout`. This module is the shared
//! implementation those two installers reuse; it was extracted only after the
//! second concrete use (`architecture.md`).
//!
//! `INT-003`: the installed command is the absolute binary path plus ContextVeil's
//! own hidden arguments, shell-quoted so a host that runs it through a shell
//! cannot re-split or expand it. `INT-004`: unrelated hooks and unrelated keys
//! are preserved, and only an artifact whose ownership and unchanged identity can
//! be established is rewritten or removed.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::integration::shell_quote;
use crate::sanitize;

/// The managed artifact for one host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// Event key under the top-level `hooks` object, for example `PostToolUse`.
    pub event: &'static str,
    /// ContextVeil's hidden arguments, for example `hook claude`.
    pub arguments: &'static str,
    /// Timeout in seconds (`RUN-004`).
    pub timeout: u64,
}

/// A matcher that selects every tool.
///
/// Verified for both hosts: an omitted matcher, `""`, and `"*"` all match every
/// tool.
const WILDCARD_MATCHER: &str = "*";

/// State of the managed hook inside a host hooks file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    Absent,
    /// Present and identical to what this binary would install.
    Current,
    /// Present in managed shape but naming a different binary path.
    Outdated {
        command: String,
    },
    /// Present but edited, so it is not ours to rewrite or remove (`INT-004`).
    Modified {
        command: String,
    },
    /// The file exists but is not valid JSON.
    Unreadable,
    /// The file is valid JSON but its `hooks` shape is unexpected.
    Unexpected,
}

/// A competing mutating hook on the same event (`INT-005`, `LIM-017`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The other hook's command, sanitized for display.
    pub command: String,
    pub approved: bool,
}

/// Why an installation or removal could not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Unreadable,
    Unexpected,
    Write,
}

/// The command string ContextVeil installs for `executable`.
pub fn managed_command(executable: &Path, spec: Spec) -> String {
    format!(
        "{} {}",
        shell_quote(&executable.to_string_lossy()),
        spec.arguments
    )
}

/// True when `command` is ContextVeil's hook command for some binary path.
///
/// Ownership is established by shape as well as by the recorded command, so a
/// lost state file cannot orphan an installed hook and moving the binary does not
/// either. The shape is narrow: an absolute path followed by exactly
/// ContextVeil's own hidden entry point.
pub fn is_managed_command(command: &str, spec: Spec) -> bool {
    let suffix = format!(" {}", spec.arguments);
    let Some(executable) = command.strip_suffix(&suffix) else {
        return false;
    };
    let executable = executable.trim_matches('\'');
    !executable.is_empty() && Path::new(executable).is_absolute()
}

/// Reads a hooks file.
pub fn read(path: &Path) -> Result<Option<Map<String, Value>>, Installed> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Installed::Unreadable),
    };
    if text.trim().is_empty() {
        return Ok(Some(Map::new()));
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(settings)) => Ok(Some(settings)),
        Ok(_) => Err(Installed::Unexpected),
        Err(_) => Err(Installed::Unreadable),
    }
}

/// Installs or updates the managed hook.
pub fn install(path: &Path, executable: &Path, spec: Spec) -> Result<(), Problem> {
    let command = managed_command(executable, spec);
    let mut file = match read(path) {
        Ok(Some(file)) => file,
        Ok(None) => Map::new(),
        Err(Installed::Unreadable) => return Err(Problem::Unreadable),
        Err(_) => return Err(Problem::Unexpected),
    };

    let groups = groups_mut(&mut file, spec).ok_or(Problem::Unexpected)?;
    // `INT-004`: never create a second managed entry.
    groups.retain(|group| !is_exactly_managed(group, spec));
    groups.push(json!({
        "matcher": WILDCARD_MATCHER,
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": spec.timeout,
        }],
    }));

    write(path, &file)
}

/// Removes the managed hook.
///
/// Returns `Ok(false)` when a ContextVeil hook was left in place because it had
/// been modified by hand (`INT-004`).
pub fn remove(path: &Path, spec: Spec) -> Result<bool, Problem> {
    let mut file = match read(path) {
        Ok(Some(file)) => file,
        Ok(None) => return Ok(true),
        Err(Installed::Unreadable) => return Err(Problem::Unreadable),
        Err(_) => return Err(Problem::Unexpected),
    };

    let Some(groups) = groups_mut(&mut file, spec) else {
        return Ok(true);
    };
    let modified_present = groups
        .iter()
        .any(|group| mentions_managed(group, spec) && !is_exactly_managed(group, spec));
    groups.retain(|group| !is_exactly_managed(group, spec));
    prune_empty(&mut file, spec);

    write(path, &file)?;
    Ok(!modified_present)
}

/// Classifies the managed hook and collects conflicts.
pub fn classify(
    file: &Map<String, Value>,
    spec: Spec,
    current_command: Option<&str>,
    approved: &[String],
) -> (Installed, Vec<Conflict>) {
    let Some(groups) = groups(file, spec) else {
        return (Installed::Absent, Vec::new());
    };

    let mut installed = Installed::Absent;
    let mut conflicts = Vec::new();
    for group in groups {
        if is_exactly_managed(group, spec) {
            let command = group["hooks"][0]["command"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            installed = match current_command {
                Some(current) if current == command => Installed::Current,
                _ => Installed::Outdated { command },
            };
            continue;
        }
        if mentions_managed(group, spec) {
            installed = Installed::Modified {
                command: sanitize::text(&command_hooks(group).join(", ")),
            };
            continue;
        }
        // `INT-005`: any other command hook on this event could also mutate the
        // result, so it is offered for individual approval.
        for command in command_hooks(group) {
            conflicts.push(Conflict {
                approved: approved.contains(&command),
                command: sanitize::text(&command),
            });
        }
    }
    (installed, conflicts)
}

/// The ContextVeil hook entry in the file, with its configured timeout.
pub fn managed_entry(file: &Map<String, Value>, spec: Spec) -> Option<(String, Option<u64>)> {
    for group in groups(file, spec)? {
        let Some(hooks) = group.get("hooks").and_then(Value::as_array) else {
            continue;
        };
        for hook in hooks {
            let Some(command) = hook.get("command").and_then(Value::as_str) else {
                continue;
            };
            if is_managed_command(command, spec) {
                return Some((
                    command.to_string(),
                    hook.get("timeout").and_then(Value::as_u64),
                ));
            }
        }
    }
    None
}

/// The executable path recorded in a managed command.
pub fn command_executable(command: &str, spec: Spec) -> Option<std::path::PathBuf> {
    command
        .strip_suffix(&format!(" {}", spec.arguments))
        .map(|path| std::path::PathBuf::from(path.trim_matches('\'')))
}

/// True when a group is exactly the artifact ContextVeil installs.
fn is_exactly_managed(group: &Value, spec: Spec) -> bool {
    let Some(object) = group.as_object() else {
        return false;
    };
    let matcher_is_wildcard = match object.get("matcher") {
        None => true,
        Some(Value::String(matcher)) => matcher.is_empty() || matcher == WILDCARD_MATCHER,
        Some(_) => false,
    };
    if !matcher_is_wildcard || object.len() > 2 {
        return false;
    }
    let Some(Value::Array(hooks)) = object.get("hooks") else {
        return false;
    };
    if hooks.len() != 1 {
        return false;
    }
    let Some(hook) = hooks[0].as_object() else {
        return false;
    };
    hook.len() == 3
        && hook.get("type").and_then(Value::as_str) == Some("command")
        && hook.get("timeout").and_then(Value::as_u64) == Some(spec.timeout)
        && hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| is_managed_command(command, spec))
}

/// True when a group mentions a ContextVeil command in any shape.
fn mentions_managed(group: &Value, spec: Spec) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| is_managed_command(command, spec))
            })
        })
}

/// Every command hook in a group, in file order.
fn command_hooks(group: &Value) -> Vec<String> {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| {
            hooks
                .iter()
                .filter(|hook| hook.get("type").and_then(Value::as_str) == Some("command"))
                .filter_map(|hook| hook.get("command").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn groups(file: &Map<String, Value>, spec: Spec) -> Option<&Vec<Value>> {
    file.get("hooks")?.get(spec.event)?.as_array()
}

/// Returns the event array, creating it when absent.
///
/// Returns `None` when `hooks` or the event key exists with an unexpected type,
/// so the file is left untouched rather than rewritten.
fn groups_mut(file: &mut Map<String, Value>, spec: Spec) -> Option<&mut Vec<Value>> {
    let hooks = file
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks.as_object_mut()?;
    let event = hooks
        .entry(spec.event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    event.as_array_mut()
}

/// Removes containers this installer created but left empty.
fn prune_empty(file: &mut Map<String, Value>, spec: Spec) {
    let hooks_empty = match file.get_mut("hooks").and_then(Value::as_object_mut) {
        None => false,
        Some(hooks) => {
            if hooks
                .get(spec.event)
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
            {
                hooks.remove(spec.event);
            }
            hooks.is_empty()
        }
    };
    if hooks_empty {
        file.remove("hooks");
    }
}

fn write(path: &Path, file: &Map<String, Value>) -> Result<(), Problem> {
    let mut rendered = serde_json::to_string_pretty(file).map_err(|_| Problem::Write)?;
    rendered.push('\n');
    crate::setup::write::write_text(path, &rendered, false)
        .map(|_| ())
        .map_err(|_| Problem::Write)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Canary;

    const SPEC: Spec = Spec {
        event: "PostToolUse",
        arguments: "hook test",
        timeout: 5,
    };

    struct Fixture {
        root: std::path::PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "contextveil-hooks-json-{}-{}",
                std::process::id(),
                Canary::generate("HOOKS").token()
            ));
            std::fs::create_dir_all(&root).expect("fixture root");
            Self { root }
        }

        fn path(&self) -> std::path::PathBuf {
            self.root.join("hooks.json")
        }

        fn executable(&self) -> std::path::PathBuf {
            self.root.join("bin").join("contextveil")
        }

        fn write(&self, contents: &str) {
            std::fs::write(self.path(), contents).expect("write hooks file");
        }

        fn read(&self) -> String {
            std::fs::read_to_string(self.path()).expect("read hooks file")
        }

        fn parsed(&self) -> Value {
            serde_json::from_str(&self.read()).expect("valid JSON")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn installation_creates_one_wildcard_command_hook() {
        let fixture = Fixture::new();
        install(&fixture.path(), &fixture.executable(), SPEC).expect("install");
        let group = &fixture.parsed()["hooks"]["PostToolUse"][0];
        assert_eq!(group["matcher"], json!("*"));
        assert_eq!(group["hooks"][0]["type"], json!("command"));
        assert_eq!(group["hooks"][0]["timeout"], json!(5));
        assert!(
            group["hooks"][0]["command"]
                .as_str()
                .expect("command")
                .ends_with(" hook test")
        );
    }

    #[test]
    fn repeat_installation_is_byte_identical() {
        let fixture = Fixture::new();
        install(&fixture.path(), &fixture.executable(), SPEC).expect("install");
        let first = fixture.read();
        install(&fixture.path(), &fixture.executable(), SPEC).expect("install again");
        assert_eq!(fixture.read(), first);
    }

    #[test]
    fn unrelated_keys_and_hooks_survive() {
        let fixture = Fixture::new();
        fixture.write(
            r#"{"description": "mine", "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/other/tool"}]}]}}"#,
        );
        install(&fixture.path(), &fixture.executable(), SPEC).expect("install");
        let file = fixture.parsed();
        assert_eq!(file["description"], json!("mine"));
        assert_eq!(
            file["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            json!("/other/tool")
        );
    }

    #[test]
    fn removal_takes_only_the_managed_entry_and_prunes_what_it_created() {
        let fixture = Fixture::new();
        install(&fixture.path(), &fixture.executable(), SPEC).expect("install");
        assert!(remove(&fixture.path(), SPEC).expect("remove"));
        assert!(fixture.parsed().get("hooks").is_none());

        fixture.write(
            r#"{"hooks": {"PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/other/tool"}]}]}}"#,
        );
        install(&fixture.path(), &fixture.executable(), SPEC).expect("install");
        assert!(remove(&fixture.path(), SPEC).expect("remove"));
        let groups = fixture.parsed()["hooks"]["PostToolUse"]
            .as_array()
            .expect("array")
            .clone();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"][0]["command"], json!("/other/tool"));
    }

    #[test]
    fn a_modified_entry_is_preserved() {
        let fixture = Fixture::new();
        fixture.write(
            r#"{"hooks": {"PostToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/opt/contextveil hook test", "timeout": 30}]}]}}"#,
        );
        let file = read(&fixture.path()).expect("readable").expect("present");
        let (installed, conflicts) = classify(&file, SPEC, None, &[]);
        assert!(matches!(installed, Installed::Modified { .. }));
        assert!(conflicts.is_empty());
        assert!(!remove(&fixture.path(), SPEC).expect("remove"));
        assert_eq!(
            fixture.parsed()["hooks"]["PostToolUse"][0]["hooks"][0]["timeout"],
            json!(30)
        );
    }

    #[test]
    fn an_unreadable_or_unexpected_file_is_never_overwritten() {
        let fixture = Fixture::new();
        let malformed = "{ not json";
        fixture.write(malformed);
        assert_eq!(
            install(&fixture.path(), &fixture.executable(), SPEC),
            Err(Problem::Unreadable)
        );
        assert_eq!(remove(&fixture.path(), SPEC), Err(Problem::Unreadable));
        assert_eq!(fixture.read(), malformed);

        let unexpected = r#"{"hooks": {"PostToolUse": 3}}"#;
        fixture.write(unexpected);
        assert_eq!(
            install(&fixture.path(), &fixture.executable(), SPEC),
            Err(Problem::Unexpected)
        );
        assert_eq!(fixture.read(), unexpected);
    }

    #[test]
    fn other_command_hooks_become_conflicts_and_can_be_approved() {
        let fixture = Fixture::new();
        fixture.write(
            r#"{"hooks": {"PostToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/other/mutator"}, {"type": "mcp_tool", "command": "/ignored"}]}]}}"#,
        );
        let file = read(&fixture.path()).expect("readable").expect("present");
        let (installed, conflicts) = classify(&file, SPEC, None, &[]);
        assert_eq!(installed, Installed::Absent);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].command, "/other/mutator");
        assert!(!conflicts[0].approved);

        let approved = vec!["/other/mutator".to_string()];
        let (_, conflicts) = classify(&file, SPEC, None, &approved);
        assert!(conflicts[0].approved);
    }

    #[test]
    fn conflict_commands_are_sanitized() {
        let fixture = Fixture::new();
        fixture.write(
            "{\"hooks\": {\"PostToolUse\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"\\u001b[31mevil\"}]}]}}",
        );
        let file = read(&fixture.path()).expect("readable").expect("present");
        let (_, conflicts) = classify(&file, SPEC, None, &[]);
        assert_eq!(conflicts[0].command, "\\e[31mevil");
    }

    #[test]
    fn the_current_binary_decides_between_current_and_outdated() {
        let fixture = Fixture::new();
        install(&fixture.path(), &fixture.executable(), SPEC).expect("install");
        let file = read(&fixture.path()).expect("readable").expect("present");
        let current = managed_command(&fixture.executable(), SPEC);
        assert_eq!(
            classify(&file, SPEC, Some(&current), &[]).0,
            Installed::Current
        );
        assert!(matches!(
            classify(&file, SPEC, Some("/elsewhere/contextveil hook test"), &[]).0,
            Installed::Outdated { .. }
        ));
    }

    #[test]
    fn managed_entries_expose_their_command_and_timeout() {
        let fixture = Fixture::new();
        install(&fixture.path(), &fixture.executable(), SPEC).expect("install");
        let file = read(&fixture.path()).expect("readable").expect("present");
        let (command, timeout) = managed_entry(&file, SPEC).expect("entry");
        assert_eq!(timeout, Some(5));
        assert_eq!(
            command_executable(&command, SPEC),
            Some(fixture.executable())
        );
    }

    #[test]
    fn managed_commands_are_recognized_only_in_their_exact_shape() {
        assert!(is_managed_command("/opt/contextveil hook test", SPEC));
        assert!(is_managed_command(
            "'/opt/my bin/contextveil' hook test",
            SPEC
        ));
        assert!(!is_managed_command("/opt/contextveil hook other", SPEC));
        assert!(!is_managed_command("relative/contextveil hook test", SPEC));
        assert!(!is_managed_command(" hook test", SPEC));
    }

    #[test]
    fn two_specs_never_claim_each_others_entries() {
        const OTHER: Spec = Spec {
            event: "PostToolUse",
            arguments: "hook different",
            timeout: 5,
        };
        let fixture = Fixture::new();
        install(&fixture.path(), &fixture.executable(), SPEC).expect("install");
        let file = read(&fixture.path()).expect("readable").expect("present");
        assert_eq!(classify(&file, OTHER, None, &[]).0, Installed::Absent);
        // The other spec sees it as a foreign command hook.
        assert_eq!(classify(&file, OTHER, None, &[]).1.len(), 1);
    }
}
