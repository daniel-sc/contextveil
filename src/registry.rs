//! Effective registry composition for one runtime event.
//!
//! Use of the effective registry is all-or-nothing (`CFG-012`): an invalid or
//! unreadable configuration file disables every redaction for the event instead
//! of producing a partial matcher. A missing global file is a non-clean
//! configuration state that warns without discarding valid redaction
//! (`CFG-013`).
//!
//! Project registry selection arrives with `T020`.

use crate::config::{self, ConfigError, Load};
use crate::matcher::Redactor;
use crate::secret::SourceId;
use crate::source::{self, Environment, Resolution, Unresolved};

/// A registry that may be used for the current event.
#[derive(Debug, Clone, Default)]
pub struct EffectiveRegistry {
    pub redactor: Redactor,
    /// Enrolled sources that currently have no usable value. These stay silent
    /// during normal runtime (`RED-009`).
    pub unresolved: Vec<(SourceId, Unresolved)>,
    pub warnings: Vec<Warning>,
}

/// A non-fatal configuration state worth reporting where the host permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
    /// No global configuration file exists, so machine setup is incomplete.
    GlobalConfigMissing,
}

impl Warning {
    pub fn message(&self) -> &'static str {
        match self {
            Warning::GlobalConfigMissing => {
                "SecretSieve global setup is incomplete: no global configuration file was found. \
                 Run `secretsieve setup`."
            }
        }
    }
}

/// A condition that prevents trustworthy use of the effective registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Malfunction {
    /// A configuration file could not be read or parsed (`CFG-012`).
    Config(ConfigError),
    /// The global configuration location cannot be determined.
    NoConfigLocation,
}

impl Malfunction {
    /// Emit-safe warning text.
    ///
    /// It names neither the file nor its content: paths are untrusted terminal
    /// input (`SEC-006`) and configuration text must not be echoed.
    pub fn message(&self) -> String {
        match self {
            Malfunction::Config(error) => format!(
                "SecretSieve disabled redaction for this event: configuration is unusable ({}). \
                 Run `secretsieve doctor`.",
                error.kind.reason()
            ),
            Malfunction::NoConfigLocation => {
                "SecretSieve disabled redaction for this event: the configuration location could \
                 not be determined. Run `secretsieve doctor`."
                    .to_string()
            }
        }
    }
}

/// Result of composing the effective registry.
#[derive(Debug, Clone)]
pub enum Outcome {
    Ready(EffectiveRegistry),
    Malfunction(Malfunction),
}

/// Builds the effective registry for one event from the global configuration.
pub fn build(environment: &Environment) -> Outcome {
    let Some(path) = config::global_config_path(environment) else {
        return Outcome::Malfunction(Malfunction::NoConfigLocation);
    };

    let (config, warnings) = match config::load(&path) {
        Load::Valid(config) => (config, Vec::new()),
        Load::Missing => (
            config::Config::default(),
            vec![Warning::GlobalConfigMissing],
        ),
        Load::Invalid(error) => return Outcome::Malfunction(Malfunction::Config(error)),
    };

    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for reference in &config.sources {
        match source::resolve(reference, environment) {
            Resolution::Resolved(secret) => resolved.push(secret),
            Resolution::Unresolved { source, why } => unresolved.push((source, why)),
        }
    }

    Outcome::Ready(EffectiveRegistry {
        redactor: Redactor::new(resolved),
        unresolved,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Canary;

    struct Fixture {
        root: std::path::PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "secretsieve-registry-{}-{}-{}",
                name,
                std::process::id(),
                Canary::generate("FIXTURE").token()
            ));
            std::fs::create_dir_all(root.join("secretsieve")).expect("fixture directory");
            Self { root }
        }

        fn write_global(&self, contents: &str) {
            std::fs::write(self.root.join("secretsieve").join("config.toml"), contents)
                .expect("write global config");
        }

        fn environment(&self, pairs: &[(&str, &str)]) -> Environment {
            let mut variables: Vec<(String, String)> = vec![(
                "XDG_CONFIG_HOME".to_string(),
                self.root.to_string_lossy().into_owned(),
            )];
            variables.extend(
                pairs
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string())),
            );
            Environment::from_pairs(variables)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_valid_global_config_produces_an_active_registry() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let fixture = Fixture::new("valid");
        fixture
            .write_global("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"GITHUB_TOKEN\"\n");
        let environment = fixture.environment(&[("GITHUB_TOKEN", canary.value())]);

        match build(&environment) {
            Outcome::Ready(registry) => {
                assert_eq!(registry.redactor.active_count(), 1);
                assert!(registry.warnings.is_empty());
                assert!(registry.unresolved.is_empty());
            }
            Outcome::Malfunction(malfunction) => panic!("unexpected malfunction: {malfunction:?}"),
        }
    }

    #[test]
    fn a_missing_global_config_warns_but_still_works() {
        let fixture = Fixture::new("missing");
        match build(&fixture.environment(&[])) {
            Outcome::Ready(registry) => {
                assert!(registry.redactor.is_empty());
                assert_eq!(registry.warnings, vec![Warning::GlobalConfigMissing]);
            }
            Outcome::Malfunction(malfunction) => panic!("unexpected malfunction: {malfunction:?}"),
        }
    }

    #[test]
    fn an_invalid_global_config_disables_every_redaction() {
        let fixture = Fixture::new("invalid");
        fixture.write_global("version = 1\n\n[[secret]]\nsource = \"env\"\n");
        match build(&fixture.environment(&[])) {
            Outcome::Ready(_) => panic!("invalid config must not produce a registry"),
            Outcome::Malfunction(Malfunction::Config(error)) => {
                assert!(matches!(
                    error.kind,
                    crate::config::ConfigErrorKind::InvalidEntry { .. }
                ));
            }
            Outcome::Malfunction(other) => panic!("unexpected malfunction: {other:?}"),
        }
    }

    #[test]
    fn an_unresolved_source_stays_silent_and_does_not_fail_the_event() {
        let fixture = Fixture::new("unresolved");
        fixture.write_global(
            "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"PRESENT\"\n\n[[secret]]\nsource = \"env\"\nname = \"ABSENT\"\n",
        );
        let environment = fixture.environment(&[("PRESENT", "value")]);
        match build(&environment) {
            Outcome::Ready(registry) => {
                assert_eq!(registry.redactor.active_count(), 1);
                assert_eq!(registry.unresolved.len(), 1);
                assert!(registry.warnings.is_empty());
            }
            Outcome::Malfunction(malfunction) => panic!("unexpected malfunction: {malfunction:?}"),
        }
    }

    #[test]
    fn malfunction_messages_contain_no_path_or_file_text() {
        let fixture = Fixture::new("safe-messages");
        fixture.write_global("version = 1\nnot valid toml at all\n");
        match build(&fixture.environment(&[])) {
            Outcome::Malfunction(malfunction) => {
                let message = malfunction.message();
                assert!(!message.contains("not valid toml"));
                assert!(!message.contains(&fixture.root.to_string_lossy().into_owned()));
            }
            Outcome::Ready(_) => panic!("expected a malfunction"),
        }
    }
}
