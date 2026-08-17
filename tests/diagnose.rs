//! Exit-code matrix for `status` and `doctor` (`CLI-005` through `CLI-007`,
//! `DIA-008`).
//!
//! These drive the built binary so the observable public contract is exercised.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use secretsieve::testing::{Canary, assert_canary_absent};

struct Machine {
    root: PathBuf,
}

impl Machine {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "secretsieve-diag-e2e-{}-{}",
            std::process::id(),
            Canary::generate("DIAG").token()
        ));
        std::fs::create_dir_all(root.join("home").join("project")).expect("project");
        std::fs::create_dir_all(root.join("home").join(".config").join("secretsieve"))
            .expect("config directory");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn project(&self) -> PathBuf {
        self.home().join("project")
    }

    fn write_global(&self, contents: &str) {
        std::fs::write(
            self.home()
                .join(".config")
                .join("secretsieve")
                .join("config.toml"),
            contents,
        )
        .expect("write global config");
    }

    fn write_record(&self, contents: &str) {
        std::fs::write(
            self.home()
                .join(".config")
                .join("secretsieve")
                .join("integrations.toml"),
            contents,
        )
        .expect("write integration record");
    }

    /// Installs a hook entry pointing at the real test binary.
    fn install_hook(&self, extra_groups: &str) {
        std::fs::create_dir_all(self.home().join(".claude")).expect("claude directory");
        let binary = env!("CARGO_BIN_EXE_secretsieve");
        std::fs::write(
            self.home().join(".claude").join("settings.json"),
            format!(
                r#"{{"hooks": {{"PostToolUse": [
                    {{"matcher": "*", "hooks": [{{"type": "command", "command": "{binary} hook claude", "timeout": 5}}]}}{extra_groups}
                ]}}}}"#
            ),
        )
        .expect("write settings");
    }

    fn run(&self, command: &str, variables: &[(&str, &str)]) -> Output {
        let mut process = Command::new(env!("CARGO_BIN_EXE_secretsieve"));
        process
            .arg(command)
            .current_dir(self.project())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in variables {
            process.env(key, value);
        }
        process.output().expect("the binary runs")
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("UTF-8 stdout")
}

#[test]
fn a_healthy_machine_exits_zero_for_both_commands() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let machine = Machine::new();
    machine.write_global("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"GITHUB_TOKEN\"\n");
    machine.install_hook("");

    let status = machine.run("status", &[("GITHUB_TOKEN", canary.value())]);
    assert_eq!(status.status.code(), Some(0));
    assert_canary_absent("status stdout", &status.stdout, &canary);

    let doctor = machine.run("doctor", &[("GITHUB_TOKEN", canary.value())]);
    assert_eq!(doctor.status.code(), Some(0), "{}", text(&doctor));
    assert!(text(&doctor).contains("the synthetic protocol check passed"));
    assert_canary_absent("doctor stdout", &doctor.stdout, &canary);
    assert_canary_absent("doctor stderr", &doctor.stderr, &canary);
}

#[test]
fn a_partially_unresolved_registry_is_healthy() {
    let canary = Canary::generate("PRESENT_TOKEN");
    let machine = Machine::new();
    machine.write_global(
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"PRESENT_TOKEN\"\n\n[[secret]]\nsource = \"env\"\nname = \"ABSENT_TOKEN\"\n",
    );
    machine.install_hook("");

    let doctor = machine.run("doctor", &[("PRESENT_TOKEN", canary.value())]);
    assert_eq!(doctor.status.code(), Some(0), "{}", text(&doctor));
    assert!(text(&doctor).contains("[warn] env ABSENT_TOKEN is enrolled but is not present"));
}

#[test]
fn a_fully_inactive_registry_is_a_health_failure() {
    let machine = Machine::new();
    machine.write_global("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"ABSENT\"\n");
    machine.install_hook("");

    let status = machine.run("status", &[]);
    assert_eq!(status.status.code(), Some(0));
    assert!(text(&status).contains("INACTIVE"));

    let doctor = machine.run("doctor", &[]);
    assert_eq!(doctor.status.code(), Some(1));
}

#[test]
fn malformed_configuration_fails_doctor_but_not_status() {
    let machine = Machine::new();
    machine.write_global("version = 1\n\n[[secret]]\nsource = \"nope\"\n");
    machine.install_hook("");

    assert_eq!(machine.run("status", &[]).status.code(), Some(0));
    let doctor = machine.run("doctor", &[]);
    assert_eq!(doctor.status.code(), Some(1));
    assert!(text(&doctor).contains("configuration is unusable"));
}

#[test]
fn an_approved_conflict_stays_healthy_but_visible() {
    // `INT-005`, `CLA-005`: an approved conflict is reported, not a failure.
    let canary = Canary::generate("TOKEN");
    let machine = Machine::new();
    machine.write_global("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"TOKEN\"\n");
    machine.install_hook(
        r#", {"matcher": "*", "hooks": [{"type": "command", "command": "/other/mutator"}]}"#,
    );
    let binary = env!("CARGO_BIN_EXE_secretsieve");
    machine.write_record(&format!(
        "version = 1\n\n[claude]\ncommand = \"{binary} hook claude\"\napproved_conflicts = [\"/other/mutator\"]\n"
    ));

    let doctor = machine.run("doctor", &[("TOKEN", canary.value())]);
    assert_eq!(doctor.status.code(), Some(0), "{}", text(&doctor));
    assert!(text(&doctor).contains("an approved hook can also change tool results"));
}

#[test]
fn an_unapproved_conflict_is_a_health_failure() {
    let canary = Canary::generate("TOKEN");
    let machine = Machine::new();
    machine.write_global("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"TOKEN\"\n");
    machine.install_hook(
        r#", {"matcher": "*", "hooks": [{"type": "command", "command": "/other/mutator"}]}"#,
    );

    let doctor = machine.run("doctor", &[("TOKEN", canary.value())]);
    assert_eq!(doctor.status.code(), Some(1));
    assert!(text(&doctor).contains("an unapproved hook can also change tool results"));
}

#[test]
fn a_missing_integration_is_a_health_failure() {
    let canary = Canary::generate("TOKEN");
    let machine = Machine::new();
    machine.write_global("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"TOKEN\"\n");

    let status = machine.run("status", &[("TOKEN", canary.value())]);
    assert_eq!(status.status.code(), Some(0));
    assert!(text(&status).contains("not installed"));

    let doctor = machine.run("doctor", &[("TOKEN", canary.value())]);
    assert_eq!(doctor.status.code(), Some(1));
    assert!(text(&doctor).contains("no coding-agent integration is installed"));
}

#[test]
fn an_inspection_that_cannot_complete_exits_two() {
    // No HOME and no XDG_CONFIG_HOME, so no configuration location exists.
    let machine = Machine::new();
    let mut process = Command::new(env!("CARGO_BIN_EXE_secretsieve"));
    let output = process
        .arg("doctor")
        .current_dir(machine.project())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));

    let status = Command::new(env!("CARGO_BIN_EXE_secretsieve"))
        .arg("status")
        .current_dir(machine.project())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    assert_eq!(status.status.code(), Some(2));
}

#[test]
fn status_runs_no_adapter_protocol_test() {
    // `DIA-001`: status must not spawn the hook. A hook path that does not exist
    // would make a protocol test fail, yet status still exits zero and says
    // nothing about a protocol check.
    let canary = Canary::generate("TOKEN");
    let machine = Machine::new();
    machine.write_global("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"TOKEN\"\n");
    std::fs::create_dir_all(machine.home().join(".claude")).expect("claude directory");
    std::fs::write(
        machine.home().join(".claude").join("settings.json"),
        r#"{"hooks": {"PostToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/gone/secretsieve hook claude", "timeout": 5}]}]}}"#,
    )
    .expect("write settings");

    let status = machine.run("status", &[("TOKEN", canary.value())]);
    assert_eq!(status.status.code(), Some(0));
    let output = text(&status);
    assert!(!output.contains("protocol check"));

    // Doctor does run it, and fails because the executable is gone.
    let doctor = machine.run("doctor", &[("TOKEN", canary.value())]);
    assert_eq!(doctor.status.code(), Some(1));
    assert!(text(&doctor).contains("the configured executable is missing"));
}

#[test]
fn doctor_is_not_offered_the_live_canary_without_a_terminal() {
    // `DIA-005`: disabled by default. Without a TTY there is nothing to confirm,
    // so no network call can happen (`SEC-003`).
    let machine = Machine::new();
    machine.write_global("version = 1\n");
    let doctor = machine.run("doctor", &[]);
    assert!(!text(&doctor).contains("live canary"));
}

#[test]
fn diagnostics_contain_no_source_content() {
    let canary = Canary::generate("FILE_TOKEN");
    let machine = Machine::new();
    std::fs::write(
        machine.project().join(".env"),
        format!("FILE_TOKEN={}\nOTHER=plain\n", canary.value()),
    )
    .expect("write dotenv");
    machine.write_global("version = 1\n");
    std::fs::write(
        machine.project().join(".secretsieve.toml"),
        "version = 1\n\n[[secret]]\nsource = \"dotenv\"\nfile = \".env\"\nall = true\n",
    )
    .expect("write project config");
    machine.install_hook("");

    for command in ["status", "doctor"] {
        let output = machine.run(command, &[]);
        assert_canary_absent(&format!("{command} stdout"), &output.stdout, &canary);
        assert_canary_absent(&format!("{command} stderr"), &output.stderr, &canary);
        assert!(!text(&output).contains("plain"));
    }
}

#[test]
fn the_project_root_follows_the_working_directory() {
    // `DIA-001` with `CFG-003`: the nearest ancestor project config is selected.
    let machine = Machine::new();
    machine.write_global("version = 1\n");
    std::fs::write(
        machine.project().join(".secretsieve.toml"),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"PROJECT_TOKEN\"\n",
    )
    .expect("write project config");
    let nested = machine.project().join("deep").join("nested");
    std::fs::create_dir_all(&nested).expect("nested directories");

    let output = Command::new(env!("CARGO_BIN_EXE_secretsieve"))
        .arg("status")
        .current_dir(&nested)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", machine.home())
        .env("PROJECT_TOKEN", "value")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(text.contains(".secretsieve.toml"));
    assert!(text.contains("active          1 value(s)"));
}

/// Sanity check that the fixtures above really point at a usable binary.
#[test]
fn the_test_binary_path_is_absolute() {
    assert!(Path::new(env!("CARGO_BIN_EXE_secretsieve")).is_absolute());
}
