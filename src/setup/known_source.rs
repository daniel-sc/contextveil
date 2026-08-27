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

const CLAUDE_ENV: [&str; 8] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_AWS_API_KEY",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "ANTHROPIC_FOUNDRY_AUTH_TOKEN",
    "AWS_BEARER_TOKEN_BEDROCK",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_CLIENT_KEY_PASSPHRASE",
];
const MCP_ENV: [&str; 8] = [
    "API_KEY",
    "ACCESS_TOKEN",
    "AUTH_TOKEN",
    "BEARER_TOKEN",
    "CLIENT_SECRET",
    "PASSWORD",
    "SECRET",
    "TOKEN",
];
const MCP_HEADERS: [&str; 6] = [
    "authorization",
    "proxy-authorization",
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-subscription-token",
];

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
        }
    }
}

#[derive(Debug, Default)]
pub struct Found {
    pub sources: Vec<SourceRef>,
    pub rules: HashMap<SourceId, Vec<Rule>>,
    pub notices: Vec<Notice>,
}

impl Found {
    fn mark_since(&mut self, start: usize, rule: Rule) {
        self.mark_since_where(start, rule, |_| true);
    }

    fn mark_since_where(&mut self, start: usize, rule: Rule, matches: impl Fn(&SourceRef) -> bool) {
        let ids: Vec<SourceId> = self.sources[start..]
            .iter()
            .filter(|source| matches(source))
            .map(SourceRef::id)
            .collect();
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
    codex(&mut found, environment, home, base);
    opencode(&mut found, environment, home, base);
    copilot(&mut found, environment, home, base);
    claude_machine(&mut found, environment, home, base);
    if environment
        .get_str("OPENCODE_AUTH_CONTENT")
        .is_some_and(|value| !value.is_empty())
    {
        let start = found.sources.len();
        found.sources.push(SourceRef::Env {
            name: "OPENCODE_AUTH_CONTENT".into(),
        });
        found.mark_since(start, Rule::OpenCodeAuthContent);
    }
    deduplicate(&mut found.sources);
    found
}

pub fn project(project_root: &Path, files: &ProjectFiles) -> Found {
    let mut found = Found::default();
    for path in &files.claude_settings {
        let start = found.sources.len();
        inspect(
            &mut found,
            path,
            project_entry(project_root, path),
            settings_sources,
        );
        found.mark_since(start, Rule::ClaudeConfiguredEnvironment);
    }
    for path in &files.claude_mcp {
        let start = found.sources.len();
        inspect(
            &mut found,
            path,
            project_entry(project_root, path),
            mcp_server_sources,
        );
        found.mark_since(start, Rule::ClaudeMcpServerCredentials);
    }
    deduplicate(&mut found.sources);
    found
}

fn codex(found: &mut Found, environment: &Environment, home: Option<&Path>, base: &Path) {
    for root in candidate_roots(
        home.map(|home| home.join(".codex")),
        environment.get("CODEX_HOME"),
        None,
        "CODEX_HOME",
        base,
        &mut found.notices,
    ) {
        let start = found.sources.len();
        inspect_at(
            found,
            &root.path.join("auth.json"),
            home,
            root.default,
            |value, path, entered, out| {
                probe_exact(
                    value,
                    path,
                    entered,
                    &[
                        "/OPENAI_API_KEY",
                        "/tokens/id_token",
                        "/tokens/access_token",
                        "/tokens/refresh_token",
                        "/personal_access_token",
                        "/bedrock_api_key/api_key",
                        "/agent_identity",
                        "/agent_identity/agent_private_key",
                    ],
                    out,
                );
            },
        );
        found.mark_since(start, Rule::CodexPrimaryCredentials);

        let start = found.sources.len();
        inspect_at(
            found,
            &root.path.join(".credentials.json"),
            home,
            root.default,
            |value, path, entered, out| {
                probe_immediate_children(
                    value,
                    "",
                    &[&["access_token"], &["refresh_token"]],
                    path,
                    entered,
                    out,
                );
            },
        );
        found.mark_since(start, Rule::CodexMcpCredentials);
    }
}

fn opencode(found: &mut Found, environment: &Environment, home: Option<&Path>, base: &Path) {
    for root in candidate_roots(
        home.map(|home| home.join(".local/share/opencode")),
        environment.get("XDG_DATA_HOME"),
        Some(Path::new("opencode")),
        "XDG_DATA_HOME",
        base,
        &mut found.notices,
    ) {
        let start = found.sources.len();
        inspect_at(
            found,
            &root.path.join("auth.json"),
            home,
            root.default,
            opencode_auth_sources,
        );
        found.mark_since(start, Rule::OpenCodeProviderCredentials);
        let start = found.sources.len();
        inspect_at(
            found,
            &root.path.join("mcp-auth.json"),
            home,
            root.default,
            opencode_mcp_auth_sources,
        );
        found.mark_since(start, Rule::OpenCodeMcpCredentials);
    }
}

fn opencode_auth_sources(value: &Value, path: &Path, entered: &str, out: &mut Vec<SourceRef>) {
    probe_immediate_children(
        value,
        "",
        &[&["key"], &["token"], &["access"], &["refresh"]],
        path,
        entered,
        out,
    );
}

fn opencode_mcp_auth_sources(value: &Value, path: &Path, entered: &str, out: &mut Vec<SourceRef>) {
    probe_immediate_children(
        value,
        "",
        &[
            &["tokens", "accessToken"],
            &["tokens", "refreshToken"],
            &["clientInfo", "clientSecret"],
            &["codeVerifier"],
        ],
        path,
        entered,
        out,
    );
}

fn copilot(found: &mut Found, environment: &Environment, home: Option<&Path>, base: &Path) {
    for root in candidate_roots(
        home.map(|home| home.join(".copilot")),
        environment.get("COPILOT_HOME"),
        None,
        "COPILOT_HOME",
        base,
        &mut found.notices,
    ) {
        let start = found.sources.len();
        inspect_at(
            found,
            &root.path.join("config.json"),
            home,
            root.default,
            |value, path, entered, out| {
                probe_immediate_string_map(value, "/copilotTokens", path, entered, out);
            },
        );
        found.mark_since(start, Rule::CopilotTokenConfiguration);

        let directory = root.path.join("mcp-oauth-config");
        if !std::fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.is_dir()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let discover = if name.strip_suffix(".tokens.json").is_some_and(is_hex64) {
                copilot_mcp_tokens_sources as fn(&Value, &Path, &str, &mut Vec<SourceRef>)
            } else if name.strip_suffix(".json").is_some_and(is_hex64) {
                copilot_mcp_client_sources
            } else {
                continue;
            };
            let start = found.sources.len();
            inspect_at(found, &path, home, root.default, discover);
            found.mark_since(start, Rule::CopilotMcpOauthCredentials);
        }
    }
}

fn copilot_mcp_tokens_sources(value: &Value, path: &Path, entered: &str, out: &mut Vec<SourceRef>) {
    probe_exact(
        value,
        path,
        entered,
        &["/access_token", "/refresh_token", "/id_token"],
        out,
    );
}

fn copilot_mcp_client_sources(value: &Value, path: &Path, entered: &str, out: &mut Vec<SourceRef>) {
    probe_exact(value, path, entered, &["/client_secret"], out);
}

fn claude_machine(found: &mut Found, environment: &Environment, home: Option<&Path>, base: &Path) {
    for root in candidate_roots(
        home.map(|home| home.join(".claude")),
        environment.get("CLAUDE_CONFIG_DIR"),
        None,
        "CLAUDE_CONFIG_DIR",
        base,
        &mut found.notices,
    ) {
        let start = found.sources.len();
        inspect_at(
            found,
            &root.path.join(".credentials.json"),
            home,
            root.default,
            |value, path, entered, out| {
                probe_exact(
                    value,
                    path,
                    entered,
                    &["/claudeAiOauth/accessToken", "/claudeAiOauth/refreshToken"],
                    out,
                );
                probe_immediate_children(
                    value,
                    "/mcpOAuth",
                    &[&["accessToken"], &["refreshToken"], &["clientSecret"]],
                    path,
                    entered,
                    out,
                );
                probe_immediate_children(
                    value,
                    "/mcpOAuthClientConfig",
                    &[&["clientSecret"]],
                    path,
                    entered,
                    out,
                );
            },
        );
        found.mark_since_where(start, Rule::ClaudePrimaryOauthCredentials, |source| {
            matches!(source, SourceRef::Json { pointer, .. } if pointer.starts_with("/claudeAiOauth/"))
        });
        found.mark_since_where(start, Rule::ClaudeMcpOauthState, |source| {
            matches!(source, SourceRef::Json { pointer, .. } if !pointer.starts_with("/claudeAiOauth/"))
        });
        let start = found.sources.len();
        inspect_at(
            found,
            &root.path.join("settings.json"),
            home,
            root.default,
            settings_sources,
        );
        found.mark_since(start, Rule::ClaudeConfiguredEnvironment);
        let start = found.sources.len();
        let state = if root.default {
            home.expect("default Claude root requires home")
                .join(".claude.json")
        } else {
            root.path.join(".claude.json")
        };
        let state = paths::normalize(&state);
        inspect_at(
            found,
            &state,
            home,
            root.default,
            |value, path, entered, out| {
                probe_immediate_children(
                    value,
                    "/mcpOAuth",
                    &[&["accessToken"], &["refreshToken"], &["clientSecret"]],
                    path,
                    entered,
                    out,
                );
                probe_immediate_children(
                    value,
                    "/mcpOAuthClientConfig",
                    &[&["clientSecret"]],
                    path,
                    entered,
                    out,
                );
                mcp_server_sources(value, path, entered, out);
            },
        );
        found.mark_since_where(start, Rule::ClaudeMcpServerCredentials, |source| {
            matches!(source, SourceRef::Json { pointer, .. } if pointer.starts_with("/mcpServers/"))
        });
        found.mark_since_where(start, Rule::ClaudeMcpOauthState, |source| {
            matches!(source, SourceRef::Json { pointer, .. } if !pointer.starts_with("/mcpServers/"))
        });
    }
}

fn settings_sources(value: &Value, path: &Path, entered: &str, out: &mut Vec<SourceRef>) {
    for name in CLAUDE_ENV {
        add_if_string(value, path, entered, &format!("/env/{name}"), out);
    }
}

fn mcp_server_sources(value: &Value, path: &Path, entered: &str, out: &mut Vec<SourceRef>) {
    let Some(servers) = value.get("mcpServers").and_then(Value::as_object) else {
        return;
    };
    for (server_name, server) in servers {
        if let Some(headers) = server.get("headers").and_then(Value::as_object) {
            for (name, value) in headers {
                if MCP_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                    add_dynamic(
                        Some(value),
                        path,
                        entered,
                        &["mcpServers", server_name, "headers", name],
                        out,
                    );
                }
            }
        }
        if let Some(env) = server.get("env").and_then(Value::as_object) {
            for name in MCP_ENV {
                add_dynamic(
                    env.get(name),
                    path,
                    entered,
                    &["mcpServers", server_name, "env", name],
                    out,
                );
            }
            for name in CLAUDE_ENV {
                add_dynamic(
                    env.get(name),
                    path,
                    entered,
                    &["mcpServers", server_name, "env", name],
                    out,
                );
            }
        }
    }
}

fn probe_exact(
    value: &Value,
    path: &Path,
    entered: &str,
    pointers: &[&str],
    out: &mut Vec<SourceRef>,
) {
    for pointer in pointers {
        add_if_string(value, path, entered, pointer, out);
    }
}

fn probe_immediate_children(
    value: &Value,
    container: &str,
    leaves: &[&[&str]],
    path: &Path,
    entered: &str,
    out: &mut Vec<SourceRef>,
) {
    let Some(entries) = value.pointer(container).and_then(Value::as_object) else {
        return;
    };
    for (name, entry) in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        for leaf in leaves {
            let mut tokens = container_tokens(container);
            tokens.push(name.as_str());
            tokens.extend_from_slice(leaf);
            let mut selected = None;
            for (index, token) in leaf.iter().enumerate() {
                selected = if index == 0 {
                    entry.get(token)
                } else {
                    selected.and_then(|value: &Value| value.get(token))
                };
            }
            add_dynamic(selected, path, entered, &tokens, out);
        }
    }
}

fn probe_immediate_string_map(
    value: &Value,
    container: &str,
    path: &Path,
    entered: &str,
    out: &mut Vec<SourceRef>,
) {
    let Some(entries) = value.pointer(container).and_then(Value::as_object) else {
        return;
    };
    for (name, value) in entries {
        let mut tokens = container_tokens(container);
        tokens.push(name.as_str());
        add_dynamic(Some(value), path, entered, &tokens, out);
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

fn inspect_at<F>(found: &mut Found, path: &Path, home: Option<&Path>, default: bool, discover: F)
where
    F: FnOnce(&Value, &Path, &str, &mut Vec<SourceRef>),
{
    let entered = if default {
        home_entry(home, path)
    } else {
        path.to_str().map(str::to_string)
    };
    inspect(found, path, entered, discover);
}

fn inspect<F>(found: &mut Found, path: &Path, entered: Option<String>, discover: F)
where
    F: FnOnce(&Value, &Path, &str, &mut Vec<SourceRef>),
{
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
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            unavailable(found, path, "it could not be read");
            return;
        }
    };
    match file.metadata() {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            unavailable(found, path, "it could not be read");
            return;
        }
    }
    let mut bytes = Vec::new();
    match file.read_to_end(&mut bytes) {
        Ok(_) => {}
        Err(_) => {
            unavailable(found, path, "it could not be read");
            return;
        }
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            unavailable(found, path, "it is not valid UTF-8");
            return;
        }
    };
    match json::parse(&text) {
        Ok(value) => discover(&value, path, &entered, &mut found.sources),
        Err(_) => unavailable(found, path, "it is malformed JSON"),
    }
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

    fn machine_pointers(tree: &Tree) -> Vec<String> {
        let home = tree.0.join("home");
        let environment = Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
        pointers(&machine(&environment, Some(&home), &tree.0))
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
    fn lowercase_hash_names_are_pinned() {
        assert!(is_hex64(&"a".repeat(64)));
        assert!(!is_hex64(&"A".repeat(64)));
        assert!(!is_hex64(&"a".repeat(63)));
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
    fn codex_probes_each_bounded_credential_field_independently() {
        let tree = Tree::new();
        tree.write("home/.codex/auth.json", r#"{"OPENAI_API_KEY":"a","tokens":{"access_token":"b"},"agent_identity":{"agent_private_key":"c"},"ignored":"d"}"#);
        tree.write(
            "home/.codex/.credentials.json",
            r#"{
                "valid":{"server_name":"server","server_url":"https://example.test","client_id":"client","access_token":"d","refresh_token":"e","metadata":{"ignored":true}},
                "without_refresh":{"server_name":"server","server_url":"https://example.test","client_id":"client","access_token":"f"},
                "null_refresh":{"server_name":"server","server_url":"https://example.test","client_id":"client","access_token":"g","refresh_token":null},
                "incomplete":{"server_name":"server","access_token":"h"},
                "wrong_type":{"server_name":"server","server_url":"https://example.test","client_id":7,"access_token":"i"},
                "bad_refresh":{"server_name":"server","server_url":"https://example.test","client_id":"client","access_token":"j","refresh_token":7}
            }"#,
        );
        let home = tree.0.join("home");
        let environment = Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
        let found = machine(&environment, Some(&home), &tree.0);
        let pointers = pointers(&found);
        for expected in [
            "/OPENAI_API_KEY",
            "/tokens/access_token",
            "/agent_identity/agent_private_key",
            "/valid/access_token",
            "/valid/refresh_token",
            "/without_refresh/access_token",
            "/null_refresh/access_token",
            "/incomplete/access_token",
            "/wrong_type/access_token",
            "/bad_refresh/access_token",
        ] {
            assert!(
                pointers.iter().any(|pointer| pointer == expected),
                "missing {expected}: {pointers:?}"
            );
        }
        let rejected = "/ignored";
        assert!(
            !pointers.iter().any(|pointer| pointer == rejected),
            "accepted {rejected}"
        );
        assert!(found.notices.is_empty());
    }

    #[test]
    fn opencode_auth_probes_direct_fields_without_schema_gating() {
        let tree = Tree::new();
        tree.write(
            "home/.local/share/opencode/auth.json",
            r#"{
                "api":{"type":"api","key":"api-canary","metadata":{"region":"test"},"nearbySecret":"ignored"},
                "oauth":{"type":"oauth","refresh":"refresh-canary","access":"access-canary","expires":0,"accountId":"account","enterpriseUrl":"https://example.test","token":"ignored"},
                "wellknown":{"type":"wellknown","key":"identifier","token":"token-canary","access":"ignored"},
                "incomplete_api":{"type":"api"},
                "empty_api":{"type":"api","key":""},
                "incomplete_oauth":{"type":"oauth","refresh":"refresh-canary","access":"access-canary"},
                "incomplete_wellknown":{"type":"wellknown","token":"token-canary"},
                "wrong_metadata":{"type":"api","key":"api-canary","metadata":{"region":7}},
                "wrong_expires":{"type":"oauth","refresh":"refresh-canary","access":"access-canary","expires":-1},
                "wrong_optional":{"type":"oauth","refresh":"refresh-canary","access":"access-canary","expires":1,"accountId":7},
                "unknown":{"type":"future","key":"ignored","token":"ignored"}
            }"#,
        );
        assert_eq!(
            machine_pointers(&tree),
            vec![
                "/api/key",
                "/oauth/token",
                "/oauth/access",
                "/oauth/refresh",
                "/wellknown/key",
                "/wellknown/token",
                "/wellknown/access",
                "/incomplete_oauth/access",
                "/incomplete_oauth/refresh",
                "/incomplete_wellknown/token",
                "/wrong_metadata/key",
                "/wrong_expires/access",
                "/wrong_expires/refresh",
                "/wrong_optional/access",
                "/wrong_optional/refresh",
                "/unknown/key",
                "/unknown/token",
            ]
        );
    }

    #[test]
    fn opencode_auth_stays_bounded_and_ignores_unlisted_fields() {
        let tree = Tree::new();
        tree.write(
            "home/.local/share/opencode/auth.json",
            r#"{
                "oauth":{"type":"oauth","refresh":"","access":"","expires":12,"extraToken":"ignored"},
                "wellknown":{"type":"wellknown","key":"","token":"","secret":"ignored"},
                "unknown":{"type":"future","nested":{"key":"ignored"}}
            }"#,
        );
        assert!(machine_pointers(&tree).is_empty());
    }

    #[test]
    fn opencode_mcp_probes_each_server_entry_independently() {
        let tree = Tree::new();
        let path = "home/.local/share/opencode/mcp-auth.json";
        tree.write(
            path,
            r#"{
                "a~b/srv":{"tokens":{"accessToken":"access-canary","refreshToken":"refresh-canary","nearby":"ignored"},"clientInfo":{"clientId":"client","clientSecret":"secret-canary","nearby":"ignored"},"codeVerifier":"verifier-canary","oauthState":"state-is-not-a-credential","serverUrl":"https://example.test","accessToken":"ignored"},
                "optional":{"unknown":{"clientSecret":"ignored"}}
            }"#,
        );
        assert_eq!(
            machine_pointers(&tree),
            vec![
                "/a~0b~1srv/tokens/accessToken",
                "/a~0b~1srv/tokens/refreshToken",
                "/a~0b~1srv/clientInfo/clientSecret",
                "/a~0b~1srv/codeVerifier",
            ]
        );

        tree.write(
            path,
            r#"{"valid":{"tokens":{"accessToken":"access-canary"}},"incomplete":{"tokens":{"refreshToken":"refresh-canary"}}}"#,
        );
        assert_eq!(
            machine_pointers(&tree),
            vec![
                "/valid/tokens/accessToken",
                "/incomplete/tokens/refreshToken"
            ]
        );

        tree.write(
            path,
            r#"{"valid":{"tokens":{"accessToken":"access-canary"}},"wrong":{"clientInfo":{"clientId":7}}}"#,
        );
        assert_eq!(machine_pointers(&tree), vec!["/valid/tokens/accessToken"]);

        tree.write(
            path,
            r#"{"no-match":{"oauthState":"state","serverUrl":"https://example.test","unknown":{"accessToken":"ignored"}}}"#,
        );
        assert!(machine_pointers(&tree).is_empty());

        tree.write(path, r#"[{"tokens":{"accessToken":"access-canary"}}]"#);
        assert!(machine_pointers(&tree).is_empty());
    }

    #[test]
    fn copilot_probes_bounded_token_containers_and_hashed_files() {
        let tree = Tree::new();
        tree.write(
            "home/.copilot/config.json",
            r#"{"copilotTokens":{"github.com":"n"},"token":"o"}"#,
        );
        let hash = "a".repeat(64);
        tree.write(
            &format!("home/.copilot/mcp-oauth-config/{hash}.tokens.json"),
            r#"{"access_token":"p","unknown":"q"}"#,
        );
        tree.write(
            "home/.copilot/mcp-oauth-config/not-a-hash.tokens.json",
            r#"{"access_token":"rejected"}"#,
        );
        let home = tree.0.join("home");
        let environment = Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
        assert_eq!(
            pointers(&machine(&environment, Some(&home), &tree.0)),
            vec!["/copilotTokens/github.com", "/access_token"]
        );
    }

    #[test]
    fn copilot_comment_bearing_config_is_inspected_as_json5() {
        let tree = Tree::new();
        tree.write(
            "home/.copilot/config.json",
            r#"{
                // Copilot CLI writes this configuration with comments.
                copilotTokens: {
                    'github.com': 'token-canary',
                },
            }"#,
        );

        let home = tree.0.join("home");
        let environment = Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
        let found = machine(&environment, Some(&home), &tree.0);

        assert_eq!(pointers(&found), vec!["/copilotTokens/github.com"]);
        assert!(found.notices.is_empty());
    }

    #[test]
    fn copilot_mcp_token_fields_are_independent() {
        let tree = Tree::new();
        let hash = "b".repeat(64);
        let path = format!("home/.copilot/mcp-oauth-config/{hash}.tokens.json");
        tree.write(
            &path,
            r#"{"access_token":"access-canary","refresh_token":"refresh-canary","id_token":"id-canary","nearby_secret":"ignored"}"#,
        );
        assert_eq!(
            machine_pointers(&tree),
            vec!["/access_token", "/refresh_token", "/id_token"]
        );

        tree.write(&path, r#"{"refresh_token":"refresh-canary"}"#);
        assert_eq!(machine_pointers(&tree), vec!["/refresh_token"]);

        tree.write(
            &path,
            r#"{"access_token":"access-canary","refresh_token":7}"#,
        );
        assert_eq!(machine_pointers(&tree), vec!["/access_token"]);

        tree.write(
            &path,
            r#"{"access_token":"","refresh_token":"","id_token":"","token":"ignored"}"#,
        );
        assert!(machine_pointers(&tree).is_empty());

        tree.write(&path, r#"["access-canary"]"#);
        assert!(machine_pointers(&tree).is_empty());
    }

    #[test]
    fn copilot_mcp_client_secret_does_not_require_client_id() {
        let tree = Tree::new();
        let hash = "c".repeat(64);
        let path = format!("home/.copilot/mcp-oauth-config/{hash}.json");
        tree.write(
            &path,
            r#"{"client_id":"client-canary","client_secret":"secret-canary","access_token":"ignored"}"#,
        );
        assert_eq!(machine_pointers(&tree), vec!["/client_secret"]);

        tree.write(&path, r#"{"client_secret":"secret-canary"}"#);
        assert_eq!(machine_pointers(&tree), vec!["/client_secret"]);

        tree.write(&path, r#"{"client_id":"client-canary","client_secret":7}"#);
        assert!(machine_pointers(&tree).is_empty());

        tree.write(
            &path,
            r#"{"client_id":"client-canary","nearby_secret":"ignored"}"#,
        );
        assert!(machine_pointers(&tree).is_empty());

        tree.write(&path, r#"["client-canary"]"#);
        assert!(machine_pointers(&tree).is_empty());
    }

    #[test]
    fn claude_probes_bounded_primary_oauth_and_mcp_fields() {
        let tree = Tree::new();
        tree.write(
            "home/.claude/.credentials.json",
            r#"{"claudeAiOauth":{"accessToken":"r"},"mcpOAuth":{"srv":{"clientSecret":"s"}}}"#,
        );
        tree.write(
            "home/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_API_KEY":"t","RANDOM_TOKEN":"u"}}"#,
        );
        tree.write(
            "home/.claude.json",
            r#"{"mcpOAuthClientConfig":{"srv":{"clientSecret":"v"}}}"#,
        );
        let environment = Environment::from_pairs([
            ("HOME", tree.0.join("home").to_string_lossy().into_owned()),
            ("OPENCODE_AUTH_CONTENT", "whole".to_string()),
        ]);
        let found = machine(&environment, Some(&tree.0.join("home")), &tree.0);
        let pointers = pointers(&found);
        for expected in [
            "/env/ANTHROPIC_API_KEY",
            "/mcpOAuthClientConfig/srv/clientSecret",
            "env:OPENCODE_AUTH_CONTENT",
        ] {
            assert!(
                pointers.iter().any(|pointer| pointer == expected),
                "missing {expected}: {pointers:?}"
            );
        }
        for expected in ["/claudeAiOauth/accessToken", "/mcpOAuth/srv/clientSecret"] {
            assert!(
                pointers.iter().any(|pointer| pointer == expected),
                "missing {expected}: {pointers:?}"
            );
        }
        assert!(
            !pointers
                .iter()
                .any(|pointer| pointer == "/env/RANDOM_TOKEN")
        );
        assert!(found.notices.is_empty());
    }

    #[test]
    fn unrepresentable_dynamic_pointer_tokens_are_skipped_without_panicking() {
        let tree = Tree::new();
        tree.write(
            "home/.copilot/config.json",
            r#"{"copilotTokens":{"*":"value","":"value"}}"#,
        );
        tree.write(
            "home/.local/share/opencode/auth.json",
            r#"{"":{"type":"api","key":"value"}}"#,
        );
        let home = tree.0.join("home");
        let environment = Environment::from_pairs([("HOME", home.to_string_lossy().into_owned())]);
        let found = machine(&environment, Some(&home), &tree.0);
        assert!(found.sources.is_empty());
        assert!(found.notices.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn codex_filesystem_matrix_covers_primary_mcp_override_and_failure_boundaries() {
        let tree = Tree::new();
        let canary = Canary::generate("CODEX_MATRIX");
        let target = tree.write(
            "targets/codex-auth.json",
            &format!(
                r#"{{"OPENAI_API_KEY":"{}","unrelated":"ignored"}}"#,
                canary.value()
            ),
        );
        std::fs::create_dir_all(tree.0.join("home/.codex")).expect("Codex root");
        std::os::unix::fs::symlink(&target, tree.0.join("home/.codex/auth.json"))
            .expect("Codex exact-file symlink");
        tree.write(
            "home/.codex/.credentials.json",
            &format!(
                r#"{{"server":{{"server_name":"server","server_url":"https://example.test","client_id":"client","access_token":"{}"}}}}"#,
                canary.value()
            ),
        );
        tree.write(
            "home/unrelated/auth.json",
            &format!(r#"{{"OPENAI_API_KEY":"{}"}}"#, canary.value()),
        );
        let (home, environment) = default_environment(&tree);
        let found = machine(&environment, Some(&home), &tree.0);
        assert_eq!(
            pointers(&found),
            vec!["/OPENAI_API_KEY", "/server/access_token"]
        );
        assert!(found.notices.is_empty());
        assert_found_is_canary_free(&found, &canary);

        std::fs::remove_file(tree.0.join("home/.codex/auth.json")).expect("remove symlink");
        tree.write("home/.codex/auth.json", r#"{"ordinary":"value"}"#);
        tree.write("home/.codex/.credentials.json", "{");
        let found = machine(&environment, Some(&home), &tree.0);
        assert!(found.sources.is_empty());
        assert_eq!(found.notices.len(), 1);
        assert_found_is_canary_free(&found, &canary);

        tree.write(
            "override/codex/auth.json",
            &format!(r#"{{"tokens":{{"access_token":"{}"}}}}"#, canary.value()),
        );
        let override_environment = Environment::from_pairs([
            ("HOME", home.to_string_lossy().into_owned()),
            (
                "CODEX_HOME",
                tree.0.join("override/codex").to_string_lossy().into_owned(),
            ),
        ]);
        let found = machine(&override_environment, Some(&home), &tree.0);
        assert!(pointers(&found).contains(&"/tokens/access_token".to_string()));
        assert_found_is_canary_free(&found, &canary);
    }

    #[test]
    #[cfg(unix)]
    fn opencode_filesystem_matrix_covers_primary_mcp_override_and_failure_boundaries() {
        let tree = Tree::new();
        let canary = Canary::generate("OPENCODE_MATRIX");
        let target = tree.write(
            "targets/opencode-auth.json",
            &format!(
                r#"{{"provider":{{"type":"api","key":"{}","ignored":"nearby"}}}}"#,
                canary.value()
            ),
        );
        std::fs::create_dir_all(tree.0.join("home/.local/share/opencode")).expect("OpenCode root");
        std::os::unix::fs::symlink(&target, tree.0.join("home/.local/share/opencode/auth.json"))
            .expect("OpenCode exact-file symlink");
        tree.write(
            "home/.local/share/opencode/mcp-auth.json",
            &format!(
                r#"{{"server":{{"tokens":{{"accessToken":"{}"}}}}}}"#,
                canary.value()
            ),
        );
        tree.write(
            "home/unrelated/mcp-auth.json",
            &format!(
                r#"{{"server":{{"tokens":{{"accessToken":"{}"}}}}}}"#,
                canary.value()
            ),
        );
        let (home, environment) = default_environment(&tree);
        let found = machine(&environment, Some(&home), &tree.0);
        assert_eq!(
            pointers(&found),
            vec!["/provider/key", "/server/tokens/accessToken"]
        );
        assert!(found.notices.is_empty());
        assert_found_is_canary_free(&found, &canary);

        std::fs::remove_file(tree.0.join("home/.local/share/opencode/auth.json"))
            .expect("remove symlink");
        tree.write(
            "home/.local/share/opencode/auth.json",
            r#"{"provider":{"type":"future","key":"value"}}"#,
        );
        tree.write("home/.local/share/opencode/mcp-auth.json", "{");
        let found = machine(&environment, Some(&home), &tree.0);
        assert_eq!(pointers(&found), vec!["/provider/key"]);
        assert_eq!(found.notices.len(), 1);
        assert_found_is_canary_free(&found, &canary);

        tree.write(
            "override/data/opencode/auth.json",
            &format!(
                r#"{{"provider":{{"type":"api","key":"{}"}}}}"#,
                canary.value()
            ),
        );
        let override_environment = Environment::from_pairs([
            ("HOME", home.to_string_lossy().into_owned()),
            (
                "XDG_DATA_HOME",
                tree.0.join("override/data").to_string_lossy().into_owned(),
            ),
        ]);
        let found = machine(&override_environment, Some(&home), &tree.0);
        assert!(pointers(&found).contains(&"/provider/key".to_string()));
        assert_found_is_canary_free(&found, &canary);
    }

    #[test]
    #[cfg(unix)]
    fn copilot_filesystem_matrix_covers_primary_mcp_override_and_failure_boundaries() {
        let tree = Tree::new();
        let canary = Canary::generate("COPILOT_MATRIX");
        let target = tree.write(
            "targets/copilot-config.json",
            &format!(
                r#"{{"copilotTokens":{{"github.com":"{}"}},"unrelated":"ignored"}}"#,
                canary.value()
            ),
        );
        std::fs::create_dir_all(tree.0.join("home/.copilot/mcp-oauth-config"))
            .expect("Copilot MCP root");
        std::os::unix::fs::symlink(&target, tree.0.join("home/.copilot/config.json"))
            .expect("Copilot exact-file symlink");
        let hash = "a".repeat(64);
        tree.write(
            &format!("home/.copilot/mcp-oauth-config/{hash}.tokens.json"),
            &format!(
                r#"{{"access_token":"{}","unrelated":"ignored"}}"#,
                canary.value()
            ),
        );
        tree.write(
            "home/unrelated/config.json",
            &format!(
                r#"{{"copilotTokens":{{"github.com":"{}"}}}}"#,
                canary.value()
            ),
        );
        let (home, environment) = default_environment(&tree);
        let found = machine(&environment, Some(&home), &tree.0);
        assert_eq!(
            pointers(&found),
            vec!["/copilotTokens/github.com", "/access_token"]
        );
        assert!(found.notices.is_empty());
        assert_found_is_canary_free(&found, &canary);

        std::fs::remove_file(tree.0.join("home/.copilot/config.json")).expect("remove symlink");
        tree.write(
            "home/.copilot/config.json",
            r#"{"copilotTokens":{"github.com":7}}"#,
        );
        tree.write(
            &format!("home/.copilot/mcp-oauth-config/{hash}.tokens.json"),
            "{",
        );
        let found = machine(&environment, Some(&home), &tree.0);
        assert!(found.sources.is_empty());
        assert_eq!(found.notices.len(), 1);
        assert_found_is_canary_free(&found, &canary);

        tree.write(
            "override/copilot/config.json",
            &format!(
                r#"{{"copilotTokens":{{"github.com":"{}"}}}}"#,
                canary.value()
            ),
        );
        let override_environment = Environment::from_pairs([
            ("HOME", home.to_string_lossy().into_owned()),
            (
                "COPILOT_HOME",
                tree.0
                    .join("override/copilot")
                    .to_string_lossy()
                    .into_owned(),
            ),
        ]);
        let found = machine(&override_environment, Some(&home), &tree.0);
        assert!(pointers(&found).contains(&"/copilotTokens/github.com".to_string()));
        assert_found_is_canary_free(&found, &canary);
    }

    #[test]
    #[cfg(unix)]
    fn claude_filesystem_matrix_covers_platform_primary_mcp_override_and_failure_boundaries() {
        let tree = Tree::new();
        let canary = Canary::generate("CLAUDE_MATRIX");
        let settings_target = tree.write(
            "targets/claude-settings.json",
            &format!(
                r#"{{"env":{{"ANTHROPIC_API_KEY":"{}","UNRELATED":"ignored"}}}}"#,
                canary.value()
            ),
        );
        std::fs::create_dir_all(tree.0.join("home/.claude")).expect("Claude root");
        std::os::unix::fs::symlink(&settings_target, tree.0.join("home/.claude/settings.json"))
            .expect("Claude exact-file symlink");
        tree.write(
            "home/.claude/.credentials.json",
            &format!(
                r#"{{"claudeAiOauth":{{"accessToken":"{}"}}}}"#,
                canary.value()
            ),
        );
        tree.write(
            "home/.claude.json",
            &format!(
                r#"{{"mcpOAuth":{{"server":{{"clientSecret":"{}"}}}},"unrelated":"ignored"}}"#,
                canary.value()
            ),
        );
        tree.write(
            "home/unrelated/settings.json",
            &format!(r#"{{"env":{{"ANTHROPIC_API_KEY":"{}"}}}}"#, canary.value()),
        );
        let (home, environment) = default_environment(&tree);
        let found = machine(&environment, Some(&home), &tree.0);
        let found_pointers = pointers(&found);
        assert!(found_pointers.contains(&"/env/ANTHROPIC_API_KEY".to_string()));
        assert!(found_pointers.contains(&"/mcpOAuth/server/clientSecret".to_string()));
        #[cfg(not(target_os = "macos"))]
        assert!(found_pointers.contains(&"/claudeAiOauth/accessToken".to_string()));
        #[cfg(target_os = "macos")]
        assert!(!found_pointers.contains(&"/claudeAiOauth/accessToken".to_string()));
        assert!(found.notices.is_empty());
        assert_found_is_canary_free(&found, &canary);

        std::fs::remove_file(tree.0.join("home/.claude/settings.json")).expect("remove symlink");
        tree.write(
            "home/.claude/settings.json",
            r#"{"env":{"UNRELATED":"value"}}"#,
        );
        tree.write("home/.claude.json", "{");
        let found = machine(&environment, Some(&home), &tree.0);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(pointers(&found), vec!["/claudeAiOauth/accessToken"]);
        #[cfg(target_os = "macos")]
        assert!(found.sources.is_empty());
        assert_eq!(found.notices.len(), 1);
        assert_found_is_canary_free(&found, &canary);

        tree.write(
            "override/claude/settings.json",
            &format!(
                r#"{{"env":{{"ANTHROPIC_AUTH_TOKEN":"{}"}}}}"#,
                canary.value()
            ),
        );
        let override_environment = Environment::from_pairs([
            ("HOME", home.to_string_lossy().into_owned()),
            (
                "CLAUDE_CONFIG_DIR",
                tree.0
                    .join("override/claude")
                    .to_string_lossy()
                    .into_owned(),
            ),
        ]);
        let found = machine(&override_environment, Some(&home), &tree.0);
        assert!(pointers(&found).contains(&"/env/ANTHROPIC_AUTH_TOKEN".to_string()));
        assert_found_is_canary_free(&found, &canary);
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
