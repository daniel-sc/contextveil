//! Known Source Rule identities and bounded setup-time credential discovery.
//!
//! Every match becomes an ordinary environment or exact JSON reference.
//! Transformed values, keychains, helpers, and broad directory recursion are
//! deliberately outside this module.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::json::{self, Value};
use crate::paths;
use crate::sanitize;
use crate::secret::SourceId;
use crate::source::{Environment, SourceRef};

use super::discovery::ProjectFiles;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rule {
    SecretLikeName,
    CredentialBearingUrl,
    CodexPrimaryCredentials,
    CodexMcpCredentials,
    OpenCodeProviderCredentials,
    OpenCodeMcpCredentials,
    OpenCodeAuthContent,
    CopilotTokenConfiguration,
    CopilotMcpOauthCredentials,
    ClaudePrimaryOauthCredentials,
    ClaudeConfiguredEnvironment,
    ClaudeMcpOauthState,
    ClaudeMcpServerCredentials,
    PropertiesConfiguration,
}

impl Rule {
    pub fn display(self) -> &'static str {
        match self {
            Self::SecretLikeName => "secret-like source name",
            Self::CredentialBearingUrl => "credential-bearing URL",
            Self::CodexPrimaryCredentials => "Codex primary credentials",
            Self::CodexMcpCredentials => "Codex MCP credentials",
            Self::OpenCodeProviderCredentials => "OpenCode provider credentials",
            Self::OpenCodeMcpCredentials => "OpenCode MCP credentials",
            Self::OpenCodeAuthContent => "OpenCode whole environment credential content",
            Self::CopilotTokenConfiguration => "Copilot token configuration",
            Self::CopilotMcpOauthCredentials => "Copilot MCP OAuth credentials",
            Self::ClaudePrimaryOauthCredentials => "Claude primary OAuth credentials",
            Self::ClaudeConfiguredEnvironment => "Claude configured environment credentials",
            Self::ClaudeMcpOauthState => "Claude MCP OAuth state",
            Self::ClaudeMcpServerCredentials => "Claude MCP server credentials",
            Self::PropertiesConfiguration => "properties configuration",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RootSpec {
    default: &'static str,
    override_name: &'static str,
    override_suffix: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum Location {
    File(&'static str),
    HomeOrRootFile(&'static str),
    Hex64File {
        directory: &'static str,
        suffix: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
enum KeyMatch {
    Any,
    Exact(&'static [&'static str]),
    AsciiCaseInsensitive(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
enum Probe {
    Exact {
        pointers: &'static [&'static str],
        rule: Rule,
    },
    ImmediateChildren {
        container: &'static str,
        leaves: &'static [&'static [&'static str]],
        rule: Rule,
    },
    Map {
        container: &'static str,
        keys: KeyMatch,
        rule: Rule,
    },
    NestedMaps {
        container: &'static str,
        maps: &'static [MapSpec],
        rule: Rule,
    },
}

#[derive(Debug, Clone, Copy)]
struct MapSpec {
    nested: &'static str,
    keys: KeyMatch,
}

#[derive(Debug, Clone, Copy)]
struct DocumentSpec {
    location: Location,
    probes: &'static [Probe],
}

#[derive(Debug, Clone, Copy)]
struct MachineSpec {
    root: RootSpec,
    documents: &'static [DocumentSpec],
}

const CLAUDE_ENV_KEYS: KeyMatch = KeyMatch::Exact(&[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_AWS_API_KEY",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "ANTHROPIC_FOUNDRY_AUTH_TOKEN",
    "AWS_BEARER_TOKEN_BEDROCK",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_CLIENT_KEY_PASSPHRASE",
]);
const MCP_ENV_KEYS: KeyMatch = KeyMatch::Exact(&[
    "API_KEY",
    "ACCESS_TOKEN",
    "AUTH_TOKEN",
    "BEARER_TOKEN",
    "CLIENT_SECRET",
    "PASSWORD",
    "SECRET",
    "TOKEN",
]);
const MCP_HEADER_KEYS: KeyMatch = KeyMatch::AsciiCaseInsensitive(&[
    "authorization",
    "proxy-authorization",
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-subscription-token",
]);

const CODEX_PRIMARY_PROBES: &[Probe] = &[Probe::Exact {
    pointers: &[
        "/OPENAI_API_KEY",
        "/tokens/id_token",
        "/tokens/access_token",
        "/tokens/refresh_token",
        "/personal_access_token",
        "/bedrock_api_key/api_key",
        "/agent_identity",
        "/agent_identity/agent_private_key",
    ],
    rule: Rule::CodexPrimaryCredentials,
}];
const CODEX_MCP_PROBES: &[Probe] = &[Probe::ImmediateChildren {
    container: "",
    leaves: &[&["access_token"], &["refresh_token"]],
    rule: Rule::CodexMcpCredentials,
}];
const OPENCODE_PROVIDER_PROBES: &[Probe] = &[Probe::ImmediateChildren {
    container: "",
    leaves: &[&["key"], &["token"], &["access"], &["refresh"]],
    rule: Rule::OpenCodeProviderCredentials,
}];
const OPENCODE_MCP_PROBES: &[Probe] = &[Probe::ImmediateChildren {
    container: "",
    leaves: &[
        &["tokens", "accessToken"],
        &["tokens", "refreshToken"],
        &["clientInfo", "clientSecret"],
        &["codeVerifier"],
    ],
    rule: Rule::OpenCodeMcpCredentials,
}];
const COPILOT_CONFIG_PROBES: &[Probe] = &[Probe::Map {
    container: "/copilotTokens",
    keys: KeyMatch::Any,
    rule: Rule::CopilotTokenConfiguration,
}];
const COPILOT_TOKEN_PROBES: &[Probe] = &[Probe::Exact {
    pointers: &["/access_token", "/refresh_token", "/id_token"],
    rule: Rule::CopilotMcpOauthCredentials,
}];
const COPILOT_CLIENT_PROBES: &[Probe] = &[Probe::Exact {
    pointers: &["/client_secret"],
    rule: Rule::CopilotMcpOauthCredentials,
}];
const CLAUDE_CREDENTIALS_PROBES: &[Probe] = &[
    // Claude's primary credential is keychain-backed on macOS.
    #[cfg(not(target_os = "macos"))]
    Probe::Exact {
        pointers: &["/claudeAiOauth/accessToken", "/claudeAiOauth/refreshToken"],
        rule: Rule::ClaudePrimaryOauthCredentials,
    },
    Probe::ImmediateChildren {
        container: "/mcpOAuth",
        leaves: &[&["accessToken"], &["refreshToken"], &["clientSecret"]],
        rule: Rule::ClaudeMcpOauthState,
    },
    Probe::ImmediateChildren {
        container: "/mcpOAuthClientConfig",
        leaves: &[&["clientSecret"]],
        rule: Rule::ClaudeMcpOauthState,
    },
];
const CLAUDE_SETTINGS_PROBES: &[Probe] = &[Probe::Map {
    container: "/env",
    keys: CLAUDE_ENV_KEYS,
    rule: Rule::ClaudeConfiguredEnvironment,
}];
const MCP_SERVER_MAPS: &[MapSpec] = &[
    MapSpec {
        nested: "headers",
        keys: MCP_HEADER_KEYS,
    },
    MapSpec {
        nested: "env",
        keys: MCP_ENV_KEYS,
    },
    MapSpec {
        nested: "env",
        keys: CLAUDE_ENV_KEYS,
    },
];
const CLAUDE_STATE_PROBES: &[Probe] = &[
    Probe::ImmediateChildren {
        container: "/mcpOAuth",
        leaves: &[&["accessToken"], &["refreshToken"], &["clientSecret"]],
        rule: Rule::ClaudeMcpOauthState,
    },
    Probe::ImmediateChildren {
        container: "/mcpOAuthClientConfig",
        leaves: &[&["clientSecret"]],
        rule: Rule::ClaudeMcpOauthState,
    },
    Probe::NestedMaps {
        container: "/mcpServers",
        maps: MCP_SERVER_MAPS,
        rule: Rule::ClaudeMcpServerCredentials,
    },
];
const PROJECT_MCP_PROBES: &[Probe] = &[Probe::NestedMaps {
    container: "/mcpServers",
    maps: MCP_SERVER_MAPS,
    rule: Rule::ClaudeMcpServerCredentials,
}];

const CODEX_DOCUMENTS: &[DocumentSpec] = &[
    DocumentSpec {
        location: Location::File("auth.json"),
        probes: CODEX_PRIMARY_PROBES,
    },
    DocumentSpec {
        location: Location::File(".credentials.json"),
        probes: CODEX_MCP_PROBES,
    },
];
const OPENCODE_DOCUMENTS: &[DocumentSpec] = &[
    DocumentSpec {
        location: Location::File("auth.json"),
        probes: OPENCODE_PROVIDER_PROBES,
    },
    DocumentSpec {
        location: Location::File("mcp-auth.json"),
        probes: OPENCODE_MCP_PROBES,
    },
];
const COPILOT_DOCUMENTS: &[DocumentSpec] = &[
    DocumentSpec {
        location: Location::File("config.json"),
        probes: COPILOT_CONFIG_PROBES,
    },
    DocumentSpec {
        location: Location::Hex64File {
            directory: "mcp-oauth-config",
            suffix: ".tokens.json",
        },
        probes: COPILOT_TOKEN_PROBES,
    },
    DocumentSpec {
        location: Location::Hex64File {
            directory: "mcp-oauth-config",
            suffix: ".json",
        },
        probes: COPILOT_CLIENT_PROBES,
    },
];
const CLAUDE_DOCUMENTS: &[DocumentSpec] = &[
    DocumentSpec {
        location: Location::File(".credentials.json"),
        probes: CLAUDE_CREDENTIALS_PROBES,
    },
    DocumentSpec {
        location: Location::File("settings.json"),
        probes: CLAUDE_SETTINGS_PROBES,
    },
    DocumentSpec {
        location: Location::HomeOrRootFile(".claude.json"),
        probes: CLAUDE_STATE_PROBES,
    },
];
const MACHINE_SPECS: &[MachineSpec] = &[
    MachineSpec {
        root: RootSpec {
            default: ".codex",
            override_name: "CODEX_HOME",
            override_suffix: "",
        },
        documents: CODEX_DOCUMENTS,
    },
    MachineSpec {
        root: RootSpec {
            default: ".local/share/opencode",
            override_name: "XDG_DATA_HOME",
            override_suffix: "opencode",
        },
        documents: OPENCODE_DOCUMENTS,
    },
    MachineSpec {
        root: RootSpec {
            default: ".copilot",
            override_name: "COPILOT_HOME",
            override_suffix: "",
        },
        documents: COPILOT_DOCUMENTS,
    },
    MachineSpec {
        root: RootSpec {
            default: ".claude",
            override_name: "CLAUDE_CONFIG_DIR",
            override_suffix: "",
        },
        documents: CLAUDE_DOCUMENTS,
    },
];

const ENVIRONMENT_SOURCES: &[(&str, Rule)] =
    &[("OPENCODE_AUTH_CONTENT", Rule::OpenCodeAuthContent)];

#[derive(Debug, Default)]
pub struct Found {
    pub sources: Vec<SourceRef>,
    pub rules: HashMap<SourceId, Vec<Rule>>,
    pub notices: Vec<Notice>,
}

impl Found {
    fn mark_since(&mut self, start: usize, rule: Rule) {
        let ids: Vec<SourceId> = self.sources[start..].iter().map(SourceRef::id).collect();
        for id in ids {
            let rules = self.rules.entry(id).or_default();
            if !rules.contains(&rule) {
                rules.push(rule);
            }
        }
    }
}

#[derive(Debug)]
pub struct Notice {
    pub display: String,
    pub reason: &'static str,
}

pub fn machine(environment: &Environment, home: Option<&Path>, base: &Path) -> Found {
    let mut found = Found::default();
    for spec in MACHINE_SPECS {
        let default = home.map(|home| home.join(spec.root.default));
        for root in candidate_roots(
            default,
            environment.get(spec.root.override_name),
            if spec.root.override_suffix.is_empty() {
                None
            } else {
                Some(Path::new(spec.root.override_suffix))
            },
            spec.root.override_name,
            base,
            &mut found.notices,
        ) {
            for document in spec.documents {
                inspect_location(&mut found, &root, home, document);
            }
        }
    }
    for (name, rule) in ENVIRONMENT_SOURCES {
        if environment
            .get_str(name)
            .is_some_and(|value| !value.is_empty())
        {
            let start = found.sources.len();
            found.sources.push(SourceRef::Env {
                name: (*name).into(),
            });
            found.mark_since(start, *rule);
        }
    }
    let mut gradle_roots = Vec::new();
    if let Some(home) = home {
        gradle_roots.push((paths::normalize(&home.join(".gradle")), true));
    }
    if let Some(value) = environment.get("GRADLE_USER_HOME") {
        match value.to_str() {
            Some("") => {}
            Some(value) => {
                let path = explicit_path(value, base);
                if !gradle_roots.iter().any(|(known, _)| known == &path) {
                    gradle_roots.push((path, false));
                }
            }
            None => found.notices.push(Notice {
                display: "GRADLE_USER_HOME".to_string(),
                reason: "its override is not valid UTF-8",
            }),
        }
    }
    for (root, default) in gradle_roots {
        let path = root.join("gradle.properties");
        let entered = if default {
            home_entry(home, &path)
        } else {
            path.to_str().map(str::to_string)
        };
        inspect_properties_document(&mut found, &path, entered);
    }
    deduplicate(&mut found.sources);
    found
}

pub fn project(project_root: &Path, files: &ProjectFiles) -> Found {
    let mut found = Found::default();
    for path in &files.claude_settings {
        inspect_document(
            &mut found,
            path,
            project_entry(project_root, path),
            CLAUDE_SETTINGS_PROBES,
        );
    }
    for path in &files.claude_mcp {
        inspect_document(
            &mut found,
            path,
            project_entry(project_root, path),
            PROJECT_MCP_PROBES,
        );
    }
    for file in &files.properties {
        let Some(entered) = file.entered.as_deref() else {
            unavailable(&mut found, &file.path, "its path is not valid UTF-8");
            continue;
        };
        match &file.state {
            super::discovery::PropertiesState::Unavailable(why) => {
                unavailable(&mut found, &file.path, why.reason());
            }
            super::discovery::PropertiesState::Available(properties) => {
                add_properties_candidates(&mut found, &file.path, entered, properties);
            }
        }
    }
    deduplicate(&mut found.sources);
    found
}

fn inspect_properties_document(found: &mut Found, path: &Path, entered: Option<String>) {
    let Some(entered) = entered else {
        unavailable(found, path, "its path is not valid UTF-8");
        return;
    };
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            unavailable(found, path, "it could not be read");
            return;
        }
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            unavailable(found, path, "it could not be read");
            return;
        }
    };
    match crate::properties::parse(&bytes) {
        Ok(properties) => add_properties_candidates(found, path, &entered, &properties),
        Err(_) => unavailable(found, path, "it is malformed properties"),
    }
}

fn add_properties_candidates(
    found: &mut Found,
    path: &Path,
    entered: &str,
    properties: &crate::properties::Properties,
) {
    for (key, value) in properties.entries() {
        let value = value.trim();
        if value.is_empty()
            || (super::vocabulary::gating_term(key).is_none()
                && !super::credential_url::is_credential_bearing(value))
        {
            continue;
        }
        let source = SourceRef::Properties {
            entered: entered.to_string(),
            path: path.to_path_buf(),
            key: key.to_string(),
        };
        let id = source.id();
        found.sources.push(source);
        found
            .rules
            .entry(id)
            .or_default()
            .push(Rule::PropertiesConfiguration);
    }
}

fn inspect_location(found: &mut Found, root: &Root, home: Option<&Path>, document: &DocumentSpec) {
    match document.location {
        Location::File(file) => {
            inspect_at(
                found,
                &root.path.join(file),
                home,
                root.default,
                document.probes,
            );
        }
        Location::HomeOrRootFile(file) => {
            let path = if root.default {
                root.path
                    .parent()
                    .expect("default root has a parent")
                    .join(file)
            } else {
                root.path.join(file)
            };
            inspect_at(found, &path, home, root.default, document.probes);
        }
        Location::Hex64File { directory, suffix } => {
            let directory = root.path.join(directory);
            if !std::fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.is_dir()) {
                return;
            }
            let Ok(entries) = std::fs::read_dir(directory) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
                    continue;
                }
                let file_name = entry.file_name();
                let Some(name) = file_name.to_str() else {
                    continue;
                };
                if !name.strip_suffix(suffix).is_some_and(is_hex64) {
                    continue;
                }
                inspect_at(found, &path, home, root.default, document.probes);
            }
        }
    }
}

fn inspect_at(
    found: &mut Found,
    path: &Path,
    home: Option<&Path>,
    default: bool,
    probes: &[Probe],
) {
    let entered = if default {
        home_entry(home, path)
    } else {
        path.to_str().map(str::to_string)
    };
    inspect_document(found, path, entered, probes);
}

fn inspect_document(found: &mut Found, path: &Path, entered: Option<String>, probes: &[Probe]) {
    let Some(entered) = entered else {
        unavailable(found, path, "its path is not valid UTF-8");
        return;
    };
    let Some(value) = read_document(found, path) else {
        return;
    };
    for probe in probes {
        let start = found.sources.len();
        apply_probe(probe, &value, path, &entered, &mut found.sources);
        found.mark_since(start, probe.rule());
    }
}

fn read_document(found: &mut Found, path: &Path) -> Option<Value> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            unavailable(found, path, "it could not be read");
            return None;
        }
    }
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            unavailable(found, path, "it could not be read");
            return None;
        }
    };
    match file.metadata() {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            unavailable(found, path, "it could not be read");
            return None;
        }
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        unavailable(found, path, "it could not be read");
        return None;
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            unavailable(found, path, "it is not valid UTF-8");
            return None;
        }
    };
    match json::parse(&text) {
        Ok(value) => Some(value),
        Err(_) => {
            unavailable(found, path, "it is malformed JSON");
            None
        }
    }
}

impl Probe {
    fn rule(self) -> Rule {
        match self {
            Self::Exact { rule, .. }
            | Self::ImmediateChildren { rule, .. }
            | Self::Map { rule, .. }
            | Self::NestedMaps { rule, .. } => rule,
        }
    }
}

fn apply_probe(probe: &Probe, value: &Value, path: &Path, entered: &str, out: &mut Vec<SourceRef>) {
    match probe {
        Probe::Exact { pointers, .. } => {
            for pointer in *pointers {
                add_if_string(value, path, entered, pointer, out);
            }
        }
        Probe::ImmediateChildren {
            container, leaves, ..
        } => {
            let Some(entries) = value.pointer(container).and_then(Value::as_object) else {
                return;
            };
            for (name, entry) in entries {
                let Some(entry) = entry.as_object() else {
                    continue;
                };
                for leaf in *leaves {
                    let mut tokens = container_tokens(container);
                    tokens.push(name.as_str());
                    let mut selected = None;
                    for (index, token) in leaf.iter().enumerate() {
                        selected = if index == 0 {
                            entry.get(token)
                        } else {
                            selected.and_then(|value: &Value| value.get(token))
                        };
                    }
                    tokens.extend_from_slice(leaf);
                    add_dynamic(selected, path, entered, &tokens, out);
                }
            }
        }
        Probe::Map {
            container, keys, ..
        } => {
            let Some(entries) = value.pointer(container).and_then(Value::as_object) else {
                return;
            };
            for (name, entry) in entries {
                if key_matches(*keys, name) {
                    let mut tokens = container_tokens(container);
                    tokens.push(name.as_str());
                    add_dynamic(Some(entry), path, entered, &tokens, out);
                }
            }
        }
        Probe::NestedMaps {
            container, maps, ..
        } => {
            let Some(entries) = value.pointer(container).and_then(Value::as_object) else {
                return;
            };
            for (name, entry) in entries {
                for map in *maps {
                    let Some(values) = entry.get(map.nested).and_then(Value::as_object) else {
                        continue;
                    };
                    for (key, value) in values {
                        if !key_matches(map.keys, key) {
                            continue;
                        }
                        let mut tokens = container_tokens(container);
                        tokens.push(name.as_str());
                        tokens.push(map.nested);
                        tokens.push(key.as_str());
                        add_dynamic(Some(value), path, entered, &tokens, out);
                    }
                }
            }
        }
    }
}

fn key_matches(keys: KeyMatch, name: &str) -> bool {
    match keys {
        KeyMatch::Any => true,
        KeyMatch::Exact(names) => names.contains(&name),
        KeyMatch::AsciiCaseInsensitive(names) => {
            names.iter().any(|key| name.eq_ignore_ascii_case(key))
        }
    }
}

fn container_tokens(container: &str) -> Vec<&str> {
    container
        .strip_prefix('/')
        .into_iter()
        .flat_map(|container| container.split('/'))
        .collect()
}

fn add_if_string(
    value: &Value,
    path: &Path,
    entered: &str,
    pointer: &str,
    out: &mut Vec<SourceRef>,
) {
    if value
        .pointer(pointer)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
    {
        add_source(path, entered, pointer.to_string(), out);
    }
}

fn add_dynamic(
    value: Option<&Value>,
    path: &Path,
    entered: &str,
    tokens: &[&str],
    out: &mut Vec<SourceRef>,
) {
    if tokens.iter().any(|token| token.is_empty() || *token == "*")
        || !value
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
    {
        return;
    }
    let pointer = tokens
        .iter()
        .map(|token| json::encode_token(token))
        .collect::<Vec<_>>()
        .join("/");
    add_source(path, entered, format!("/{pointer}"), out);
}

fn add_source(path: &Path, entered: &str, pointer: String, out: &mut Vec<SourceRef>) {
    if json::final_token(&pointer).is_err() {
        return;
    }
    out.push(SourceRef::Json {
        entered: entered.into(),
        path: path.to_path_buf(),
        pointer,
    });
}

fn unavailable(found: &mut Found, path: &Path, reason: &'static str) {
    found.notices.push(Notice {
        display: sanitize::path(path),
        reason,
    });
}

#[derive(Debug)]
struct Root {
    path: PathBuf,
    default: bool,
}

fn candidate_roots(
    default: Option<PathBuf>,
    override_value: Option<&std::ffi::OsStr>,
    override_suffix: Option<&Path>,
    override_name: &str,
    invocation_directory: &Path,
    notices: &mut Vec<Notice>,
) -> Vec<Root> {
    let mut roots = Vec::new();
    if let Some(path) = default {
        roots.push(Root {
            path: paths::normalize(&path),
            default: true,
        });
    }

    if let Some(value) = override_value {
        match value.to_str() {
            Some("") => {}
            Some(value) => {
                let mut path = explicit_path(value, invocation_directory);
                if let Some(suffix) = override_suffix {
                    path.push(suffix);
                }
                let path = paths::normalize(&path);
                if !roots.iter().any(|root| root.path == path) {
                    roots.push(Root {
                        path,
                        default: false,
                    });
                }
            }
            None => notices.push(Notice {
                display: override_name.to_string(),
                reason: "its override is not valid UTF-8",
            }),
        }
    }

    roots
}

fn explicit_path(value: &str, base: &Path) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        paths::normalize(path)
    } else {
        paths::normalize(&base.join(path))
    }
}

fn home_entry(home: Option<&Path>, path: &Path) -> Option<String> {
    let home = home?;
    let home = paths::normalize(home);
    let path = paths::normalize(path);
    path.strip_prefix(home)
        .ok()?
        .to_str()
        .map(|tail| format!("~/{tail}"))
}

fn project_entry(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root).ok()?.to_str().map(str::to_string)
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn deduplicate(sources: &mut Vec<SourceRef>) {
    let mut seen = HashSet::new();
    sources.retain(|source| seen.insert(source.id()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Canary, assert_canary_absent};

    struct Tree(PathBuf);

    impl Tree {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "contextveil-known-source-{}-{}",
                std::process::id(),
                crate::testing::Canary::generate("KNOWN").token()
            ));
            std::fs::create_dir_all(&path).expect("fixture root");
            Self(path)
        }

        fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("directories");
            std::fs::write(&path, contents).expect("fixture file");
            path
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn pointers(found: &Found) -> Vec<String> {
        found
            .sources
            .iter()
            .filter_map(|source| match source {
                SourceRef::Json { pointer, .. } => Some(pointer.clone()),
                SourceRef::Env { name } => Some(format!("env:{name}")),
                _ => None,
            })
            .collect()
    }

    fn assert_found_is_canary_free(found: &Found, canary: &Canary) {
        let source_metadata = format!("{:?}", found.sources);
        assert_canary_absent(
            "Known Source SourceRef metadata",
            source_metadata.as_bytes(),
            canary,
        );
        let source_ids = found.sources.iter().map(SourceRef::id).collect::<Vec<_>>();
        let config_metadata = format!("{source_ids:?}");
        assert_canary_absent(
            "Known Source config-facing identities",
            config_metadata.as_bytes(),
            canary,
        );
        for notice in &found.notices {
            assert_canary_absent(
                "Known Source notice path",
                notice.display.as_bytes(),
                canary,
            );
            assert_canary_absent(
                "Known Source notice reason",
                notice.reason.as_bytes(),
                canary,
            );
        }
    }

    fn default_environment(tree: &Tree) -> (PathBuf, Environment) {
        let home = tree.0.join("home");
        let environment = Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
        (home, environment)
    }

    #[test]
    fn explicit_overrides_are_literal_normalized_paths() {
        assert_eq!(
            explicit_path("~/../stores", Path::new("/project")),
            PathBuf::from("/project/stores")
        );
        assert_eq!(
            explicit_path("sub/../stores", Path::new("/project")),
            PathBuf::from("/project/stores")
        );
    }

    #[test]
    fn candidate_roots_are_additive_and_deduplicate_normalized_paths() {
        let mut notices = Vec::new();
        let roots = candidate_roots(
            Some(PathBuf::from("/home/default/../default")),
            Some(std::ffi::OsStr::new("./default")),
            None,
            "CODEX_HOME",
            Path::new("/home"),
            &mut notices,
        );

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, PathBuf::from("/home/default"));
        assert!(roots[0].default);
        assert!(notices.is_empty());
    }

    #[test]
    fn probes_are_bounded_and_accept_only_non_empty_strings() {
        let value = json::parse(
            r#"{
                "plain":"one",
                "providers":{"good":{"key":"two"},"bad":{"key":7},"*":{"key":"ignored"},"":{"key":"ignored"}},
                "tokens":{"good":"three","empty":"","number":4},
                "servers":{"a~b/srv":{"headers":{"Authorization":"four","Cookie":"ignored"},"env":{"TOKEN":"five","OTHER":"ignored"}}}
            }"#,
        )
        .expect("probe document");
        let mut sources = Vec::new();
        let path = Path::new("credentials.json");
        let entered = "credentials.json";

        apply_probe(
            &Probe::Exact {
                pointers: &["/plain"],
                rule: Rule::CodexPrimaryCredentials,
            },
            &value,
            path,
            entered,
            &mut sources,
        );
        apply_probe(
            &Probe::ImmediateChildren {
                container: "/providers",
                leaves: &[&["key"]],
                rule: Rule::OpenCodeProviderCredentials,
            },
            &value,
            path,
            entered,
            &mut sources,
        );
        apply_probe(
            &Probe::Map {
                container: "/tokens",
                keys: KeyMatch::Any,
                rule: Rule::CopilotTokenConfiguration,
            },
            &value,
            path,
            entered,
            &mut sources,
        );
        apply_probe(
            &Probe::NestedMaps {
                container: "/servers",
                maps: &[
                    MapSpec {
                        nested: "headers",
                        keys: MCP_HEADER_KEYS,
                    },
                    MapSpec {
                        nested: "env",
                        keys: MCP_ENV_KEYS,
                    },
                ],
                rule: Rule::ClaudeMcpServerCredentials,
            },
            &value,
            path,
            entered,
            &mut sources,
        );

        assert_eq!(
            pointers(&Found {
                sources,
                rules: HashMap::new(),
                notices: Vec::new(),
            }),
            vec![
                "/plain",
                "/providers/good/key",
                "/tokens/good",
                "/servers/a~0b~1srv/headers/Authorization",
                "/servers/a~0b~1srv/env/TOKEN",
            ]
        );
    }

    #[test]
    fn overrides_are_resolved_at_discovery_time_and_persist_explicit_paths() {
        let tree = Tree::new();
        tree.write(
            "project/stores/codex/auth.json",
            r#"{"OPENAI_API_KEY":"value"}"#,
        );
        tree.write(
            "project/stores/open/opencode/auth.json",
            r#"{"p":{"type":"api","key":"value"}}"#,
        );
        tree.write(
            "project/stores/copilot/config.json",
            r#"{"copilotTokens":{"p":"value"}}"#,
        );
        tree.write(
            "project/stores/claude/settings.json",
            r#"{"env":{"ANTHROPIC_API_KEY":"value"}}"#,
        );
        let environment = Environment::from_pairs([
            ("CODEX_HOME", "stores/codex"),
            ("XDG_DATA_HOME", "stores/open"),
            ("COPILOT_HOME", "stores/copilot"),
            ("CLAUDE_CONFIG_DIR", "stores/claude"),
        ]);
        let found = machine(&environment, None, &tree.0.join("project"));
        assert_eq!(found.sources.len(), 4);
        assert!(found.sources.iter().all(|source| match source {
            SourceRef::Json { entered, path, .. } => {
                entered == &path.to_string_lossy()
                    && path.starts_with(tree.0.join("project/stores"))
                    && !entered.contains('~')
            }
            _ => false,
        }));
    }

    #[test]
    fn valid_no_match_is_silent_but_invalid_matched_json_is_noticed() {
        let tree = Tree::new();
        tree.write("home/.codex/auth.json", r#"{"unrelated":"value"}"#);
        tree.write("home/.local/share/opencode/auth.json", "{");
        tree.write("home/.copilot/config.json", r#"{"copilotTokens":{"x":1}}"#);
        let home = tree.0.join("home");
        let environment = Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
        let found = machine(&environment, Some(&home), &tree.0);
        assert!(found.sources.is_empty());
        assert_eq!(found.notices.len(), 1);
        assert!(found.notices[0].display.ends_with("opencode/auth.json"));
        assert_eq!(found.notices[0].reason, "it is malformed JSON");
    }

    #[test]
    fn project_matches_use_relative_paths_and_exact_mcp_names() {
        let tree = Tree::new();
        let settings = tree.write(
            "project/app/.claude/settings.json",
            r#"{"env":{"CLAUDE_CODE_OAUTH_TOKEN":"a","OTHER_TOKEN":"b"}}"#,
        );
        let mcp = tree.write("project/app/.mcp.json", r#"{"mcpServers":{"srv/a":{"headers":{"authorization":"c","Cookie":"d"},"env":{"TOKEN":"e","TOKEN_FILE":"f"}}}}"#);
        let files = ProjectFiles {
            dotenv: vec![],
            properties: vec![],
            claude_settings: vec![settings],
            claude_mcp: vec![mcp],
        };
        let found = project(&tree.0.join("project"), &files);
        let pointers = pointers(&found);
        assert_eq!(
            pointers,
            vec![
                "/env/CLAUDE_CODE_OAUTH_TOKEN",
                "/mcpServers/srv~1a/headers/authorization",
                "/mcpServers/srv~1a/env/TOKEN",
            ]
        );
        assert!(found.sources.iter().all(|source| match source {
            SourceRef::Json { entered, .. } => entered.starts_with("app/"),
            _ => false,
        }));
    }

    #[test]
    fn claude_user_state_uses_the_same_narrow_mcp_server_fields() {
        let tree = Tree::new();
        tree.write("home/.claude.json", r#"{"mcpServers":{"srv":{"headers":{"X-Api-Key":"a","Cookie":"b"},"env":{"CLIENT_SECRET":"c","CLIENT_ID":"d"}}}}"#);
        let home = tree.0.join("home");
        let environment = Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
        assert_eq!(
            pointers(&machine(&environment, Some(&home), &tree.0)),
            vec![
                "/mcpServers/srv/headers/X-Api-Key",
                "/mcpServers/srv/env/CLIENT_SECRET",
            ]
        );
    }

    #[test]
    #[cfg(unix)]
    fn unreadable_matched_json_produces_a_safe_notice() {
        use std::os::unix::fs::PermissionsExt;
        let tree = Tree::new();
        let path = tree.write(
            "home/.codex/auth.json",
            r#"{"OPENAI_API_KEY":"never reported"}"#,
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .expect("permissions");
        if std::fs::read(&path).is_err() {
            let home = tree.0.join("home");
            let environment =
                Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
            let found = machine(&environment, Some(&home), &tree.0);
            assert!(found.sources.is_empty());
            assert_eq!(found.notices.len(), 1);
            assert_eq!(found.notices[0].reason, "it could not be read");
        }
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    #[test]
    #[cfg(unix)]
    fn exact_machine_fifo_and_symlink_to_fifo_are_skipped_promptly() {
        let tree = Tree::new();
        let fifo = tree.0.join("home/.codex/auth.json");
        std::fs::create_dir_all(fifo.parent().expect("FIFO parent")).expect("Codex root");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(status.success());

        let symlink = tree.0.join("home/.local/share/opencode/auth.json");
        std::fs::create_dir_all(symlink.parent().expect("symlink parent")).expect("OpenCode root");
        std::os::unix::fs::symlink(&fifo, &symlink).expect("symlink to FIFO");

        let (home, environment) = default_environment(&tree);
        let started = std::time::Instant::now();
        let found = machine(&environment, Some(&home), &tree.0);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "Known Source discovery attempted to open a FIFO"
        );
        assert!(found.sources.is_empty());
        assert!(found.notices.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn copilot_mcp_oauth_directory_symlink_is_not_traversed() {
        let tree = Tree::new();
        let canary = Canary::generate("COPILOT_DIRECTORY_SYMLINK");
        let hash = "d".repeat(64);
        tree.write(
            &format!("outside/{hash}.tokens.json"),
            &format!(r#"{{"access_token":"{}"}}"#, canary.value()),
        );
        std::fs::create_dir_all(tree.0.join("home/.copilot")).expect("Copilot root");
        std::os::unix::fs::symlink(
            tree.0.join("outside"),
            tree.0.join("home/.copilot/mcp-oauth-config"),
        )
        .expect("directory symlink");

        let (home, environment) = default_environment(&tree);
        let found = machine(&environment, Some(&home), &tree.0);
        assert!(found.sources.is_empty());
        assert!(found.notices.is_empty());
        assert_found_is_canary_free(&found, &canary);
    }

    #[test]
    #[cfg(unix)]
    fn exact_machine_symlinks_are_followed_but_project_symlinks_are_not_walked() {
        let tree = Tree::new();
        let target = tree.write("target.json", r#"{"OPENAI_API_KEY":"value"}"#);
        std::fs::create_dir_all(tree.0.join("home/.codex")).expect("codex directory");
        std::os::unix::fs::symlink(target, tree.0.join("home/.codex/auth.json"))
            .expect("file symlink");
        let home = tree.0.join("home");
        let environment = Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
        assert_eq!(machine(&environment, Some(&home), &tree.0).sources.len(), 1);

        tree.write(
            "outside/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_API_KEY":"value"}}"#,
        );
        std::fs::create_dir_all(tree.0.join("project")).expect("project");
        std::os::unix::fs::symlink(tree.0.join("outside"), tree.0.join("project/linked"))
            .expect("directory symlink");
        let walked = super::super::discovery::project_files(&tree.0.join("project"));
        assert!(walked.claude_settings.is_empty());
    }
}
