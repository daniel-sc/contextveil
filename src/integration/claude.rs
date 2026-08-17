//! Claude Code integration installation, inspection, and verification.
//!
//! `CLA-001`: setup manages one synchronous wildcard `PostToolUse` command hook
//! in `~/.claude/settings.json` with a 5-second timeout (`RUN-004`).
//! `INT-003`: the command uses the absolute current binary path, passes its
//! payload on stdin, and never relies on shell interpolation.
//! `INT-004`: unrelated settings are preserved, and only an artifact whose
//! ownership and unchanged identity can be established is removed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::integration::state::{Managed, State};
use crate::integration::{Detection, find_executable, shell_quote};
use crate::sanitize;
use crate::source::Environment;

/// Hook timeout in seconds (`CLA-001`, `RUN-004`).
pub const TIMEOUT_SECONDS: u64 = 5;

/// The matcher that selects every tool.
///
/// Verified against Claude Code 2.1.233: an omitted matcher, `""`, and `"*"`
/// all match every tool.
const WILDCARD_MATCHER: &str = "*";

const EVENT: &str = "PostToolUse";
const HOOK_ARGUMENTS: [&str; 2] = ["hook", "claude"];

/// Everything setup and doctor need to know about the installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub settings_path: PathBuf,
    pub detection: Detection,
    pub installed: Installed,
    /// Other `PostToolUse` command hooks that could also mutate results.
    pub conflicts: Vec<Conflict>,
    /// The executable path of the SecretSieve hook found in the settings file,
    /// whatever shape the entry has. Doctor checks that it still exists.
    pub hook_executable: Option<PathBuf>,
    /// The timeout recorded on that entry, which doctor checks against
    /// `RUN-004`.
    pub hook_timeout: Option<u64>,
    /// True when a managed-policy file disables all hooks for this host.
    pub hooks_disabled_by_policy: bool,
}

/// State of the managed hook inside the settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// No SecretSieve hook is present.
    Absent,
    /// A managed hook is present and matches the current binary.
    Current,
    /// A managed hook is present but names a different binary path.
    Outdated { command: String },
    /// A SecretSieve hook is present but was edited, so it is not ours to
    /// rewrite or remove (`INT-004`).
    Modified { command: String },
    /// The settings file exists but cannot be parsed.
    SettingsUnreadable,
    /// The settings file is valid JSON but `hooks` is not the expected shape.
    SettingsUnexpected,
}

/// A competing mutating hook (`INT-005`, `LIM-017`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The other hook's command, sanitized for display.
    pub command: String,
    pub approved: bool,
}

/// Why an installation or removal could not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallError {
    NoHome,
    SettingsUnreadable,
    SettingsUnexpected,
    Write,
    ExecutablePath,
}

impl InstallError {
    pub fn reason(&self) -> &'static str {
        match self {
            InstallError::NoHome => "the home directory is unknown",
            InstallError::SettingsUnreadable => {
                "the Claude settings file is not valid JSON and was left unchanged"
            }
            InstallError::SettingsUnexpected => {
                "the Claude settings file has an unexpected `hooks` shape and was left unchanged"
            }
            InstallError::Write => "the Claude settings file could not be written",
            InstallError::ExecutablePath => "the SecretSieve binary path could not be determined",
        }
    }
}

/// Path of the user settings file (`CLA-001`).
pub fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

/// The command string SecretSieve installs for `executable`.
pub fn managed_command(executable: &Path) -> String {
    let mut command = shell_quote(&executable.to_string_lossy());
    for argument in HOOK_ARGUMENTS {
        command.push(' ');
        command.push_str(argument);
    }
    command
}

/// True when `command` is the SecretSieve hook command for some binary path.
///
/// Ownership is established by shape as well as by the recorded command, so a
/// lost or reset state file cannot orphan an installed hook, and renaming or
/// moving the binary does not either. The shape is narrow: an absolute path
/// followed by exactly SecretSieve's own hidden entry point.
pub fn is_managed_command(command: &str) -> bool {
    let Some(executable) = command.strip_suffix(" hook claude") else {
        return false;
    };
    let executable = executable.trim_matches('\'');
    !executable.is_empty() && Path::new(executable).is_absolute()
}

/// Inspects detection, installation, and conflicts without changing anything.
pub fn inspect(
    environment: &Environment,
    home: &Path,
    executable: Option<&Path>,
    state: &State,
) -> Inspection {
    let settings_path = settings_path(home);
    let detection = if find_executable(environment.get_str("PATH"), "claude").is_some()
        || home.join(".claude").is_dir()
    {
        Detection::Detected
    } else {
        Detection::NotDetected
    };

    let current = executable.map(managed_command);
    let settings = read_settings(&settings_path);
    let (installed, conflicts) = match &settings {
        Err(error) => (error.clone(), Vec::new()),
        Ok(None) => (Installed::Absent, Vec::new()),
        Ok(Some(settings)) => classify(settings, current.as_deref(), state),
    };
    let entry = settings
        .as_ref()
        .ok()
        .and_then(|settings| settings.as_ref())
        .and_then(managed_entry);

    Inspection {
        settings_path,
        detection,
        installed,
        conflicts,
        hook_executable: entry.as_ref().and_then(|(command, _)| {
            command
                .strip_suffix(" hook claude")
                .map(|path| PathBuf::from(path.trim_matches('\'')))
        }),
        hook_timeout: entry.and_then(|(_, timeout)| timeout),
        hooks_disabled_by_policy: hooks_disabled_by_policy(),
    }
}

/// The SecretSieve hook entry in the settings file, with its timeout.
fn managed_entry(settings: &Map<String, Value>) -> Option<(String, Option<u64>)> {
    for group in post_tool_use_groups(settings)? {
        let Some(hooks) = group.get("hooks").and_then(Value::as_array) else {
            continue;
        };
        for hook in hooks {
            let Some(command) = hook.get("command").and_then(Value::as_str) else {
                continue;
            };
            if is_managed_command(command) {
                return Some((
                    command.to_string(),
                    hook.get("timeout").and_then(Value::as_u64),
                ));
            }
        }
    }
    None
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

/// Installs or updates the managed hook.
pub fn install(home: &Path, executable: &Path, state: &mut State) -> Result<(), InstallError> {
    let command = managed_command(executable);
    let path = settings_path(home);

    let mut settings = match read_settings(&path) {
        Ok(Some(settings)) => settings,
        Ok(None) => Map::new(),
        Err(Installed::SettingsUnreadable) => return Err(InstallError::SettingsUnreadable),
        Err(_) => return Err(InstallError::SettingsUnexpected),
    };

    let groups = post_tool_use_groups_mut(&mut settings).ok_or(InstallError::SettingsUnexpected)?;
    // `INT-004`: never create a second managed entry.
    groups.retain(|group| !is_exactly_managed(group));
    groups.push(managed_group(&command));

    write_settings(&path, &settings)?;
    state.claude = Some(Managed {
        command,
        approved_conflicts: state
            .claude
            .as_ref()
            .map(|managed| managed.approved_conflicts.clone())
            .unwrap_or_default(),
    });
    Ok(())
}

/// Removes the managed hook.
///
/// Returns `Ok(false)` when an installed SecretSieve hook was left in place
/// because it had been modified (`INT-004`).
pub fn remove(home: &Path, state: &mut State) -> Result<bool, InstallError> {
    let path = settings_path(home);
    let mut settings = match read_settings(&path) {
        Ok(Some(settings)) => settings,
        Ok(None) => {
            state.claude = None;
            return Ok(true);
        }
        Err(Installed::SettingsUnreadable) => return Err(InstallError::SettingsUnreadable),
        Err(_) => return Err(InstallError::SettingsUnexpected),
    };

    let Some(groups) = post_tool_use_groups_mut(&mut settings) else {
        state.claude = None;
        return Ok(true);
    };

    let modified_present = groups
        .iter()
        .any(|group| mentions_managed_command(group) && !is_exactly_managed(group));
    groups.retain(|group| !is_exactly_managed(group));
    prune_empty(&mut settings);

    write_settings(&path, &settings)?;
    state.claude = None;
    Ok(!modified_present)
}

/// Records the user's approval of a competing hook (`INT-005`).
pub fn approve_conflict(state: &mut State, command: &str) {
    let managed = state.claude.get_or_insert_with(Managed::default);
    if !managed
        .approved_conflicts
        .iter()
        .any(|known| known == command)
    {
        managed.approved_conflicts.push(command.to_string());
    }
}

/// Result of the offline synthetic protocol check (`INT-006`, `DIA-006`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    Passed,
    Failed(&'static str),
}

/// Runs the installed binary against a synthetic `PostToolUse` payload.
///
/// The check is offline and self-contained: it enrolls a generated
/// non-credential canary through a temporary configuration directory, feeds it
/// through the real protocol path, and requires the canary to be absent from
/// the response (`SEC-003`, `TST-005`).
pub fn verify_offline(executable: &Path) -> Verification {
    let canary = format!("SSCANARY-VERIFY-{}-{}", std::process::id(), verify_nonce());
    let root = std::env::temp_dir().join(format!("secretsieve-verify-{canary}"));
    let configuration = root.join("secretsieve");
    if std::fs::create_dir_all(&configuration).is_err() {
        return Verification::Failed("a temporary configuration could not be created");
    }
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&root);
    };

    if std::fs::write(
        configuration.join("config.toml"),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"SECRETSIEVE_VERIFY\"\n",
    )
    .is_err()
    {
        cleanup();
        return Verification::Failed("a temporary configuration could not be written");
    }

    let payload = json!({
        "hook_event_name": EVENT,
        "tool_name": "Bash",
        "tool_input": {"command": "printenv SECRETSIEVE_VERIFY"},
        "tool_response": {"stdout": canary.clone(), "stderr": "", "interrupted": false},
    })
    .to_string();

    let outcome = run_hook(executable, &root, &canary, &payload);
    cleanup();
    outcome
}

fn run_hook(executable: &Path, config_root: &Path, canary: &str, payload: &str) -> Verification {
    let mut child = match Command::new(executable)
        .args(HOOK_ARGUMENTS)
        .env_clear()
        .env("XDG_CONFIG_HOME", config_root)
        .env("SECRETSIEVE_VERIFY", canary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return Verification::Failed("the configured executable could not be run"),
    };

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(payload.as_bytes());
    }

    // `RUN-004`: the host allows five seconds, so the check uses the same bound.
    let deadline = Instant::now() + Duration::from_secs(TIMEOUT_SECONDS);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                return Verification::Failed("the hook did not answer within the timeout");
            }
            Err(_) => return Verification::Failed("the hook process could not be waited for"),
        }
    }

    let Ok(output) = child.wait_with_output() else {
        return Verification::Failed("the hook output could not be read");
    };
    if !output.status.success() {
        return Verification::Failed("the hook exited with a failure status");
    }
    if output
        .stdout
        .windows(canary.len())
        .any(|w| w == canary.as_bytes())
        || output
            .stderr
            .windows(canary.len())
            .any(|w| w == canary.as_bytes())
    {
        return Verification::Failed("the synthetic value was not replaced");
    }
    let Ok(response) = serde_json::from_slice::<Value>(&output.stdout) else {
        return Verification::Failed("the hook did not return valid protocol output");
    };
    let updated = &response["hookSpecificOutput"]["updatedToolOutput"]["stdout"];
    match updated.as_str() {
        Some("<SECRET:SECRETSIEVE_VERIFY>") => Verification::Passed,
        _ => Verification::Failed("the hook did not return the expected replacement"),
    }
}

/// Runs the optional paid, networked Claude canary (`DIA-005`).
///
/// It enrolls a conspicuous random non-credential value through a temporary
/// source configuration, asks Claude Code once to print that value with a
/// narrowly allowed `Bash` tool, and requires the value to be absent from
/// Claude's reply. It therefore tests exactly one path: a successful `Bash`
/// `PostToolUse` result flowing through the installed hook.
///
/// This is the only network-capable runtime workflow besides installation
/// (`SEC-003`), it is never enabled by default, and it has no automated coverage
/// by design (`TST-008`, `DEV-001`): running it costs money and requires host
/// credentials, so it is exercised by the manual release qualification
/// (`REL-008`).
pub fn live_canary(home: &Path) -> Result<(), String> {
    let canary = format!("SSCANARY-LIVE-{}-{}", std::process::id(), verify_nonce());
    let root = std::env::temp_dir().join(format!("secretsieve-live-{canary}"));
    let configuration = root.join("secretsieve");
    std::fs::create_dir_all(&configuration)
        .map_err(|_| "a temporary configuration could not be created".to_string())?;
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&root);
    };
    if std::fs::write(
        configuration.join("config.toml"),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"SECRETSIEVE_LIVE_CANARY\"\n",
    )
    .is_err()
    {
        cleanup();
        return Err("a temporary configuration could not be written".to_string());
    }

    let prompt = "Run exactly this shell command and reply with its output verbatim, nothing \
                  else: printenv SECRETSIEVE_LIVE_CANARY";
    let spawned = Command::new("claude")
        .args(["-p", prompt, "--allowedTools", "Bash(printenv *)"])
        .current_dir(home)
        .env("XDG_CONFIG_HOME", &root)
        .env("SECRETSIEVE_LIVE_CANARY", &canary)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(_) => {
            cleanup();
            return Err("`claude` could not be run".to_string());
        }
    };

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
                cleanup();
                return Err("Claude did not answer within three minutes".to_string());
            }
            Err(_) => {
                cleanup();
                return Err("the Claude process could not be waited for".to_string());
            }
        }
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(_) => {
            cleanup();
            return Err("Claude's output could not be read".to_string());
        }
    };
    cleanup();

    if !output.status.success() {
        return Err("Claude exited with a failure status".to_string());
    }
    let disclosed = output
        .stdout
        .windows(canary.len())
        .any(|window| window == canary.as_bytes());
    if disclosed {
        return Err(
            "the generated value reached Claude's reply, so the covered path did not redact it"
                .to_string(),
        );
    }
    Ok(())
}

fn verify_nonce() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or_default(),
    );
    format!("{:016x}", hasher.finish())
}

/// The absolute path of the running binary (`INT-003`).
pub fn current_executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

fn managed_group(command: &str) -> Value {
    json!({
        "matcher": WILDCARD_MATCHER,
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": TIMEOUT_SECONDS,
        }],
    })
}

/// True when a group is exactly the artifact SecretSieve installs.
fn is_exactly_managed(group: &Value) -> bool {
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
        && hook.get("timeout").and_then(Value::as_u64) == Some(TIMEOUT_SECONDS)
        && hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(is_managed_command)
}

/// True when a group mentions a SecretSieve command in any shape.
fn mentions_managed_command(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(is_managed_command)
            })
        })
}

fn classify(
    settings: &Map<String, Value>,
    current_command: Option<&str>,
    state: &State,
) -> (Installed, Vec<Conflict>) {
    let Some(groups) = post_tool_use_groups(settings) else {
        return (Installed::Absent, Vec::new());
    };

    let mut installed = Installed::Absent;
    let mut conflicts = Vec::new();
    let approved = state
        .claude
        .as_ref()
        .map(|managed| managed.approved_conflicts.clone())
        .unwrap_or_default();

    for group in groups {
        if is_exactly_managed(group) {
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
        if mentions_managed_command(group) {
            installed = Installed::Modified {
                command: sanitize::text(&describe_group(group)),
            };
            continue;
        }
        // `INT-005`: any other command hook on this event could also mutate the
        // result, so it is offered for individual approval.
        for command in command_hooks(group) {
            let approved = approved.contains(&command);
            conflicts.push(Conflict {
                command: sanitize::text(&command),
                approved,
            });
        }
    }
    (installed, conflicts)
}

fn describe_group(group: &Value) -> String {
    command_hooks(group).join(", ")
}

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

fn post_tool_use_groups(settings: &Map<String, Value>) -> Option<&Vec<Value>> {
    settings.get("hooks")?.get(EVENT)?.as_array()
}

/// Returns the `PostToolUse` array, creating it when absent.
///
/// Returns `None` when `hooks` or `hooks.PostToolUse` exists with an unexpected
/// type, so the file is left untouched rather than rewritten.
fn post_tool_use_groups_mut(settings: &mut Map<String, Value>) -> Option<&mut Vec<Value>> {
    let hooks = settings
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks.as_object_mut()?;
    let event = hooks
        .entry(EVENT.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    event.as_array_mut()
}

/// Removes containers this installer created but left empty.
fn prune_empty(settings: &mut Map<String, Value>) {
    let hooks_empty = match settings.get_mut("hooks").and_then(Value::as_object_mut) {
        None => false,
        Some(hooks) => {
            if hooks
                .get(EVENT)
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
            {
                hooks.remove(EVENT);
            }
            hooks.is_empty()
        }
    };
    if hooks_empty {
        settings.remove("hooks");
    }
}

fn read_settings(path: &Path) -> Result<Option<Map<String, Value>>, Installed> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Installed::SettingsUnreadable),
    };
    if text.trim().is_empty() {
        return Ok(Some(Map::new()));
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(settings)) => Ok(Some(settings)),
        Ok(_) => Err(Installed::SettingsUnexpected),
        Err(_) => Err(Installed::SettingsUnreadable),
    }
}

fn write_settings(path: &Path, settings: &Map<String, Value>) -> Result<(), InstallError> {
    let mut rendered = serde_json::to_string_pretty(settings).map_err(|_| InstallError::Write)?;
    rendered.push('\n');
    crate::setup::write::write_text(path, &rendered, false)
        .map(|_| ())
        .map_err(|_| InstallError::Write)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Canary;

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

        /// A path whose file name is `secretsieve`, as a real installation has.
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
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).expect("valid JSON")
    }

    #[test]
    fn a_clean_installation_creates_the_managed_hook() {
        let home = Home::new();
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");

        let settings = parsed(&home.read_settings());
        let groups = settings["hooks"]["PostToolUse"]
            .as_array()
            .expect("PostToolUse array");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["matcher"], json!("*"));
        assert_eq!(groups[0]["hooks"][0]["type"], json!("command"));
        assert_eq!(groups[0]["hooks"][0]["timeout"], json!(5));
        let command = groups[0]["hooks"][0]["command"]
            .as_str()
            .expect("command string");
        assert!(command.ends_with(" hook claude"));
        assert!(is_managed_command(command));
        assert_eq!(state.claude.expect("recorded state").command, command);
    }

    #[test]
    fn repeat_installation_does_not_duplicate_the_entry() {
        let home = Home::new();
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("first install");
        let first = home.read_settings();
        install(&home.root, &home.executable(), &mut state).expect("second install");
        assert_eq!(home.read_settings(), first);
        let settings = parsed(&home.read_settings());
        assert_eq!(
            settings["hooks"]["PostToolUse"]
                .as_array()
                .expect("array")
                .len(),
            1
        );
    }

    #[test]
    fn unrelated_settings_are_preserved() {
        let home = Home::new();
        home.write_settings(
            r#"{
  "model": "opus",
  "env": {"FOO": "bar"},
  "hooks": {
    "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/other/tool"}]}]
  }
}"#,
        );
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");

        let settings = parsed(&home.read_settings());
        assert_eq!(settings["model"], json!("opus"));
        assert_eq!(settings["env"]["FOO"], json!("bar"));
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            json!("/other/tool")
        );
        assert!(settings["hooks"]["PostToolUse"].is_array());
    }

    #[test]
    fn an_upgrade_replaces_an_outdated_managed_entry() {
        let home = Home::new();
        home.write_settings(
            r#"{"hooks": {"PostToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/old/path/secretsieve hook claude", "timeout": 5}]}]}}"#,
        );
        let mut state = State::default();
        let inspection = inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &state,
        );
        assert!(matches!(inspection.installed, Installed::Outdated { .. }));

        install(&home.root, &home.executable(), &mut state).expect("install");
        let settings = parsed(&home.read_settings());
        let groups = settings["hooks"]["PostToolUse"].as_array().expect("array");
        assert_eq!(groups.len(), 1);
        assert!(
            !groups[0]["hooks"][0]["command"]
                .as_str()
                .expect("command")
                .contains("/old/path/")
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

        let settings = parsed(&home.read_settings());
        let groups = settings["hooks"]["PostToolUse"].as_array().expect("array");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"][0]["command"], json!("/other/tool"));
        assert!(state.claude.is_none());
    }

    #[test]
    fn a_modified_entry_is_preserved_with_a_warning() {
        // `INT-004`: identity cannot be established, so it stays.
        let home = Home::new();
        home.write_settings(
            r#"{"hooks": {"PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/opt/secretsieve hook claude", "timeout": 30}]}]}}"#,
        );
        let mut state = State::default();
        let inspection = inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &state,
        );
        assert!(matches!(inspection.installed, Installed::Modified { .. }));

        assert!(!remove(&home.root, &mut state).expect("remove"));
        let settings = parsed(&home.read_settings());
        assert_eq!(
            settings["hooks"]["PostToolUse"][0]["hooks"][0]["timeout"],
            json!(30)
        );
    }

    #[test]
    fn malformed_settings_are_never_overwritten() {
        let home = Home::new();
        let malformed = "{ this is not json";
        home.write_settings(malformed);
        let mut state = State::default();

        assert_eq!(
            install(&home.root, &home.executable(), &mut state),
            Err(InstallError::SettingsUnreadable)
        );
        assert_eq!(home.read_settings(), malformed);
        assert_eq!(
            remove(&home.root, &mut state),
            Err(InstallError::SettingsUnreadable)
        );
        assert_eq!(home.read_settings(), malformed);

        let inspection = inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &state,
        );
        assert_eq!(inspection.installed, Installed::SettingsUnreadable);
    }

    #[test]
    fn an_unexpected_hooks_shape_is_left_untouched() {
        let home = Home::new();
        let unexpected = r#"{"hooks": {"PostToolUse": "not an array"}}"#;
        home.write_settings(unexpected);
        let mut state = State::default();
        assert_eq!(
            install(&home.root, &home.executable(), &mut state),
            Err(InstallError::SettingsUnexpected)
        );
        assert_eq!(home.read_settings(), unexpected);
    }

    #[test]
    fn other_post_tool_use_command_hooks_are_reported_as_conflicts() {
        let home = Home::new();
        home.write_settings(
            r#"{"hooks": {"PostToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/other/mutator --rewrite"}]}]}}"#,
        );
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");

        let inspection = inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &state,
        );
        assert_eq!(inspection.installed, Installed::Current);
        assert_eq!(inspection.conflicts.len(), 1);
        assert_eq!(inspection.conflicts[0].command, "/other/mutator --rewrite");
        assert!(!inspection.conflicts[0].approved);

        approve_conflict(&mut state, "/other/mutator --rewrite");
        let inspection = inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &state,
        );
        assert!(inspection.conflicts[0].approved);
    }

    #[test]
    fn conflict_commands_are_sanitized_before_display() {
        let home = Home::new();
        home.write_settings(
            "{\"hooks\": {\"PostToolUse\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"\\u001b[31mevil\"}]}]}}",
        );
        let inspection = inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &State::default(),
        );
        assert_eq!(inspection.conflicts[0].command, "\\e[31mevil");
    }

    #[test]
    fn a_disabled_or_absent_installation_is_reported_as_absent() {
        let home = Home::new();
        let inspection = inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &State::default(),
        );
        assert_eq!(inspection.installed, Installed::Absent);
        assert!(inspection.conflicts.is_empty());

        home.write_settings(r#"{"hooks": {}}"#);
        let inspection = inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &State::default(),
        );
        assert_eq!(inspection.installed, Installed::Absent);
    }

    #[test]
    fn detection_uses_the_executable_or_the_configuration_directory() {
        let home = Home::new();
        let detected = inspect(
            &Environment::default(),
            &home.root,
            Some(&home.executable()),
            &State::default(),
        );
        assert_eq!(detected.detection, Detection::Detected);

        let elsewhere = home.root.join("no-claude-here");
        std::fs::create_dir_all(&elsewhere).expect("directory");
        let undetected = inspect(&Environment::default(), &elsewhere, None, &State::default());
        assert_eq!(undetected.detection, Detection::NotDetected);
    }

    #[test]
    fn managed_commands_are_recognized_only_in_their_exact_shape() {
        assert!(is_managed_command("/opt/bin/secretsieve hook claude"));
        assert!(is_managed_command("'/opt/my bin/secretsieve' hook claude"));
        assert!(!is_managed_command("/opt/bin/secretsieve hook codex"));
        assert!(!is_managed_command("relative/secretsieve hook claude"));
        assert!(!is_managed_command(" hook claude"));
        assert!(!is_managed_command("secretsieve"));
        assert!(!is_managed_command(
            "/opt/bin/secretsieve hook claude --extra"
        ));
    }

    #[test]
    fn removing_an_installation_prunes_only_containers_it_created() {
        let home = Home::new();
        let mut state = State::default();
        install(&home.root, &home.executable(), &mut state).expect("install");
        remove(&home.root, &mut state).expect("remove");
        let settings = parsed(&home.read_settings());
        assert!(settings.get("hooks").is_none());
    }
}
