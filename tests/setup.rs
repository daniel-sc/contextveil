//! Filesystem tests for the interactive setup workflow (`TST-003`).
//!
//! Each test drives `setup::run` with a scripted transcript inside an isolated
//! home and project, so no developer configuration is read or written.

use std::path::{Path, PathBuf};

use secretsieve::cli::Exit;
use secretsieve::setup;
use secretsieve::setup::ui::Terminal;
use secretsieve::source::Environment;
use secretsieve::testing::{Canary, assert_canary_absent};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "secretsieve-setup-{}-{}",
            std::process::id(),
            Canary::generate("SETUP").token()
        ));
        std::fs::create_dir_all(root.join("home")).expect("home");
        std::fs::create_dir_all(root.join("home").join("project")).expect("project");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn project(&self) -> PathBuf {
        self.home().join("project")
    }

    fn global_config(&self) -> PathBuf {
        self.home()
            .join(".config")
            .join("secretsieve")
            .join("config.toml")
    }

    fn project_config(&self) -> PathBuf {
        self.project().join(".secretsieve.toml")
    }

    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.project().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("directories");
        }
        std::fs::write(&path, contents).expect("write file");
        path
    }

    fn environment(&self, pairs: &[(&str, &str)]) -> Environment {
        let mut variables = vec![(
            "HOME".to_string(),
            self.home().to_string_lossy().into_owned(),
        )];
        variables.extend(
            pairs
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string())),
        );
        Environment::from_pairs(variables)
    }

    /// Runs setup with a scripted transcript and returns the exit code and the
    /// complete terminal output.
    fn run(&self, script: &str, environment: &Environment) -> (Exit, String) {
        self.run_from(script, environment, &self.project())
    }

    fn run_from(
        &self,
        script: &str,
        environment: &Environment,
        directory: &Path,
    ) -> (Exit, String) {
        let mut output: Vec<u8> = Vec::new();
        let exit = {
            let mut terminal = Terminal::new(std::io::Cursor::new(script.to_string()), &mut output);
            setup::run(&mut terminal, environment, directory)
        };
        (exit, String::from_utf8(output).expect("UTF-8 transcript"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Accepts the default selection in both enrollment phases.
const ACCEPT_BOTH: &str = "\n\n";

#[test]
fn a_gated_environment_candidate_is_enrolled_by_default() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let fixture = Fixture::new();
    let environment = fixture.environment(&[
        ("GITHUB_TOKEN", canary.value()),
        ("EDITOR", "vi"),
        ("PATH", "/usr/bin"),
    ]);

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &environment);
    assert_eq!(exit, Exit::Ok, "transcript:\n{transcript}");

    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("GITHUB_TOKEN"));
    assert!(
        !global.contains("EDITOR"),
        "ungated names must not be enrolled"
    );
    // `CFG-003`: the project file exists even with no project sources.
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.starts_with("version = 1"));

    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("global config", global.as_bytes(), &canary);
}

#[test]
fn setup_shows_a_masked_preview_and_its_reason() {
    let canary = Canary::generate_with_length("API_KEY", 40);
    let fixture = Fixture::new();
    let environment = fixture.environment(&[("API_KEY", canary.value())]);

    let (_, transcript) = fixture.run(ACCEPT_BOTH, &environment);
    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
    assert!(transcript.contains("(40 characters)"));
    assert!(transcript.contains("name contains `key`"));
    // First and last four characters only, per `SET-010`.
    let revealed: String = canary.value().chars().take(4).collect();
    assert!(transcript.contains(&revealed));
    let hidden: String = canary.value().chars().skip(6).take(10).collect();
    assert!(!transcript.contains(&hidden));
}

#[test]
fn rerunning_setup_with_no_changes_is_idempotent() {
    let canary = Canary::generate("STRIPE_SECRET");
    let fixture = Fixture::new();
    let environment = fixture.environment(&[("STRIPE_SECRET", canary.value())]);

    assert_eq!(fixture.run(ACCEPT_BOTH, &environment).0, Exit::Ok);
    let first_global = std::fs::read(fixture.global_config()).expect("global config");
    let first_project = std::fs::read(fixture.project_config()).expect("project config");

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &environment);
    assert_eq!(exit, Exit::Ok);
    assert_eq!(
        std::fs::read(fixture.global_config()).expect("global config"),
        first_global
    );
    assert_eq!(
        std::fs::read(fixture.project_config()).expect("project config"),
        first_project
    );
    assert!(transcript.contains("No change"));
}

#[test]
fn cancelling_the_first_phase_writes_nothing() {
    let fixture = Fixture::new();
    let environment = fixture.environment(&[("API_TOKEN", "value")]);

    let (exit, _) = fixture.run("q\n", &environment);
    assert_eq!(exit, Exit::Failure);
    assert!(!fixture.global_config().exists());
    assert!(!fixture.project_config().exists());
}

#[test]
fn ending_input_cancels_without_writing() {
    let fixture = Fixture::new();
    let environment = fixture.environment(&[("API_TOKEN", "value")]);

    let (exit, _) = fixture.run("", &environment);
    assert_eq!(exit, Exit::Failure);
    assert!(!fixture.global_config().exists());
}

#[test]
fn an_invalid_existing_config_is_preserved_byte_for_byte() {
    let fixture = Fixture::new();
    let invalid = "version = 1\n\n[[secret]]\nsource = \"unknown\"\nname = \"A\"\n";
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent")).expect("directory");
    std::fs::write(fixture.global_config(), invalid).expect("write invalid config");

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Failure);
    assert_eq!(
        std::fs::read_to_string(fixture.global_config()).expect("read back"),
        invalid
    );
    // `CFG-014`: no other file is created either.
    assert!(!fixture.project_config().exists());
    assert!(transcript.contains("not a valid SecretSieve configuration"));
    assert!(transcript.contains("made no change"));
}

#[test]
fn an_invalid_project_config_stops_setup_before_the_global_phase() {
    let fixture = Fixture::new();
    let invalid = "version = 2\n";
    std::fs::write(fixture.project_config(), invalid).expect("write invalid project config");

    let (exit, _) = fixture.run(ACCEPT_BOTH, &fixture.environment(&[("API_TOKEN", "v")]));
    assert_eq!(exit, Exit::Failure);
    assert!(!fixture.global_config().exists());
    assert_eq!(
        std::fs::read_to_string(fixture.project_config()).expect("read back"),
        invalid
    );
}

#[test]
fn project_dotenv_keys_are_discovered_and_gated() {
    let canary = Canary::generate("SERVICE_TOKEN");
    let fixture = Fixture::new();
    fixture.write(
        ".env.local",
        &format!("SERVICE_TOKEN={}\nLOG_LEVEL=debug\n", canary.value()),
    );

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");

    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("SERVICE_TOKEN"));
    assert!(project.contains(".env.local"));
    assert!(
        !project.contains("LOG_LEVEL"),
        "ungated keys are not enrolled"
    );
    assert_canary_absent("project config", project.as_bytes(), &canary);
    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
}

#[test]
fn a_colliding_candidate_is_visible_but_unselected() {
    let fixture = Fixture::new();
    // A short, common value that also appears in a tracked file.
    fixture.write(".env", "APP_SECRET=common\n");
    fixture.write("src/config.rs", "let default = \"common\";\n");

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("collision:"));
    assert!(transcript.contains("src/config.rs"));

    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    // `SET-007`: shown, but not enrolled without an explicit choice.
    assert!(!project.contains("APP_SECRET"));
}

#[test]
fn a_collision_can_be_overridden_by_the_user() {
    // `SET-008`: the user is authoritative.
    let fixture = Fixture::new();
    fixture.write(".env", "APP_SECRET=common\n");
    fixture.write("notes.txt", "common\n");

    let (exit, _) = fixture.run("\n1\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok);
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("APP_SECRET"));
}

#[test]
fn wildcard_enrollment_requires_an_extra_confirmation() {
    let fixture = Fixture::new();
    fixture.write(".env.shared", "A_TOKEN=one\nB=two\n");

    // Decline the confirmation: nothing is added.
    let (exit, transcript) = fixture.run("\nw\n.env.shared\nn\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("every current and future key"));
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(!project.contains("all = true"));

    // Accept it: the wildcard entry is stored with the path as entered.
    let fixture = Fixture::new();
    fixture.write(".env.shared", "A_TOKEN=one\nB=two\n");
    let (exit, _) = fixture.run("\nw\n.env.shared\ny\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok);
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("all = true"));
    assert!(project.contains("\".env.shared\""));
}

#[test]
fn an_unresolved_manual_source_requires_confirmation() {
    let fixture = Fixture::new();

    // Decline: not saved.
    let (exit, transcript) = fixture.run("e\nABSENT_TOKEN\nn\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("currently unresolved"));
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(!global.contains("ABSENT_TOKEN"));

    // Accept: saved even though it does not resolve yet (`SET-005`).
    let fixture = Fixture::new();
    let (exit, _) = fixture.run("e\nABSENT_TOKEN\ny\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok);
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("ABSENT_TOKEN"));
}

#[test]
fn existing_enrollment_survives_a_rerun_even_when_unresolved() {
    // `CFG-015`: an entry is never removed just because it does not resolve.
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent")).expect("directory");
    std::fs::write(
        fixture.global_config(),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"ROTATED_TOKEN\"\n",
    )
    .expect("write existing config");

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("ROTATED_TOKEN"));
    assert!(transcript.contains("(enrolled)"));
}

#[test]
fn an_enrolled_entry_can_be_removed_deliberately() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent")).expect("directory");
    std::fs::write(
        fixture.global_config(),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"OLD_TOKEN\"\n",
    )
    .expect("write existing config");

    let (exit, _) = fixture.run("1\n\n\n", &fixture.environment(&[("OLD_TOKEN", "value")]));
    assert_eq!(exit, Exit::Ok);
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(!global.contains("OLD_TOKEN"));
}

#[test]
fn an_enrolled_malformed_source_must_be_repaired_or_removed() {
    // `SET-013`: setup cannot complete while an enrolled source is malformed.
    let fixture = Fixture::new();
    fixture.write(".env.broken", "A=1\nnot an assignment\n");
    std::fs::write(
        fixture.project_config(),
        "version = 1\n\n[[secret]]\nsource = \"dotenv\"\nfile = \".env.broken\"\nkey = \"A\"\n",
    )
    .expect("write project config");

    // Trying to save without removing it is refused, then removal succeeds.
    let (exit, transcript) = fixture.run("\n\n1\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("must be repaired or deselected"));
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(!project.contains(".env.broken"));
}

#[test]
fn an_unavailable_discovered_file_does_not_stop_discovery() {
    let canary = Canary::generate("GOOD_TOKEN");
    let fixture = Fixture::new();
    fixture.write(".env.broken", "unparseable line\n");
    fixture.write(".env", &format!("GOOD_TOKEN={}\n", canary.value()));

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("GOOD_TOKEN"));
    assert!(!project.contains(".env.broken"));
}

#[test]
#[cfg(unix)]
fn a_non_utf8_path_is_reported_and_never_persisted() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let fixture = Fixture::new();
    let name = OsString::from_vec(vec![b'.', b'e', b'n', b'v', b'.', 0xff]);
    std::fs::write(fixture.project().join(&name), "API_TOKEN=value\n").expect("write file");

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(!project.contains("\\xff"));
    assert!(!project.contains("API_TOKEN"));
    assert!(!transcript.contains('\u{fffd}'));
}

#[test]
#[cfg(unix)]
fn a_project_phase_failure_keeps_the_committed_global_phase() {
    // `SET-014`: a completed phase stays committed when a later phase fails.
    use std::os::unix::fs::PermissionsExt;

    let canary = Canary::generate("KEEP_TOKEN");
    let fixture = Fixture::new();
    // A read-only project directory makes the project write fail while the
    // global write, which lives elsewhere, still succeeds.
    std::fs::set_permissions(fixture.project(), std::fs::Permissions::from_mode(0o500))
        .expect("make the project directory read-only");
    let writable = std::fs::write(fixture.project().join(".probe"), "x").is_ok();

    let environment = fixture.environment(&[("KEEP_TOKEN", canary.value())]);
    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &environment);
    let _ = std::fs::set_permissions(fixture.project(), std::fs::Permissions::from_mode(0o700));

    if writable {
        // A privileged test runner ignores the permission bits.
        return;
    }
    assert_eq!(exit, Exit::Failure, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("KEEP_TOKEN"));
    assert!(transcript.contains("could not be written"));
    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
}

#[test]
fn the_project_root_is_selected_from_the_working_directory() {
    // `CFG-003`: the enclosing Git worktree root is used when no project config
    // exists yet.
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.project().join(".git")).expect("git directory");
    let nested = fixture.project().join("packages").join("app");
    std::fs::create_dir_all(&nested).expect("nested directory");

    let (exit, _) = fixture.run_from(ACCEPT_BOTH, &fixture.environment(&[]), &nested);
    assert_eq!(exit, Exit::Ok);
    assert!(fixture.project_config().exists());
    assert!(!nested.join(".secretsieve.toml").exists());
}

#[test]
fn global_dotenv_probing_covers_the_documented_locations() {
    let canary = Canary::generate("HARNESS_TOKEN");
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.home().join(".claude")).expect("claude directory");
    std::fs::write(
        fixture.home().join(".claude").join(".env"),
        format!("HARNESS_TOKEN={}\n", canary.value()),
    )
    .expect("write harness dotenv");

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("~/.claude/.env"));
    assert!(global.contains("HARNESS_TOKEN"));
    assert_canary_absent("global config", global.as_bytes(), &canary);
}

#[test]
fn the_transcript_never_contains_a_full_value_or_a_fingerprint() {
    let canary = Canary::generate_with_length("MASTER_PASSWORD", 30);
    let fixture = Fixture::new();
    fixture.write(".env", &format!("MASTER_PASSWORD={}\n", canary.value()));
    let environment = fixture.environment(&[("MASTER_PASSWORD", canary.value())]);

    let (_, transcript) = fixture.run(ACCEPT_BOTH, &environment);
    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
    // No deterministic fingerprint is shown either (`SET-010`).
    assert!(!transcript.contains("sha"));
    assert!(!transcript.contains("hash"));
}

#[test]
fn terminal_escapes_in_names_and_paths_are_neutralized() {
    let fixture = Fixture::new();
    let hostile = "\u{1b}[31mAPI_TOKEN";
    let environment = fixture.environment(&[(hostile, "value")]);

    let (_, transcript) = fixture.run(ACCEPT_BOTH, &environment);
    assert!(!transcript.contains('\u{1b}'));
    assert!(transcript.contains("\\e[31mAPI_TOKEN"));
}

#[test]
fn duplicate_dotenv_keys_are_warned_about_without_values() {
    let canary = Canary::generate("DUPLICATE_TOKEN");
    let fixture = Fixture::new();
    fixture.write(
        ".env",
        &format!(
            "DUPLICATE_TOKEN=first\nDUPLICATE_TOKEN={}\n",
            canary.value()
        ),
    );

    let (exit, transcript) = fixture.run(ACCEPT_BOTH, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("more than once"));
    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
}
