//! Filesystem tests for the interactive setup workflow (`TST-003`).
//!
//! Each test drives `setup::run` with a scripted transcript inside an isolated
//! home and project, so no developer configuration is read or written.
//!
//! With the config and source unit tests, these cover the `TST-002` and `TST-003`
//! cases: strict fields, duplicate identities, cross-scope duplicates, missing
//! sources, recursive discovery and its exclusions, permissions, atomic writes,
//! invalid-config preservation, repeat setup, and partial multi-phase failure.

use std::path::{Path, PathBuf};

use contextveil::cli::Exit;
use contextveil::registry;
use contextveil::secret::SourceId;
use contextveil::setup;
use contextveil::setup::ui::Terminal;
use contextveil::source::Environment;
use contextveil::testing::{Canary, assert_canary_absent};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "contextveil-setup-{}-{}",
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
            .join("contextveil")
            .join("config.toml")
    }

    fn project_config(&self) -> PathBuf {
        self.project().join(".contextveil.toml")
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
        self.run_with_executable(
            script,
            environment,
            directory,
            Path::new(env!("CARGO_BIN_EXE_contextveil")),
        )
    }

    fn run_with_executable(
        &self,
        script: &str,
        environment: &Environment,
        directory: &Path,
        executable: &Path,
    ) -> (Exit, String) {
        let mut output: Vec<u8> = Vec::new();
        let exit = {
            let mut terminal = Terminal::new(std::io::Cursor::new(script.to_string()), &mut output);
            setup::run(&mut terminal, environment, directory, Some(executable))
        };
        (exit, String::from_utf8(output).expect("UTF-8 transcript"))
    }

    #[cfg(unix)]
    fn executable_failing(&self, harness: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = self.root.join(format!("contextveil-fail-{harness}"));
        let real = env!("CARGO_BIN_EXE_contextveil").replace('\'', "'\\''");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"$1 $2\" = \"hook {harness}\" ]; then exit 1; fi\nexec '{real}' \"$@\"\n"
            ),
        )
        .expect("verification wrapper");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("executable wrapper");
        path
    }

    fn claude_settings(&self) -> PathBuf {
        self.home().join(".claude").join("settings.json")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Accepts the defaults in both enrollment phases and the integration phase.
const ACCEPT_ALL: &str = "\n\n\n";

#[test]
fn a_gated_environment_candidate_is_enrolled_by_default() {
    let canary = Canary::generate("GITHUB_TOKEN");
    let fixture = Fixture::new();
    let environment = fixture.environment(&[
        ("GITHUB_TOKEN", canary.value()),
        ("EDITOR", "vi"),
        ("PATH", "/usr/bin"),
    ]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
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
fn an_unresolved_automatic_candidate_remains_selected() {
    let fixture = Fixture::new();
    let environment = fixture.environment(&[("EMPTY_TOKEN", "")]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("[x] env EMPTY_TOKEN"), "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("EMPTY_TOKEN"), "{global}");
}

#[test]
fn a_database_url_candidate_is_enrolled_as_its_environment_source() {
    let canary = Canary::generate("DATABASE_URL_PASSWORD");
    let fixture = Fixture::new();
    let url = format!("postgresql://app:{}@db.example.test/app", canary.value());
    let environment = fixture.environment(&[("DATABASE_URL", &url)]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("credential-bearing URL"));

    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("DATABASE_URL"));
    assert!(!global.contains("postgresql://"));
    assert_canary_absent("URL setup transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("URL global config", global.as_bytes(), &canary);
}

#[test]
fn credential_url_admission_uses_trimmed_environment_and_dotenv_values() {
    let env_canary = Canary::generate("TRIMMED_ENV_URL_PASSWORD");
    let file_canary = Canary::generate("TRIMMED_DOTENV_URL_PASSWORD");
    let fixture = Fixture::new();
    let env_url = format!(
        "  postgresql://app:{}@env.example.test/app  ",
        env_canary.value()
    );
    let file_url = format!(
        "  postgresql://app:{}@file.example.test/app  ",
        file_canary.value()
    );
    std::fs::write(
        fixture.project().join(".env"),
        format!("SERVICE_ENDPOINT='{file_url}'\n"),
    )
    .expect("dotenv fixture");
    let environment = fixture.environment(&[("SERVICE_ENDPOINT", &env_url)]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(transcript.matches("credential-bearing URL").count(), 2);
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(global.contains("SERVICE_ENDPOINT"));
    assert!(project.contains("SERVICE_ENDPOINT"));
    assert_canary_absent(
        "trimmed URL setup transcript",
        transcript.as_bytes(),
        &env_canary,
    );
    assert_canary_absent(
        "trimmed URL setup transcript",
        transcript.as_bytes(),
        &file_canary,
    );
}

#[test]
fn rule_count_does_not_change_candidate_order() {
    let fixture = Fixture::new();
    let environment = fixture.environment(&[
        ("A_TOKEN", "aaaaaaaaaaaaaaaaaaaaaaaa:"),
        ("Z_PASSWORD", "https://user:x@z.example.test"),
    ]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(
        transcript
            .find("env A_TOKEN")
            .expect("single-rule candidate")
            < transcript
                .find("env Z_PASSWORD")
                .expect("multi-rule candidate"),
        "rule count changed candidate order:\n{transcript}"
    );
    assert_eq!(transcript.matches("credential-bearing URL").count(), 1);
    assert_eq!(transcript.matches("secret-like source name").count(), 2);
    assert!(
        transcript.contains("rules: secret-like source name, credential-bearing URL"),
        "{transcript}"
    );
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("A_TOKEN"));
    assert!(global.contains("Z_PASSWORD"));
}

#[test]
fn value_shape_does_not_change_candidate_order() {
    let fixture = Fixture::new();
    let environment = fixture.environment(&[
        ("A_TOKEN", "short"),
        ("Z_TOKEN", "aB3aB3aB3aB3aB3aB3aB3aB3aB3aB3aB3aB3"),
    ]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(
        transcript.find("env A_TOKEN").expect("short candidate")
            < transcript.find("env Z_TOKEN").expect("shaped candidate"),
        "value shape changed candidate order:\n{transcript}"
    );
}

#[test]
fn overlapping_known_source_rules_are_all_attributed() {
    let fixture = Fixture::new();
    let environment = fixture.environment(&[(
        "OPENCODE_AUTH_CONTENT",
        "https://user:password@example.test",
    )]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(
        transcript.contains("credential-bearing URL"),
        "{transcript}"
    );
    assert!(
        transcript.contains("OpenCode whole environment credential content"),
        "{transcript}"
    );
}

#[test]
fn non_credential_url_shapes_do_not_bypass_name_gating() {
    let canary = Canary::generate("REJECTED_URL_PASSWORD");
    let fixture = Fixture::new();
    let relative = format!("//user:{}@example.test/path", canary.value());
    let opaque = format!("scheme:user:{}@example.test", canary.value());
    let environment = fixture.environment(&[
        ("RELATIVE_URL", &relative),
        ("OPAQUE_URI", &opaque),
        ("HOST_URL", "https://example.test/path"),
        ("USERINFO_URL", "https://user@example.test/path"),
        ("EMPTY_USERINFO_URL", "https://user:@example.test/path"),
    ]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    for name in [
        "RELATIVE_URL",
        "OPAQUE_URI",
        "HOST_URL",
        "USERINFO_URL",
        "EMPTY_USERINFO_URL",
    ] {
        assert!(!global.contains(name), "{name} must not be enrolled");
    }
    assert_canary_absent("rejected URL transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("rejected URL config", global.as_bytes(), &canary);
}

#[test]
fn setup_shows_a_masked_preview_and_rules_without_shape_details() {
    let canary = Canary::generate_with_length("API_KEY", 40);
    let fixture = Fixture::new();
    let environment = fixture.environment(&[("API_KEY", canary.value())]);

    let (_, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
    assert!(transcript.contains("(40 characters)"));
    assert!(transcript.contains("rules: secret-like source name"));
    for removed in [
        "long value",
        "mixed character classes",
        "encoded-looking value",
    ] {
        assert!(!transcript.contains(removed), "{transcript}");
    }
    // First and last four characters only, per `SET-010`.
    let revealed: String = canary.value().chars().take(4).collect();
    assert!(transcript.contains(&revealed));
    let hidden: String = canary.value().chars().skip(6).take(10).collect();
    assert!(!transcript.contains(&hidden));
}

#[test]
fn selection_screen_rerenders_after_every_continuing_interaction() {
    let fixture = Fixture::new();
    let environment = fixture.environment(&[("API_TOKEN", "value"), ("MANUAL", "other")]);

    let (exit, transcript) = fixture.run(
        "not-a-choice\nw\nmissing.env\nn\ne\nMANUAL\n\n\n\n",
        &environment,
    );
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(transcript.matches("Choose an action:").count(), 6);
}

#[test]
fn rerunning_setup_with_no_changes_is_idempotent() {
    let canary = Canary::generate("STRIPE_SECRET");
    let fixture = Fixture::new();
    let environment = fixture.environment(&[("STRIPE_SECRET", canary.value())]);

    assert_eq!(fixture.run(ACCEPT_ALL, &environment).0, Exit::Ok);
    let first_global = std::fs::read(fixture.global_config()).expect("global config");
    let first_project = std::fs::read(fixture.project_config()).expect("project config");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
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
fn initial_rows_use_existing_then_new_tiers_but_save_uses_identity_order() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent"))
        .expect("config directory");
    std::fs::write(
        fixture.global_config(),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"Z_TOKEN\"\n",
    )
    .expect("global config");
    let environment = fixture.environment(&[("A_TOKEN", "a-value"), ("Z_TOKEN", "z-value")]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(
        transcript
            .find("env Z_TOKEN (enrolled)")
            .expect("existing row")
            < transcript.find("env A_TOKEN").expect("new row"),
        "{transcript}"
    );

    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(
        global.find("A_TOKEN").expect("A identity") < global.find("Z_TOKEN").expect("Z identity"),
        "{global}"
    );
}

#[test]
fn save_normalizes_source_kinds_and_normalized_identity_fields() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent"))
        .expect("config directory");
    std::fs::write(
        fixture.global_config(),
        r#"version = 1

[[secret]]
source = "json"
file = "z.json"
pointer = "/z"

[[secret]]
source = "dotenv"
file = "0.env"
all = true

[[secret]]
source = "dotenv"
file = "b.env"
key = "A_KEY"

[[secret]]
source = "dotenv"
file = "z/../a.env"
key = "Z_KEY"

[[secret]]
source = "env"
name = "Z_TOKEN"

[[secret]]
source = "env"
name = "A_TOKEN"
"#,
    )
    .expect("global config");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let ordered = [
        "name = \"A_TOKEN\"",
        "name = \"Z_TOKEN\"",
        "file = \"z/../a.env\"",
        "file = \"b.env\"",
        "all = true",
        "pointer = \"/z\"",
    ];
    for pair in ordered.windows(2) {
        assert!(
            global.find(pair[0]).expect("earlier identity")
                < global.find(pair[1]).expect("later identity"),
            "{global}"
        );
    }
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

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Failure);
    assert_eq!(
        std::fs::read_to_string(fixture.global_config()).expect("read back"),
        invalid
    );
    // `CFG-014`: no other file is created either.
    assert!(!fixture.project_config().exists());
    assert!(transcript.contains("not a valid ContextVeil configuration"));
    assert!(transcript.contains("made no change"));
}

#[test]
fn an_invalid_project_config_stops_setup_before_the_global_phase() {
    let fixture = Fixture::new();
    let invalid = "version = 2\n";
    std::fs::write(fixture.project_config(), invalid).expect("write invalid project config");

    let (exit, _) = fixture.run(ACCEPT_ALL, &fixture.environment(&[("API_TOKEN", "v")]));
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

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
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
fn common_literals_are_excluded_from_every_automatic_source_family() {
    let fixture = Fixture::new();
    fixture.write(".env.common", "DOTENV_SECRET=' FaLsE '\n");
    fixture.write(
        ".claude/settings.json",
        r#"{"env":{"ANTHROPIC_API_KEY":" YES "}}"#,
    );
    fixture.write("application.properties", "database.password= No \n");
    fixture.write(".npmrc", "//registry.example.test/:_authToken=\"ON\"\n");
    let environment = fixture.environment(&[("SECRET_ENABLED", " TrUe ")]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");

    for excluded in [
        "SECRET_ENABLED",
        "DOTENV_SECRET",
        "ANTHROPIC_API_KEY",
        "database.password",
        "_authToken",
    ] {
        assert!(!transcript.contains(excluded), "{transcript}");
        assert!(!global.contains(excluded), "{global}");
        assert!(!project.contains(excluded), "{project}");
    }
}

#[test]
fn registry_and_proxy_urls_are_discovered_in_dotenv_files() {
    let registry = Canary::generate("REGISTRY_URL_PASSWORD");
    let proxy = Canary::generate("PROXY_URL_PASSWORD");
    let fixture = Fixture::new();
    fixture.write(
        ".env.urls",
        &format!(
            "REGISTRY=https://publisher:{}@registry.example.test/package\nPROXY=http://agent:{}@proxy.example.test:8080\nLOG_LEVEL=debug\n",
            registry.value(),
            proxy.value()
        ),
    );

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("key = \"REGISTRY\""));
    assert!(project.contains("key = \"PROXY\""));
    assert!(!project.contains("LOG_LEVEL"));
    assert_canary_absent("dotenv URL transcript", transcript.as_bytes(), &registry);
    assert_canary_absent("dotenv URL transcript", transcript.as_bytes(), &proxy);
    assert_canary_absent("dotenv URL config", project.as_bytes(), &registry);
    assert_canary_absent("dotenv URL config", project.as_bytes(), &proxy);
}

#[test]
fn equal_url_candidates_use_the_normal_candidate_group() {
    let canary = Canary::generate("GROUPED_URL_PASSWORD");
    let fixture = Fixture::new();
    let url = format!("https://agent:{}@service.example.test", canary.value());
    let environment = fixture.environment(&[("PRIMARY_URL", &url), ("SECONDARY_URL", &url)]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(transcript.matches("Same current value").count(), 1);
    assert_eq!(transcript.matches("credential-bearing URL").count(), 1);
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("PRIMARY_URL"));
    assert!(global.contains("SECONDARY_URL"));
    assert_canary_absent("grouped URL transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("grouped URL config", global.as_bytes(), &canary);
}

#[test]
fn equal_value_group_toggle_applies_to_every_alias() {
    let canary = Canary::generate("GROUPED_ENV_TOKEN");
    let fixture = Fixture::new();
    let environment = fixture.environment(&[
        ("FIRST_API_TOKEN", canary.value()),
        ("SECOND_API_SECRET", canary.value()),
    ]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(transcript.matches("Same current value").count(), 1);
    assert_eq!(transcript.matches("secret-like source name").count(), 1);
    assert!(transcript.contains("env FIRST_API_TOKEN"));
    assert!(transcript.contains("env SECOND_API_SECRET"));

    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("FIRST_API_TOKEN"));
    assert!(global.contains("SECOND_API_SECRET"));

    let (exit, transcript) = fixture.run("1\n\n\n\n", &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(!global.contains("FIRST_API_TOKEN"));
    assert!(!global.contains("SECOND_API_SECRET"));
    assert_canary_absent("grouped setup transcript", transcript.as_bytes(), &canary);
}

#[test]
fn group_representatives_ignore_discovery_iteration_order() {
    fn run_with_tokens(tokens: &str) -> (String, String) {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.home().join(".copilot")).expect("copilot directory");
        std::fs::write(
            fixture.home().join(".copilot/config.json"),
            format!(r#"{{"copilotTokens":{{{tokens}}}}}"#),
        )
        .expect("copilot config");

        let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
        assert_eq!(exit, Exit::Ok, "{transcript}");
        let config = std::fs::read_to_string(fixture.global_config()).expect("global config");
        (config, transcript)
    }

    let (first, first_transcript) = run_with_tokens(r#""z.example":"same","a.example":"same""#);
    let (second, second_transcript) = run_with_tokens(r#""a.example":"same","z.example":"same""#);

    assert_eq!(first, second);
    for output in [&first, &first_transcript, &second_transcript] {
        assert!(
            output.find("/copilotTokens/a.example").expect("A pointer")
                < output.find("/copilotTokens/z.example").expect("Z pointer"),
            "{output}"
        );
    }
}

#[test]
fn equal_values_in_different_phases_remain_separate_choices() {
    let canary = Canary::generate("CROSS_PHASE_TOKEN");
    let fixture = Fixture::new();
    fixture.write(".env", &format!("PROJECT_TOKEN={}\n", canary.value()));
    let environment = fixture.environment(&[("GLOBAL_TOKEN", canary.value())]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!transcript.contains("Same current value"));
    assert!(!transcript.contains("collision:"));
    assert!(
        std::fs::read_to_string(fixture.global_config())
            .expect("global config")
            .contains("GLOBAL_TOKEN")
    );
    assert!(
        std::fs::read_to_string(fixture.project_config())
            .expect("project config")
            .contains("PROJECT_TOKEN")
    );
}

#[test]
fn an_enrolled_alias_keeps_its_colliding_group_selected() {
    let canary = Canary::generate("PARTIAL_GROUP_TOKEN");
    let fixture = Fixture::new();
    let original = "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"FIRST_TOKEN\"\n";
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent"))
        .expect("config directory");
    std::fs::write(fixture.global_config(), original).expect("global config");
    let environment = fixture.environment(&[
        ("FIRST_TOKEN", canary.value()),
        ("SECOND_TOKEN", canary.value()),
    ]);
    fixture.write("README.md", canary.value());

    let (exit, transcript) = fixture.run("s\n\n\n", &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(
        std::fs::read_to_string(fixture.global_config()).expect("global config"),
        original
    );

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("FIRST_TOKEN"));
    assert!(global.contains("SECOND_TOKEN"));
    assert!(transcript.contains("[x] Same current value (2 sources) (enrolled)"));
}

#[test]
fn a_common_literal_alias_does_not_join_an_existing_enrollment() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent"))
        .expect("config directory");
    std::fs::write(
        fixture.global_config(),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"FIRST_TOKEN\"\n",
    )
    .expect("global config");
    let environment =
        fixture.environment(&[("FIRST_TOKEN", "disabled"), ("SECOND_TOKEN", "disabled")]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(
        transcript.contains("env FIRST_TOKEN (enrolled)"),
        "{transcript}"
    );
    assert!(
        transcript.contains("secret-like source name"),
        "{transcript}"
    );
    assert!(!transcript.contains("SECOND_TOKEN"), "{transcript}");
    assert!(!transcript.contains("Same current value"), "{transcript}");

    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("FIRST_TOKEN"), "{global}");
    assert!(!global.contains("SECOND_TOKEN"), "{global}");
}

#[test]
fn aliases_split_into_separate_rows_after_their_values_diverge() {
    let fixture = Fixture::new();
    let first = fixture.environment(&[("FIRST_TOKEN", "same"), ("SECOND_TOKEN", "same")]);
    assert_eq!(fixture.run(ACCEPT_ALL, &first).0, Exit::Ok);

    let second = fixture.environment(&[("FIRST_TOKEN", "one"), ("SECOND_TOKEN", "two")]);
    let (exit, transcript) = fixture.run(ACCEPT_ALL, &second);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!transcript.contains("Same current value"));
    assert!(transcript.contains("env FIRST_TOKEN (enrolled)"));
    assert!(transcript.contains("env SECOND_TOKEN (enrolled)"));
}

#[test]
fn every_alias_file_is_excluded_but_an_unrelated_collision_remains() {
    let canary = Canary::generate("ALIAS_COLLISION_TOKEN");
    let fixture = Fixture::new();
    fixture.write(".env.one", &format!("FIRST_TOKEN={}\n", canary.value()));
    fixture.write(".env.two", &format!("SECOND_SECRET={}\n", canary.value()));
    fixture.write("README.md", canary.value());

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("Same current value"));
    assert!(transcript.contains("1 occurrence(s) elsewhere"));
    assert!(transcript.contains("README.md x1"));
    assert_canary_absent("alias collision transcript", transcript.as_bytes(), &canary);
}

#[test]
fn a_colliding_candidate_is_visible_but_unselected() {
    let fixture = Fixture::new();
    // A short, common value that also appears in a tracked file.
    fixture.write(".env", "APP_SECRET=common\n");
    fixture.write("src/config.rs", "let default = \"common\";\n");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(
        transcript.contains("[ ] dotenv .env key APP_SECRET"),
        "{transcript}"
    );
    assert!(transcript.contains("collision:"));
    assert!(transcript.contains("src/config.rs"));
    assert!(
        transcript.contains(
            "not selected by default because of this collision; toggle row 1 to enroll anyway"
        ),
        "{transcript}"
    );

    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    // `SET-007`: shown, but not enrolled without an explicit choice.
    assert!(!project.contains("APP_SECRET"));
}

#[test]
fn a_colliding_url_candidate_is_visible_but_unselected() {
    let canary = Canary::generate("COLLIDING_URL_PASSWORD");
    let fixture = Fixture::new();
    let url = format!("https://agent:{}@service.example.test", canary.value());
    fixture.write("README.md", &url);
    let environment = fixture.environment(&[("SERVICE_URL", &url)]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("env SERVICE_URL"));
    assert!(transcript.contains("collision:"));
    assert!(transcript.contains("README.md"));
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(!global.contains("SERVICE_URL"));
    assert_canary_absent("colliding URL transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("colliding URL config", global.as_bytes(), &canary);
}

#[test]
fn a_collision_can_be_overridden_by_the_user() {
    // `SET-008`: the user is authoritative.
    let fixture = Fixture::new();
    fixture.write(".env", "APP_SECRET=common\n");
    fixture.write("notes.txt", "common\n");

    let (exit, transcript) = fixture.run("\n1\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(
        transcript
            .matches("not selected by default because of this collision")
            .count(),
        1,
        "{transcript}"
    );
    assert!(
        transcript.contains("[x] dotenv .env key APP_SECRET"),
        "{transcript}"
    );
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("APP_SECRET"));
}

#[test]
fn toggling_an_ordinary_group_does_not_rescan_the_project() {
    struct WriteBeforeRead {
        script: std::io::Cursor<String>,
        path: PathBuf,
        contents: String,
        written: bool,
    }

    impl std::io::Read for WriteBeforeRead {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if !self.written {
                std::fs::write(&self.path, &self.contents)?;
                self.written = true;
            }
            std::io::Read::read(&mut self.script, buffer)
        }
    }

    let canary = Canary::generate("TOGGLE_SCAN_TOKEN");
    let fixture = Fixture::new();
    let environment = fixture.environment(&[("APP_SECRET", canary.value())]);
    let input = WriteBeforeRead {
        script: std::io::Cursor::new("1\n\n\n\n".to_string()),
        path: fixture.project().join("created-after-initial-scan.txt"),
        contents: canary.value().to_string(),
        written: false,
    };
    let mut output = Vec::new();

    let exit = {
        let mut terminal = Terminal::new(input, &mut output);
        setup::run(
            &mut terminal,
            &environment,
            &fixture.project(),
            Some(Path::new(env!("CARGO_BIN_EXE_contextveil"))),
        )
    };
    let transcript = String::from_utf8(output).expect("UTF-8 transcript");

    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!transcript.contains("collision:"), "{transcript}");
    assert_canary_absent("toggle transcript", transcript.as_bytes(), &canary);
}

#[test]
fn a_manual_collision_warns_without_reversing_selection() {
    let fixture = Fixture::new();
    fixture.write("notes.txt", "common\n");
    let environment = fixture.environment(&[("MANUAL_VALUE", "common")]);

    let (exit, transcript) = fixture.run("e\nMANUAL_VALUE\n\n\n\n", &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("collision:"), "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("MANUAL_VALUE"), "{global}");
}

#[test]
fn wildcard_enrollment_requires_an_extra_confirmation() {
    let fixture = Fixture::new();
    fixture.write(".env.shared", "A_TOKEN=one\nB=two\n");

    // Decline the confirmation: nothing is added.
    let (exit, transcript) = fixture.run("\nw\n.env.shared\nn\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("every current and future key"));
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(!project.contains("all = true"));

    // Accept it: the wildcard entry is stored with the path as entered.
    let fixture = Fixture::new();
    fixture.write(".env.shared", "A_TOKEN=one\nB=two\n");
    let (exit, _) = fixture.run("\nw\n.env.shared\ny\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok);
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("all = true"));
    assert!(project.contains("\".env.shared\""));
}

#[test]
fn manual_and_wildcard_enrollment_can_protect_common_literals() {
    let fixture = Fixture::new();
    fixture.write(".env.flags", "ORDINARY=default\n");
    let environment = fixture.environment(&[("API_TOKEN", "true")]);

    let (exit, transcript) = fixture.run("e\nAPI_TOKEN\n\nw\n.env.flags\ny\n\n\n", &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(global.contains("API_TOKEN"), "{global}");
    assert!(project.contains("all = true"), "{project}");

    let registry = match registry::build(&environment, Some(&fixture.project())) {
        registry::Outcome::Ready(registry) => registry,
        registry::Outcome::Malfunction(problem) => panic!("registry malfunction: {problem:?}"),
    };
    assert_eq!(registry.redactor.active_count(), 2);
    let mut tally = registry.redactor.tally();
    let redacted = registry
        .redactor
        .redact("true default", &mut tally)
        .expect("both common literals should be redacted");
    assert!(!redacted.contains("true"), "{redacted}");
    assert!(!redacted.contains("default"), "{redacted}");
}

#[test]
fn a_selected_wildcard_suppresses_redundant_keyed_candidates() {
    let fixture = Fixture::new();
    fixture.write(".env.shared", "API_TOKEN=value\nOTHER=plain\n");

    let (exit, transcript) = fixture.run("\nw\n.env.shared\ny\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("all = true"));
    assert!(!project.contains("key = \"API_TOKEN\""));
}

#[test]
fn wildcard_values_exclude_their_file_for_other_candidate_groups() {
    let fixture = Fixture::new();
    fixture.write(".env.target", "TARGET_TOKEN=shared\n");
    fixture.write(".env.wild", "ORDINARY_NAME=shared\n");

    let (exit, transcript) = fixture.run("\nw\n.env.wild\ny\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("key = \"TARGET_TOKEN\""));
    assert!(project.contains("all = true"));
}

#[test]
fn deselecting_a_wildcard_restores_keyed_candidates_and_collisions() {
    let fixture = Fixture::new();
    fixture.write(".env.target", "TARGET_TOKEN=shared\n");
    fixture.write(".env.wild", "ORDINARY_NAME=shared\n");

    let (exit, transcript) = fixture.run("\nw\n.env.wild\ny\n2\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(!project.contains("TARGET_TOKEN"));
    assert!(!project.contains("all = true"));
}

#[test]
fn a_wildcard_in_the_other_phase_suppresses_the_same_files_keyed_candidate() {
    let fixture = Fixture::new();
    let dotenv = fixture.write(".env", "API_TOKEN=value\n");
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent"))
        .expect("config directory");
    std::fs::write(
        fixture.global_config(),
        format!(
            "version = 1\n\n[[secret]]\nsource = \"dotenv\"\nfile = \"{}\"\nall = true\n",
            dotenv.display()
        ),
    )
    .expect("global config");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(!project.contains("API_TOKEN"));
}

#[test]
fn skipping_a_wildcard_toggle_restores_cross_phase_suppression() {
    let fixture = Fixture::new();
    let dotenv = fixture.write(".env", "API_TOKEN=value\n");
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent"))
        .expect("config directory");
    let original = format!(
        "version = 1\n\n[[secret]]\nsource = \"dotenv\"\nfile = \"{}\"\nall = true\n",
        dotenv.display()
    );
    std::fs::write(fixture.global_config(), &original).expect("global config");

    let (exit, transcript) = fixture.run("1\ns\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(
        std::fs::read_to_string(fixture.global_config()).expect("global config"),
        original
    );
    assert!(!transcript.contains("key API_TOKEN"), "{transcript}");
}

#[test]
fn resolvable_manual_sources_merge_into_an_existing_group() {
    let canary = Canary::generate("MANUAL_GROUP_TOKEN");
    let fixture = Fixture::new();
    fixture.write("manual.env", &format!("PRIVATE_VALUE={}\n", canary.value()));
    fixture.write(
        "manual.json",
        &format!(r#"{{"credential":"{}"}}"#, canary.value()),
    );
    let environment = fixture.environment(&[
        ("AUTO_TOKEN", canary.value()),
        ("UNGATED_MANUAL", canary.value()),
    ]);
    let script =
        "e\nUNGATED_MANUAL\n\nk\nmanual.env\nPRIVATE_VALUE\nj\nmanual.json\n/credential\n\n\n";

    let (exit, transcript) = fixture.run(script, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("Same current value"));
    assert!(
        transcript.matches("Same current value").count() >= 2,
        "{transcript}"
    );
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(global.contains("AUTO_TOKEN"));
    assert!(global.contains("UNGATED_MANUAL"));
    assert!(project.contains("PRIVATE_VALUE"));
    assert!(project.contains("/credential"));
    assert_canary_absent("manual group transcript", transcript.as_bytes(), &canary);
}

#[test]
fn manual_addition_preserves_existing_display_order() {
    let canary = Canary::generate("MANUAL_ORDER_TOKEN");
    let fixture = Fixture::new();
    let environment =
        fixture.environment(&[("B_TOKEN", canary.value()), ("A_MANUAL", canary.value())]);

    let (exit, transcript) = fixture.run("e\nA_MANUAL\n\n\n\n", &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let grouped = &transcript[transcript
        .rfind("Same current value")
        .expect("group after manual addition")..];
    assert!(
        grouped.find("env B_TOKEN").expect("original member")
            < grouped.find("env A_MANUAL").expect("manual member"),
        "{grouped}"
    );

    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(
        global.find("A_MANUAL").expect("manual identity")
            < global.find("B_TOKEN").expect("automatic identity"),
        "{global}"
    );
    let registry = match registry::build(&environment, Some(&fixture.project())) {
        registry::Outcome::Ready(registry) => registry,
        registry::Outcome::Malfunction(problem) => panic!("registry malfunction: {problem:?}"),
    };
    assert_eq!(
        registry.redactor.canonical_sources().next(),
        Some(&SourceId::env("A_MANUAL"))
    );
    assert_canary_absent("manual order transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("manual order config", global.as_bytes(), &canary);
}

#[test]
fn save_order_can_change_the_canonical_alias() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent"))
        .expect("config directory");
    std::fs::write(
        fixture.global_config(),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"Z_TOKEN\"\n\n[[secret]]\nsource = \"env\"\nname = \"A_TOKEN\"\n",
    )
    .expect("global config");
    let environment = fixture.environment(&[("Z_TOKEN", "same"), ("A_TOKEN", "same")]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let registry = match registry::build(&environment, Some(&fixture.project())) {
        registry::Outcome::Ready(registry) => registry,
        registry::Outcome::Malfunction(problem) => panic!("registry malfunction: {problem:?}"),
    };
    assert_eq!(
        registry.redactor.canonical_sources().next(),
        Some(&SourceId::env("A_TOKEN"))
    );
}

#[test]
fn manual_sources_are_not_attributed_to_automatic_rules() {
    let fixture = Fixture::new();
    fixture.write("manual.credentials", "API_TOKEN=value\n");

    let (exit, transcript) = fixture.run(
        "s\nk\nmanual.credentials\nAPI_TOKEN\n\n\n",
        &fixture.environment(&[]),
    );
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!transcript.contains("rules:"), "{transcript}");
}

#[test]
fn an_unresolved_manual_source_requires_confirmation() {
    let fixture = Fixture::new();

    // Decline: not saved.
    let (exit, transcript) = fixture.run("e\nABSENT_TOKEN\nn\n\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("currently unresolved"));
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(!global.contains("ABSENT_TOKEN"));

    // Properties use the same confirmation path and persist an exact key.
    let fixture = Fixture::new();
    let (exit, _) = fixture.run(
        "p\nmissing.properties\nmissing.password\ny\n\n\n\n",
        &fixture.environment(&[]),
    );
    assert_eq!(exit, Exit::Ok);
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("missing.properties"));
    assert!(global.contains("missing.password"));
}

#[test]
fn exact_json_fields_can_be_enrolled_manually_in_both_scopes() {
    let canary = Canary::generate("JSON_SETUP_TOKEN");
    let fixture = Fixture::new();
    std::fs::write(
        fixture.home().join("global-auth.json"),
        format!(r#"{{"token":"{}"}}"#, canary.value()),
    )
    .expect("global JSON");
    fixture.write(
        "project-auth.json",
        &format!(r#"{{"nested":{{"access/token":"{}"}}}}"#, canary.value()),
    );

    let script =
        "j\n~/global-auth.json\n/token\n\nj\nproject-auth.json\n/nested/access~1token\n\n\n";
    let (exit, transcript) = fixture.run(script, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");

    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(global.contains("source = \"json\""));
    assert!(global.contains("file = \"~/global-auth.json\""));
    assert!(global.contains("pointer = \"/token\""));
    assert!(project.contains("file = \"project-auth.json\""));
    assert!(project.contains("pointer = \"/nested/access~1token\""));
    assert!(!transcript.contains("secret-like source name"));
    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("global config", global.as_bytes(), &canary);
    assert_canary_absent("project config", project.as_bytes(), &canary);
}

#[test]
fn properties_are_discovered_and_enrolled_as_exact_keys() {
    let canary = Canary::generate("PROPERTIES_DISCOVERY");
    let url_canary = Canary::generate("PROPERTIES_URL");
    let fixture = Fixture::new();
    fixture.write(
        "modules/api/database.properties",
        &format!(
            "database.password=  {}  \nREGISTRY=https://user:{}@registry.example.test\nordinary.setting=plain\n",
            canary.value(),
            url_canary.value()
        ),
    );
    fixture.write("messages_en.properties", "password.label=Password\n");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("source = \"properties\""));
    assert!(project.contains("modules/api/database.properties"));
    assert!(project.contains("database.password"));
    assert!(
        project.contains("REGISTRY"),
        "URL admission should bypass key gating"
    );
    assert!(!project.contains("ordinary.setting"));
    assert!(!project.contains("messages_en"));
    assert!(transcript.contains("properties configuration"));
    assert!(transcript.contains("credential-bearing URL"));
    assert!(!transcript.contains("collision:"));
    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("setup transcript", transcript.as_bytes(), &url_canary);
    assert_canary_absent("project config", project.as_bytes(), &canary);
    assert_canary_absent("project config", project.as_bytes(), &url_canary);
}

#[test]
fn gradle_default_and_override_properties_are_additive() {
    let fixture = Fixture::new();
    let default = fixture.home().join(".gradle/gradle.properties");
    std::fs::create_dir_all(default.parent().expect("default parent")).expect("default directory");
    std::fs::write(&default, "repository.password=default-value\n").expect("default properties");
    let override_root = fixture.project().join("custom-gradle");
    std::fs::create_dir_all(&override_root).expect("override directory");
    std::fs::write(
        override_root.join("gradle.properties"),
        "repository.token=override-value\n",
    )
    .expect("override properties");
    let environment = fixture.environment(&[("GRADLE_USER_HOME", "custom-gradle")]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("~/.gradle/gradle.properties"));
    assert!(global.contains(&override_root.to_string_lossy().into_owned()));
    assert!(global.contains("repository.password"));
    assert!(global.contains("repository.token"));
}

#[test]
fn exact_properties_keys_can_be_enrolled_manually_in_both_scopes() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.home().join("manual.properties"),
        "ordinary.name=global-value\n",
    )
    .expect("global properties");
    fixture.write("manual.properties", "ordinary.name=project-value\n");
    let script =
        "p\n~/manual.properties\nordinary.name\n\np\nmanual.properties\nordinary.name\n\n\n";
    let (exit, transcript) = fixture.run(script, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(global.contains("source = \"properties\""));
    assert!(global.contains("~/manual.properties"));
    assert!(project.contains("manual.properties"));
    assert!(project.contains("ordinary.name"));
}

#[test]
fn npmrc_discovery_enrolls_machine_overrides_and_nested_project_files() {
    let canary = Canary::generate("NPMRC_SETUP");
    let fixture = Fixture::new();
    std::fs::write(
        fixture.home().join(".npmrc"),
        format!("//default.example/:_authToken={}\n", canary.value()),
    )
    .expect("default npmrc");
    let override_path = fixture.write(
        "config/global.npmrc",
        &format!("//global.example/:_password={}\n", canary.value()),
    );
    fixture.write(
        "packages/app/.npmrc",
        &format!("project.secret={}\n", canary.value()),
    );
    let environment =
        fixture.environment(&[("NPM_CONFIG_GLOBALCONFIG", &override_path.to_string_lossy())]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(global.contains("~/.npmrc"));
    assert!(global.contains(&override_path.to_string_lossy().into_owned()));
    assert!(project.contains("packages/app/.npmrc"));
    assert!(project.contains("project.secret"));
    assert!(transcript.contains("npmrc credentials"));
    assert!(transcript.contains("secret-like source name"));
    assert_canary_absent("npmrc setup transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("npmrc global config", global.as_bytes(), &canary);
    assert_canary_absent("npmrc project config", project.as_bytes(), &canary);
}

#[test]
fn npmrc_source_files_are_excluded_from_collision_counts() {
    let canary = Canary::generate("NPMRC_COLLISION");
    let fixture = Fixture::new();
    fixture.write(
        ".npmrc",
        &format!("//registry.example/:_authToken={}\n", canary.value()),
    );
    fixture.write("notes.txt", canary.value());

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("1 occurrence(s) elsewhere"));
    assert!(transcript.contains("notes.txt x1"));
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(!project.contains("_authToken"));
    assert_canary_absent("npmrc collision transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("npmrc collision config", project.as_bytes(), &canary);
}

#[test]
fn a_malformed_npmrc_key_is_unavailable_without_hiding_independent_valid_scalars() {
    let malformed_canary = Canary::generate("NPMRC_MALFORMED_KEY");
    let valid_canary = Canary::generate("NPMRC_VALID_KEY");
    let fixture = Fixture::new();
    std::fs::write(
        fixture.home().join(".npmrc"),
        format!(
            "//broken.example/:_auth={}\n//broken.example/:_auth='unterminated\n//registry.example/:_authToken={}\n",
            malformed_canary.value(),
            valid_canary.value()
        ),
    )
    .expect("npmrc");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(transcript.contains("unavailable: has an invalid npmrc entry"));
    assert!(global.contains("//registry.example/:_authToken"));
    assert!(!global.contains("//broken.example/:_auth"));
    for canary in [&malformed_canary, &valid_canary] {
        assert_canary_absent("npmrc setup transcript", transcript.as_bytes(), canary);
        assert_canary_absent("npmrc global config", global.as_bytes(), canary);
    }
}

#[test]
fn exact_npmrc_keys_can_be_enrolled_manually_in_both_scopes() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.home().join("manual.npmrc"),
        "ordinary=global-value\n",
    )
    .expect("global npmrc");
    fixture.write("manual.npmrc", "ordinary=project-value\n");
    let script = "r\n~/manual.npmrc\nordinary\n\nr\nmanual.npmrc\nordinary\n\n\n";
    let (exit, transcript) = fixture.run(script, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(global.contains("source = \"npmrc\""));
    assert!(global.contains("~/manual.npmrc"));
    assert!(project.contains("manual.npmrc"));
    assert!(project.contains("key = \"ordinary\""));
}

#[test]
fn unresolved_json_may_be_confirmed_but_malformed_json_cannot() {
    let fixture = Fixture::new();
    let missing = fixture.home().join("missing.json");
    let script = format!("j\n{}\n/token\ny\n\n\n\n", missing.display());
    let (exit, transcript) = fixture.run(&script, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(
        std::fs::read_to_string(fixture.global_config())
            .expect("global config")
            .contains("/token")
    );

    let fixture = Fixture::new();
    fixture.write("broken.json", r#"{"token":}"#);
    let (exit, transcript) =
        fixture.run("s\nj\nbroken.json\n/token\n\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("Not added; repair the source"));
    assert!(
        !std::fs::read_to_string(fixture.project_config())
            .expect("project config")
            .contains("broken.json")
    );
}

#[test]
fn arbitrary_json_files_are_not_discovered_or_scanned() {
    let canary = Canary::generate("UNDISCOVERED_JSON_TOKEN");
    let fixture = Fixture::new();
    fixture.write(
        "auth.json",
        &format!(r#"{{"VERY_SECRET_TOKEN":"{}"}}"#, canary.value()),
    );
    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!transcript.contains("VERY_SECRET_TOKEN"));
    assert!(
        !std::fs::read_to_string(fixture.project_config())
            .expect("project config")
            .contains("auth.json")
    );
}

#[test]
fn url_looking_json_fields_are_not_automatic_candidates() {
    let canary = Canary::generate("JSON_URL_PASSWORD");
    let fixture = Fixture::new();
    fixture.write(
        "auth.json",
        &format!(
            r#"{{"endpoint":"https://agent:{}@service.example.test"}}"#,
            canary.value()
        ),
    );

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(!project.contains("auth.json"));
    assert!(!project.contains("/endpoint"));
    assert_canary_absent("JSON URL transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("JSON URL config", project.as_bytes(), &canary);
}

#[test]
fn a_manually_added_json_url_does_not_match_the_url_rule() {
    let canary = Canary::generate("MANUAL_JSON_URL_PASSWORD");
    let fixture = Fixture::new();
    fixture.write(
        "auth.json",
        &format!(
            r#"{{"endpoint":"https://agent:{}@service.example.test"}}"#,
            canary.value()
        ),
    );

    let (exit, transcript) = fixture.run(
        "s\nj\nauth.json\n/endpoint\n\n\n",
        &fixture.environment(&[]),
    );
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!transcript.contains("credential-bearing URL"));
    assert!(
        std::fs::read_to_string(fixture.project_config())
            .expect("project config")
            .contains("/endpoint")
    );
    assert_canary_absent("manual JSON URL transcript", transcript.as_bytes(), &canary);
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

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("ROTATED_TOKEN"));
    assert!(transcript.contains("(enrolled)"));
}

#[test]
fn existing_unresolved_automatic_sources_keep_their_rule_attribution() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.global_config().parent().expect("parent"))
        .expect("config directory");
    std::fs::write(
        fixture.global_config(),
        "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"API_TOKEN\"\n",
    )
    .expect("global config");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[("API_TOKEN", "")]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(
        transcript.contains("rules: secret-like source name"),
        "{transcript}"
    );
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

    let (exit, _) = fixture.run("1\n\n\n\n", &fixture.environment(&[("OLD_TOKEN", "value")]));
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
    let (exit, transcript) = fixture.run("\n\n1\n\n\n", &fixture.environment(&[]));
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

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
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
    if std::fs::write(fixture.project().join(&name), "API_TOKEN=value\n").is_err() {
        // `LIM-022`: APFS rejects file names that are not valid UTF-8, so the
        // scenario cannot be built on every supported platform.
        return;
    }

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
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
    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
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

    let (exit, _) = fixture.run_from(ACCEPT_ALL, &fixture.environment(&[]), &nested);
    assert_eq!(exit, Exit::Ok);
    assert!(fixture.project_config().exists());
    assert!(!nested.join(".contextveil.toml").exists());
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

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
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

    let (_, transcript) = fixture.run(ACCEPT_ALL, &environment);
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

    let (_, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert!(!transcript.contains('\u{1b}'));
    assert!(transcript.contains("\\e[31mAPI_TOKEN"));
}

/// Marks Claude Code as present so the integration phase detects it.
fn detect_claude(fixture: &Fixture) {
    std::fs::create_dir_all(fixture.home().join(".claude")).expect("claude directory");
}

#[test]
fn the_claude_hook_is_installed_and_verified_offline() {
    let fixture = Fixture::new();
    detect_claude(&fixture);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("detected"));
    assert!(transcript.contains("Installed the Claude Code integration"));
    assert!(transcript.contains("Offline protocol check passed"));

    let settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.claude_settings()).expect("settings"),
    )
    .expect("valid JSON");
    let group = &settings["hooks"]["PostToolUse"][0];
    assert_eq!(group["matcher"], serde_json::json!("*"));
    assert_eq!(group["hooks"][0]["type"], serde_json::json!("command"));
    assert_eq!(group["hooks"][0]["timeout"], serde_json::json!(5));
    let command = group["hooks"][0]["command"].as_str().expect("command");
    assert!(command.ends_with(" hook claude"));
    assert!(command.starts_with('/') || command.starts_with('\''));

    // Ownership is recorded next to the global configuration.
    let record =
        std::fs::read_to_string(fixture.global_config().with_file_name("integrations.toml"))
            .expect("integration record");
    assert!(record.contains("hook claude"));
}

#[test]
fn all_integrations_pass_offline_verification() {
    let fixture = Fixture::new();

    let (exit, transcript) = fixture.run("\n\n1 2 3 4\n\n", &fixture.environment(&[]));

    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(
        transcript.matches("Offline protocol check passed").count(),
        4
    );
    assert!(fixture.claude_settings().exists());
    assert!(fixture.home().join(".codex/hooks.json").exists());
    assert!(
        fixture
            .home()
            .join(".copilot/hooks/contextveil.json")
            .exists()
    );
    assert!(
        fixture
            .home()
            .join(".config/opencode/plugins/contextveil.ts")
            .exists()
    );
}

#[test]
#[cfg(unix)]
fn verification_failure_removes_new_artifacts() {
    let fixture = Fixture::new();
    let executable = fixture.executable_failing("claude");
    let state = fixture.global_config().with_file_name("integrations.toml");

    let (exit, transcript) = fixture.run_with_executable(
        "\n\n1\n\n",
        &fixture.environment(&[]),
        &fixture.project(),
        &executable,
    );

    assert_eq!(exit, Exit::Failure, "{transcript}");
    assert!(transcript.contains("Offline protocol check failed"));
    assert!(!fixture.claude_settings().exists());
    assert!(
        !state.exists(),
        "failed ownership changes must not be saved"
    );
}

#[test]
#[cfg(unix)]
fn verification_failure_restores_previous_state_byte_for_byte() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    detect_claude(&fixture);
    assert_eq!(
        fixture.run(ACCEPT_ALL, &fixture.environment(&[])).0,
        Exit::Ok
    );

    let settings_path = fixture.claude_settings();
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings_path).expect("installed settings"))
            .expect("settings JSON");
    settings["hooks"]["PostToolUse"]
        .as_array_mut()
        .expect("hook groups")
        .push(serde_json::json!({
            "matcher": "*",
            "hooks": [{"type": "command", "command": "/other/mutator"}]
        }));
    let mut previous_artifact = b"\n  ".to_vec();
    previous_artifact.extend(serde_json::to_vec(&settings).expect("serialize settings"));
    previous_artifact.extend_from_slice(b"\n");
    std::fs::write(&settings_path, &previous_artifact).expect("custom settings bytes");
    std::fs::set_permissions(&settings_path, std::fs::Permissions::from_mode(0o640))
        .expect("custom settings permissions");

    let state_path = fixture.global_config().with_file_name("integrations.toml");
    let mut previous_state = b"# preserve this formatting\n".to_vec();
    previous_state.extend(std::fs::read(&state_path).expect("ownership state"));
    std::fs::write(&state_path, &previous_state).expect("custom state bytes");

    let executable = fixture.executable_failing("claude");
    let (exit, transcript) = fixture.run_with_executable(
        ACCEPT_ALL,
        &fixture.environment(&[]),
        &fixture.project(),
        &executable,
    );

    assert_eq!(exit, Exit::Failure, "{transcript}");
    assert!(!transcript.contains("Keep it and continue?"));
    assert_eq!(
        std::fs::read(&settings_path).expect("restored settings"),
        previous_artifact
    );
    assert_eq!(
        std::fs::metadata(&settings_path)
            .expect("settings metadata")
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert_eq!(
        std::fs::read(state_path).expect("restored state"),
        previous_state
    );
}

#[test]
#[cfg(unix)]
fn later_failure_keeps_completed_earlier_actions() {
    let fixture = Fixture::new();
    let executable = fixture.executable_failing("codex");

    let (exit, transcript) = fixture.run_with_executable(
        "\n\n1 2 3 4\n\n",
        &fixture.environment(&[]),
        &fixture.project(),
        &executable,
    );

    assert_eq!(exit, Exit::Failure, "{transcript}");
    assert!(fixture.claude_settings().exists());
    assert!(!fixture.home().join(".codex/hooks.json").exists());
    assert!(
        !fixture
            .home()
            .join(".copilot/hooks/contextveil.json")
            .exists()
    );
    assert!(
        !fixture
            .home()
            .join(".config/opencode/plugins/contextveil.ts")
            .exists()
    );
    assert!(!transcript.contains("Installed the GitHub Copilot CLI integration"));

    let state =
        std::fs::read_to_string(fixture.global_config().with_file_name("integrations.toml"))
            .expect("ownership state");
    assert!(state.contains("[claude]"));
    assert!(!state.contains("[codex]"));
}

#[test]
fn rerunning_setup_leaves_an_installed_integration_byte_identical() {
    // `SET-014`, `INT-004`: a second run must not duplicate or rewrite the
    // managed entry.
    let fixture = Fixture::new();
    detect_claude(&fixture);

    assert_eq!(
        fixture.run(ACCEPT_ALL, &fixture.environment(&[])).0,
        Exit::Ok
    );
    let first = std::fs::read(fixture.claude_settings()).expect("settings");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(
        std::fs::read(fixture.claude_settings()).expect("settings"),
        first
    );

    let settings: serde_json::Value = serde_json::from_slice(&first).expect("valid JSON");
    assert_eq!(
        settings["hooks"]["PostToolUse"]
            .as_array()
            .expect("array")
            .len(),
        1
    );
}

#[test]
fn deselecting_the_integration_removes_only_the_managed_hook() {
    let fixture = Fixture::new();
    detect_claude(&fixture);
    std::fs::write(
        fixture.claude_settings(),
        r#"{"model": "opus", "hooks": {"PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/other/tool"}]}]}}"#,
    )
    .expect("write settings");

    // The pre-existing hook is a conflict, so the first run answers its prompt.
    assert_eq!(
        fixture.run("\n\n\nn\n", &fixture.environment(&[])).0,
        Exit::Ok
    );
    let (exit, transcript) = fixture.run("\n\n1\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("Removed the Claude Code integration"));

    let settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.claude_settings()).expect("settings"),
    )
    .expect("valid JSON");
    assert_eq!(settings["model"], serde_json::json!("opus"));
    let groups = settings["hooks"]["PostToolUse"].as_array().expect("array");
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0]["hooks"][0]["command"],
        serde_json::json!("/other/tool")
    );
}

#[test]
fn a_competing_mutating_hook_is_offered_for_approval() {
    let fixture = Fixture::new();
    detect_claude(&fixture);
    std::fs::write(
        fixture.claude_settings(),
        r#"{"hooks": {"PostToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/other/mutator"}]}]}}"#,
    )
    .expect("write settings");

    // Decline first: the conflict stays unapproved.
    let (exit, transcript) = fixture.run("\n\n\nn\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("/other/mutator"));
    assert!(transcript.contains("can also change the same content"));
    let record_path = fixture.global_config().with_file_name("integrations.toml");
    let record = std::fs::read_to_string(&record_path).expect("integration record");
    assert!(!record.contains("/other/mutator"));

    // Approve on the next run: the approval is recorded (`INT-005`).
    let (exit, _) = fixture.run("\n\n\ny\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok);
    let record = std::fs::read_to_string(&record_path).expect("integration record");
    assert!(record.contains("/other/mutator"));
}

#[test]
fn an_undetected_harness_discloses_limited_verification() {
    let fixture = Fixture::new();
    // No `~/.claude` directory and no executable on PATH.
    let (exit, transcript) =
        fixture.run("\n\n1\n\n", &fixture.environment(&[("PATH", "/nowhere")]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("not detected"));
    assert!(transcript.contains("cannot \nconfirm") || transcript.contains("cannot confirm"));
    assert!(fixture.claude_settings().exists());
}

#[test]
fn a_malformed_settings_file_fails_the_integration_phase_without_changing_it() {
    let fixture = Fixture::new();
    detect_claude(&fixture);
    let malformed = "{ not json";
    std::fs::write(fixture.claude_settings(), malformed).expect("write settings");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Failure, "{transcript}");
    assert!(transcript.contains("installation failed"));
    assert_eq!(
        std::fs::read_to_string(fixture.claude_settings()).expect("read back"),
        malformed
    );
}

#[test]
fn an_experimental_integration_requires_an_affirmative_choice() {
    // `SUP-003`, `INT-001`: Codex is never selected by default, even when
    // detected, and installing it is an explicit opt-in.
    let fixture = Fixture::new();
    detect_claude(&fixture);
    std::fs::create_dir_all(fixture.home().join(".codex")).expect("codex directory");
    let codex_hooks = fixture.home().join(".codex").join("hooks.json");

    // Accepting the defaults installs Claude only.
    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("Codex CLI (EXPERIMENTAL)"));
    assert!(fixture.claude_settings().exists());
    assert!(!codex_hooks.exists());

    // Toggling it on installs it, with the experimental label and the host's
    // trust workflow disclosed.
    let (exit, transcript) = fixture.run("\n\n2\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("is EXPERIMENTAL"));
    assert!(transcript.contains("Trust all and continue"));
    assert!(transcript.contains("Installed the Codex CLI integration"));

    let hooks: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&codex_hooks).expect("hooks file"))
            .expect("valid JSON");
    let group = &hooks["hooks"]["PostToolUse"][0];
    assert_eq!(group["hooks"][0]["timeout"], serde_json::json!(5));
    assert!(
        group["hooks"][0]["command"]
            .as_str()
            .expect("command")
            .ends_with(" hook codex")
    );

    // Deselecting removes it again.
    let (exit, transcript) = fixture.run("\n\n2\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("Removed the Codex CLI integration"));
}

#[test]
fn copilot_installs_one_dedicated_file_and_leaves_others_alone() {
    // `COP-001`: only ContextVeil's own hook file is managed.
    let fixture = Fixture::new();
    detect_claude(&fixture);
    let hooks = fixture.home().join(".copilot").join("hooks");
    std::fs::create_dir_all(&hooks).expect("copilot hooks directory");
    let other = hooks.join("team-policy.json");
    let other_contents =
        r#"{"version": 1, "hooks": {"postToolUse": [{"type": "command", "bash": "/other/tool"}]}}"#;
    std::fs::write(&other, other_contents).expect("write other hook file");

    // Copilot is row 3 and is never selected by default; the conflict in the
    // other file needs review once it is selected.
    let (exit, transcript) = fixture.run("\n\n3\n\nn\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("GitHub Copilot CLI (EXPERIMENTAL)"));
    assert!(transcript.contains("Installed the GitHub Copilot CLI integration"));
    assert!(transcript.contains("/other/tool"));

    let managed: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(hooks.join("contextveil.json")).expect("managed hook file"),
    )
    .expect("valid JSON");
    assert_eq!(managed["version"], serde_json::json!(1));
    for event in ["userPromptTransformed", "postToolUse"] {
        assert_eq!(
            managed["hooks"][event][0]["timeoutSec"],
            serde_json::json!(5)
        );
    }
    assert_eq!(
        std::fs::read_to_string(&other).expect("read other hook file"),
        other_contents
    );

    // Deselecting removes only the managed file.
    let (exit, transcript) = fixture.run("\n\n3\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!hooks.join("contextveil.json").exists());
    assert!(other.exists());
}

#[test]
fn opencode_installs_one_owned_plugin_file() {
    // `OCO-001`: one ContextVeil-owned plugin file, opt-in like every
    // experimental integration.
    let fixture = Fixture::new();
    detect_claude(&fixture);
    let plugins = fixture
        .home()
        .join(".config")
        .join("opencode")
        .join("plugins");
    std::fs::create_dir_all(&plugins).expect("plugins directory");
    let other = plugins.join("other.ts");
    std::fs::write(&other, "export const Other = async () => ({})\n").expect("write other plugin");

    // OpenCode is row 4; its existing sibling plugin needs review once selected.
    let (exit, transcript) = fixture.run("\n\n4\n\nn\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("OpenCode (EXPERIMENTAL)"));
    assert!(transcript.contains("Installed the OpenCode integration"));
    assert!(transcript.contains("other.ts"));

    let plugin = std::fs::read_to_string(plugins.join("contextveil.ts")).expect("plugin file");
    assert!(plugin.starts_with("// ContextVeil managed plugin."));
    assert!(plugin.contains("chat.message"));
    assert!(plugin.contains("tool.execute.after"));
    assert!(!plugin.contains("__CONTEXTVEIL_BINARY__"));

    let (exit, transcript) = fixture.run("\n\n4\n\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!plugins.join("contextveil.ts").exists());
    assert!(other.exists(), "unrelated plugins are never removed");
}

#[test]
fn skipping_the_integration_phase_changes_nothing() {
    let fixture = Fixture::new();
    detect_claude(&fixture);
    let (exit, transcript) = fixture.run("\n\ns\n", &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!fixture.claude_settings().exists());
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

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("more than once"));
    assert_canary_absent("setup transcript", transcript.as_bytes(), &canary);
}

#[test]
fn known_sources_persist_explicit_refs_and_bypass_name_gating() {
    let canary = Canary::generate("KNOWN_IDENTITY");
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.home().join(".codex")).expect("codex directory");
    std::fs::write(
        fixture.home().join(".codex/auth.json"),
        format!(
            r#"{{"tokens":{{"access_token":"{0}","refresh_token":"{0}"}}}}"#,
            canary.value()
        ),
    )
    .expect("codex auth");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("source = \"json\""));
    assert!(global.contains("file = \"~/.codex/auth.json\""));
    assert!(global.contains("pointer = \"/tokens/access_token\""));
    assert!(global.contains("pointer = \"/tokens/refresh_token\""));
    assert!(transcript.contains("Same current value"));
    assert_eq!(transcript.matches("Codex primary credentials").count(), 1);
    assert_canary_absent("known source transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("known source config", global.as_bytes(), &canary);
}

#[test]
fn known_source_defaults_and_overrides_are_discovered_additively() {
    let codex_default = Canary::generate("CODEX_DEFAULT");
    let codex_override = Canary::generate("CODEX_OVERRIDE");
    let opencode_default = Canary::generate("OPENCODE_DEFAULT");
    let opencode_override = Canary::generate("OPENCODE_OVERRIDE");
    let copilot_default = Canary::generate("COPILOT_DEFAULT");
    let copilot_override = Canary::generate("COPILOT_OVERRIDE");
    let claude_default = Canary::generate("CLAUDE_DEFAULT");
    let claude_override = Canary::generate("CLAUDE_OVERRIDE");
    let fixture = Fixture::new();

    for (relative, contents) in [
        (
            ".codex/auth.json",
            format!(r#"{{"OPENAI_API_KEY":"{}"}}"#, codex_default.value()),
        ),
        (
            ".local/share/opencode/auth.json",
            format!(r#"{{"provider":{{"key":"{}"}}}}"#, opencode_default.value()),
        ),
        (
            ".copilot/config.json",
            format!(
                r#"{{"copilotTokens":{{"default":"{}"}}}}"#,
                copilot_default.value()
            ),
        ),
        (
            ".claude/settings.json",
            format!(
                r#"{{"env":{{"ANTHROPIC_API_KEY":"{}"}}}}"#,
                claude_default.value()
            ),
        ),
    ] {
        let path = fixture.home().join(relative);
        std::fs::create_dir_all(path.parent().expect("machine source parent")).expect("directory");
        std::fs::write(path, contents).expect("machine source");
    }
    for (relative, contents) in [
        (
            "override/codex/auth.json",
            format!(r#"{{"OPENAI_API_KEY":"{}"}}"#, codex_override.value()),
        ),
        (
            "override/opencode/auth.json",
            format!(
                r#"{{"provider":{{"key":"{}"}}}}"#,
                opencode_override.value()
            ),
        ),
        (
            "override/copilot/config.json",
            format!(
                r#"{{"copilotTokens":{{"override":"{}"}}}}"#,
                copilot_override.value()
            ),
        ),
        (
            "override/claude/settings.json",
            format!(
                r#"{{"env":{{"ANTHROPIC_API_KEY":"{}"}}}}"#,
                claude_override.value()
            ),
        ),
    ] {
        fixture.write(relative, &contents);
    }

    let environment = fixture.environment(&[
        ("CODEX_HOME", "override/codex"),
        ("XDG_DATA_HOME", "override"),
        ("COPILOT_HOME", "override/copilot"),
        ("CLAUDE_CONFIG_DIR", "override/claude"),
    ]);
    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");

    for path in [
        "~/.codex/auth.json",
        "~/.local/share/opencode/auth.json",
        "~/.copilot/config.json",
        "~/.claude/settings.json",
    ] {
        assert!(
            global.contains(path),
            "missing default path {path}: {global}"
        );
    }
    for path in [
        fixture.project().join("override/codex/auth.json"),
        fixture.project().join("override/opencode/auth.json"),
        fixture.project().join("override/copilot/config.json"),
        fixture.project().join("override/claude/settings.json"),
    ] {
        let path = path.to_string_lossy();
        assert!(
            global.contains(path.as_ref()),
            "missing override path {path}: {global}"
        );
    }
    for canary in [
        &codex_default,
        &codex_override,
        &opencode_default,
        &opencode_override,
        &copilot_default,
        &copilot_override,
        &claude_default,
        &claude_override,
    ] {
        assert_canary_absent("additive setup transcript", transcript.as_bytes(), canary);
        assert_canary_absent("additive setup config", global.as_bytes(), canary);
    }
}

#[test]
fn project_known_source_aliases_form_one_candidate_group() {
    let canary = Canary::generate("PROJECT_KNOWN_ALIAS");
    let fixture = Fixture::new();
    fixture.write(
        "app/.claude/settings.json",
        &format!(r#"{{"env":{{"ANTHROPIC_API_KEY":"{}"}}}}"#, canary.value()),
    );
    fixture.write(
        "app/.mcp.json",
        &format!(
            r#"{{"mcpServers":{{"server":{{"headers":{{"Authorization":"{}"}}}}}}}}"#,
            canary.value()
        ),
    );

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("Same current value"));
    assert_eq!(
        transcript
            .matches("Claude configured environment credentials")
            .count(),
        1
    );
    assert_eq!(
        transcript.matches("Claude MCP server credentials").count(),
        1
    );
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(project.contains("app/.claude/settings.json"));
    assert!(project.contains("app/.mcp.json"));
    assert!(project.contains("/mcpServers/server/headers/Authorization"));
    assert_canary_absent("known aliases transcript", transcript.as_bytes(), &canary);
    assert_canary_absent("known aliases config", project.as_bytes(), &canary);
}

#[test]
fn a_known_source_group_with_an_external_collision_defaults_unselected() {
    let canary = Canary::generate("PROJECT_KNOWN_COLLISION");
    let fixture = Fixture::new();
    fixture.write(
        ".claude/settings.json",
        &format!(
            r#"{{"env":{{"ANTHROPIC_AUTH_TOKEN":"{}"}}}}"#,
            canary.value()
        ),
    );
    fixture.write(
        ".mcp.json",
        &format!(
            r#"{{"mcpServers":{{"server":{{"env":{{"TOKEN":"{}"}}}}}}}}"#,
            canary.value()
        ),
    );
    fixture.write("README.txt", canary.value());

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("Same current value"));
    assert!(transcript.contains("collision:"));
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(!project.contains("settings.json"));
    assert!(!project.contains(".mcp.json"));
    assert_canary_absent("known collision transcript", transcript.as_bytes(), &canary);
}

#[test]
fn known_source_override_reruns_are_idempotent_and_pick_up_changes() {
    let fixture = Fixture::new();
    fixture.write("first/auth.json", r#"{"agent_identity":"first-value"}"#);
    fixture.write("second/auth.json", r#"{"agent_identity":"second-value"}"#);
    let first_environment = fixture.environment(&[("CODEX_HOME", "first")]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &first_environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let first = std::fs::read(fixture.global_config()).expect("first global config");
    let explicit_first = fixture.project().join("first/auth.json");
    assert!(
        String::from_utf8_lossy(&first).contains(&explicit_first.to_string_lossy().into_owned())
    );

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &first_environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert_eq!(
        std::fs::read(fixture.global_config()).expect("second read"),
        first
    );

    let second_environment = fixture.environment(&[("CODEX_HOME", "second")]);
    let (exit, transcript) = fixture.run(ACCEPT_ALL, &second_environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let rerun = std::fs::read_to_string(fixture.global_config()).expect("rerun global config");
    let first_position = rerun.find("first/auth.json").expect("existing ref remains");
    let second_position = rerun.find("second/auth.json").expect("new override ref");
    assert!(
        first_position < second_position,
        "existing refs must remain first"
    );
}

#[test]
fn malformed_and_non_utf8_known_sources_are_visible_and_secret_safe() {
    let canary = Canary::generate("MALFORMED_KNOWN");
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.home().join(".codex")).expect("codex directory");
    std::fs::write(
        fixture.home().join(".codex/auth.json"),
        format!(r#"{{"OPENAI_API_KEY":"{}""#, canary.value()),
    )
    .expect("malformed auth");
    std::fs::create_dir_all(fixture.home().join(".local/share/opencode"))
        .expect("opencode directory");
    std::fs::write(
        fixture.home().join(".local/share/opencode/auth.json"),
        [b'{', b'"', 0xff, b'"', b':', b'1', b'}'],
    )
    .expect("non UTF-8 auth");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("unavailable:"));
    assert!(transcript.contains("malformed JSON"));
    assert!(transcript.contains("not valid UTF-8"));
    assert_canary_absent(
        "known source unavailable transcript",
        transcript.as_bytes(),
        &canary,
    );
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(!global.contains("auth.json"));
}

#[test]
#[cfg(unix)]
fn setup_follows_exact_machine_file_symlinks_but_not_project_symlinks() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.home().join("target.json"),
        r#"{"agent_identity":"machine-value"}"#,
    )
    .expect("machine target");
    std::fs::create_dir_all(fixture.home().join(".codex")).expect("codex directory");
    std::os::unix::fs::symlink(
        fixture.home().join("target.json"),
        fixture.home().join(".codex/auth.json"),
    )
    .expect("machine file symlink");
    std::fs::create_dir_all(fixture.home().join("outside/.claude"))
        .expect("outside claude directory");
    std::fs::write(
        fixture.home().join("outside/.claude/settings.json"),
        r#"{"env":{"ANTHROPIC_API_KEY":"project-value"}}"#,
    )
    .expect("outside settings");
    std::os::unix::fs::symlink(
        fixture.home().join("outside"),
        fixture.project().join("linked"),
    )
    .expect("project directory symlink");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    let project = std::fs::read_to_string(fixture.project_config()).expect("project config");
    assert!(global.contains("/agent_identity"));
    assert!(!project.contains("linked"));
}

#[test]
fn relative_known_source_overrides_use_the_setup_invocation_directory() {
    let fixture = Fixture::new();
    let codex = Canary::generate("CODEX_OVERRIDE");
    let opencode = Canary::generate("OPENCODE_OVERRIDE");
    let copilot = Canary::generate("COPILOT_OVERRIDE");
    let claude = Canary::generate("CLAUDE_OVERRIDE");
    std::fs::create_dir_all(fixture.project().join(".git")).expect("git root");
    let nested = fixture.project().join("packages/app");
    std::fs::create_dir_all(&nested).expect("nested invocation directory");
    let stores = nested.join("stores");
    for (relative, contents) in [
        (
            "codex/auth.json",
            format!(r#"{{"OPENAI_API_KEY":"{}"}}"#, codex.value()),
        ),
        (
            "data/opencode/auth.json",
            format!(
                r#"{{"provider":{{"type":"api","key":"{}"}}}}"#,
                opencode.value()
            ),
        ),
        (
            "copilot/config.json",
            format!(
                r#"{{"copilotTokens":{{"github.com":"{}"}}}}"#,
                copilot.value()
            ),
        ),
        (
            "claude/settings.json",
            format!(r#"{{"env":{{"ANTHROPIC_API_KEY":"{}"}}}}"#, claude.value()),
        ),
    ] {
        let path = stores.join(relative);
        std::fs::create_dir_all(path.parent().expect("store parent")).expect("store directory");
        std::fs::write(path, contents).expect("store file");
    }
    let environment = fixture.environment(&[
        ("CODEX_HOME", "stores/codex"),
        ("XDG_DATA_HOME", "stores/data"),
        ("COPILOT_HOME", "stores/copilot"),
        ("CLAUDE_CONFIG_DIR", "stores/claude"),
    ]);

    let (exit, transcript) = fixture.run_from(ACCEPT_ALL, &environment, &nested);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    for relative in [
        "stores/codex/auth.json",
        "stores/data/opencode/auth.json",
        "stores/copilot/config.json",
        "stores/claude/settings.json",
    ] {
        assert!(
            global.contains(&nested.join(relative).to_string_lossy().into_owned()),
            "missing invocation-relative {relative}: {global}"
        );
    }
    for canary in [&codex, &opencode, &copilot, &claude] {
        assert_canary_absent("override setup transcript", transcript.as_bytes(), canary);
        assert_canary_absent("override global config", global.as_bytes(), canary);
    }
}

#[test]
#[cfg(unix)]
fn non_utf8_known_source_override_keeps_default_discovery() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let canary = Canary::generate("NON_UTF8_OVERRIDE");
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.home().join(".codex")).expect("default codex directory");
    std::fs::write(
        fixture.home().join(".codex/auth.json"),
        format!(r#"{{"OPENAI_API_KEY":"{}"}}"#, canary.value()),
    )
    .expect("default codex auth");
    let environment = Environment::from_pairs([
        (OsString::from("HOME"), fixture.home().into_os_string()),
        (
            OsString::from("CODEX_HOME"),
            OsString::from_vec(vec![b'c', b'o', b'd', b'e', b'x', 0xff]),
        ),
    ]);

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &environment);
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(transcript.contains("unavailable: CODEX_HOME"));
    assert!(transcript.contains("override is not valid UTF-8"));
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("~/.codex/auth.json"));
    assert_canary_absent(
        "non-UTF-8 override transcript",
        transcript.as_bytes(),
        &canary,
    );
    assert_canary_absent("non-UTF-8 override config", global.as_bytes(), &canary);
}

#[test]
#[cfg(unix)]
fn exact_machine_symlink_target_inside_project_is_excluded_from_collisions() {
    let canary = Canary::generate("SYMLINK_COLLISION");
    let fixture = Fixture::new();
    let target = fixture.write(
        "stores/codex-auth.json",
        &format!(r#"{{"OPENAI_API_KEY":"{}"}}"#, canary.value()),
    );
    std::fs::create_dir_all(fixture.home().join(".codex")).expect("codex directory");
    std::os::unix::fs::symlink(&target, fixture.home().join(".codex/auth.json"))
        .expect("machine source symlink");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));
    assert_eq!(exit, Exit::Ok, "{transcript}");
    assert!(!transcript.contains("collision:"), "{transcript}");
    let global = std::fs::read_to_string(fixture.global_config()).expect("global config");
    assert!(global.contains("~/.codex/auth.json"));
    assert_canary_absent(
        "symlink collision transcript",
        transcript.as_bytes(),
        &canary,
    );
}

#[test]
fn invalid_config_prevents_project_discovery_file_reads() {
    use std::time::{Duration, SystemTime};

    let fixture = Fixture::new();
    let dotenv = fixture.write("nested/.env", "API_TOKEN=value\n");
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    std::fs::File::open(&dotenv)
        .expect("dotenv handle")
        .set_times(
            std::fs::FileTimes::new()
                .set_accessed(old)
                .set_modified(old),
        )
        .expect("old file times");
    std::fs::write(fixture.project_config(), "version = 2\n").expect("invalid project config");

    let (exit, transcript) = fixture.run(ACCEPT_ALL, &fixture.environment(&[]));

    assert_eq!(exit, Exit::Failure, "{transcript}");
    assert_eq!(
        std::fs::metadata(dotenv)
            .expect("dotenv metadata")
            .accessed()
            .expect("dotenv access time"),
        old,
        "project discovery must not open dotenv files before both config preflights"
    );
}
