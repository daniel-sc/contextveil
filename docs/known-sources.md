# Known Source Rule Inventory

A **Known Source Rule** is a maintained, deterministic setup-time rule that
automatically admits candidates. Rules are advisory and version-sensitive, not
adapter coverage guarantees. Every applicable rule runs regardless of which
adapters are selected, installed, or detected. The user still chooses what to
enroll.

This is not a guarantee that an adapter covers a host or that every host
credential is found; there is no runtime `KnownSource` source type. Selected
candidates persist as ordinary explicit source references. Private schema
evidence may come from a shipped artifact, not represented as public source-code contracts.

Filesystem enumeration only identifies inputs that rules may inspect. Manual
environment, dotenv, and JSON source additions are user choices. Neither is a
Known Source Rule.

`Supported` rows are the V1 contract. `Planned` rows are clearly non-contract
roadmap notes: setup does not currently scan them, and they provide no current
coverage.

## Shared Semantics

- `CODEX_HOME`, `COPILOT_HOME`, `CLAUDE_CONFIG_DIR`, and `XDG_DATA_HOME` are
  resolved when setup runs. Changing one requires rerunning setup.
- A relative override is relative to setup's invocation directory. Overrides
  receive lexical `.`/`..` normalization, but no shell, environment-variable,
  glob, or tilde expansion.
- An empty override uses the default. A non-UTF-8 override is shown as
  unavailable and does not fall back to the default.
- Default machine paths are persisted using `~/...`; override paths are
  persisted as resolved explicit paths.
- Exact machine file paths may be symlinks when the target is a regular file.
  The bounded project traversal does not follow file or directory symlinks and
  applies normal project discovery exclusions.
- Enrolled or discovered documents persisted with `source = "json"` use the
  full JSON5 grammar. Duplicate object members remain invalid. Selection uses an
  exact RFC 6901 JSON Pointer and only a non-empty selected string resolves.
  Documents deeper than 128 object or array containers are malformed.
  Harness protocols and integration files remain strict unless separately
  specified. See [`ADR-0002`](adr/0002-json-sources-use-json5.md).
- Valid recognized-path documents with no matching schema silently produce no
  candidate. Malformed, non-UTF-8, duplicate-member, or unreadable recognized
  documents are shown as unavailable.
- Dynamic object members produce candidates only when each member name can be
  represented by an exact `CFG-016` JSON Pointer. Empty names and `*` silently
  produce no candidate.

## Rule Inventory

| Rule scope | Status | Exact admission scope and details | Evidence |
| --- | --- | --- | --- |
| Secret-like source names | Supported | Environment and discovered dotenv sources are admitted under `SET-006` when ASCII case-folded tokenization or compact suffix matching finds the exact maintained vocabulary in that requirement. Format, entropy, length, and source type do not independently admit a candidate. | Normative scope: [`SET-006`](../specification.md). Current implementation evidence: `src/setup/vocabulary.rs` and its unit fixtures. |
| Credential-bearing URLs | Supported | Environment and discovered dotenv values are admitted when they are absolute hierarchical URLs with an authority and non-empty password in userinfo. The complete URL is the candidate. JSON sources and other structured sources are not recursively inspected by this rule. | Normative scope: [`SET-017`](../specification.md). Current implementation evidence: `src/setup/credential_url.rs` and setup fixtures. |
| Codex primary credentials | Supported | Root is `CODEX_HOME`, or `~/.codex` when unset or empty. In `auth.json`, recognize `/OPENAI_API_KEY`; `/tokens/id_token`; `/tokens/access_token`; `/tokens/refresh_token`; `/personal_access_token`; `/bedrock_api_key/api_key`; and either string `/agent_identity` or `/agent_identity/agent_private_key`. | [`openai/codex@ff0e950`](https://github.com/openai/codex/commit/ff0e95007cca1edfc0877bbbbfaeb9eb77ed92b3); issue-time check [`openai/codex@d9fd91e`](https://github.com/openai/codex/commit/d9fd91edab298c2423c0c82526513e4e000284cf). Current fixtures: `src/setup/known_source.rs`. |
| Codex MCP credentials | Supported | Under the same root, inspect `.credentials.json`. For each immediate object member, recognize `access_token` and optional string `refresh_token` only when `server_name`, `server_url`, `client_id`, and `access_token` are strings and `refresh_token` is absent, null, or a string. | Same pinned Codex commits above; current schema and filesystem fixtures in `src/setup/known_source.rs`. |
| OpenCode provider credentials | Supported | Root is `${XDG_DATA_HOME}/opencode`, or `~/.local/share/opencode` when unset or empty. In `auth.json`, for each immediate provider object recognize `key` when `type` is `api`; `access` and `refresh` when `type` is `oauth`; and `token` when `type` is `wellknown`. | OpenCode 1.18.18, [`anomalyco/opencode@31406cc`](https://github.com/anomalyco/opencode/commit/31406ccc51b4bd2a4e1e086b2bcaa5f7f804f26d); current fixtures in `src/setup/known_source.rs`. |
| OpenCode MCP credentials | Supported | Under the same root, inspect `mcp-auth.json`. For each immediate server object recognize `tokens.accessToken`, `tokens.refreshToken`, `clientInfo.clientSecret`, and `codeVerifier`. | Same pinned OpenCode commit above; current schema and filesystem fixtures in `src/setup/known_source.rs`. |
| OpenCode whole environment credential content | Supported | A non-empty `OPENCODE_AUTH_CONTENT` is admitted as one whole environment source. It is not parsed into derived references. | Same pinned OpenCode commit above; current setup fixtures in `src/setup/known_source.rs` and `tests/setup.rs`. |
| Copilot token configuration | Supported | Root is `COPILOT_HOME`, or `~/.copilot` when unset or empty. In the JSON source `config.json`, admit every non-empty string value in the immediate `copilotTokens` object. Full JSON5 support includes the common comment-bearing configuration form. | Copilot CLI 1.0.80, [`github/copilot-cli@ef627e1`](https://github.com/github/copilot-cli/commit/ef627e1baad937d3c8da45f8a5541c6fc3c97b6a); official docs [`github/docs@838d187`](https://github.com/github/docs/commit/838d18789ba2c51cfe5544b3e5bf1ca3168c2795). The private structure is derived from the shipped artifact; JSON5 and comment-bearing configuration fixtures are in `src/json.rs` and `src/setup/known_source.rs`. |
| Copilot MCP OAuth credentials | Supported | Inspect only immediate regular files under `mcp-oauth-config`. For a basename of exactly 64 lowercase hexadecimal characters, `<hash>.tokens.json` recognizes top-level `access_token`, `refresh_token`, and `id_token`; `<hash>.json` recognizes top-level `client_secret`. | Same pinned Copilot release and official docs above; private structures are derived from the shipped artifact; current path/schema fixtures in `src/setup/known_source.rs`. |
| Claude primary OAuth credentials | Supported | Machine root is `CLAUDE_CONFIG_DIR`, or `~/.claude` when unset or empty. On non-macOS only, machine `.credentials.json` recognizes `/claudeAiOauth/accessToken` and `/claudeAiOauth/refreshToken`. macOS primary credentials are keychain-backed and not queried. | Claude Code 2.1.238, [`anthropics/claude-code@8a8e81d`](https://github.com/anthropics/claude-code/commit/8a8e81d098cbd0fae4ee5b9c853542945fe87016); private structures are derived from the shipped artifact; current fixtures in `src/setup/known_source.rs`. |
| Claude configured environment credentials | Supported | Machine `<root>/settings.json` and project `.claude/settings.json` at any depth recognize immediate `/env` strings named exactly `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_AWS_API_KEY`, `ANTHROPIC_FOUNDRY_API_KEY`, `ANTHROPIC_FOUNDRY_AUTH_TOKEN`, `AWS_BEARER_TOKEN_BEDROCK`, `CLAUDE_CODE_OAUTH_TOKEN`, or `CLAUDE_CODE_CLIENT_KEY_PASSPHRASE`. | Same pinned Claude release above; current exact-name and anchored-path fixtures in `src/setup/known_source.rs` and `src/setup/discovery.rs`. |
| Claude MCP OAuth state | Supported | In non-macOS machine `.credentials.json` and machine `.claude.json`, each immediate `/mcpOAuth` entry recognizes `accessToken`, `refreshToken`, and `clientSecret`; each immediate `/mcpOAuthClientConfig` entry recognizes `clientSecret`. With `CLAUDE_CONFIG_DIR`, the user-state file is `<root>/.claude.json`; otherwise it is `~/.claude.json`. | Same pinned Claude release above; private structures are derived from the shipped artifact; current schema fixtures in `src/setup/known_source.rs`. |
| Claude MCP server credentials | Supported | In machine `.claude.json` and project `.mcp.json` at any depth, inspect each immediate `mcpServers` member. String header names match case-insensitively only `authorization`, `proxy-authorization`, `x-api-key`, `api-key`, `x-auth-token`, or `x-subscription-token`. String environment names match exactly `API_KEY`, `ACCESS_TOKEN`, `AUTH_TOKEN`, `BEARER_TOKEN`, `CLIENT_SECRET`, `PASSWORD`, `SECRET`, `TOKEN`, or one of the eight Claude names in the preceding row. | Same pinned Claude release above; current exact-field and anchored-path fixtures in `src/setup/known_source.rs` and `src/setup/discovery.rs`. |
| npmrc credentials | Planned | Non-contract. npmrc files and credential entries are not scanned, admitted, or covered by the current rule inventory. Exact paths, grammar, and credential keys remain undecided. | Roadmap only; no implementation or conformance evidence. |
| Recognized INI credential stores | Planned | Non-contract. INI files are not generically scanned, and no INI store schema family has current coverage. Any future support must name bounded stores, paths, grammar, and exact credential fields. | Roadmap only; no implementation or conformance evidence. |

## Boundaries

Recognized store rules do not query OS keychains, execute credential helpers,
read raw credential sidecars, decode values, or promise coverage for a future
host version. Copilot `.secret` and `.verifier` files and `mcp-secrets` fallback
files remain unsupported. Manually enroll a representable environment, dotenv,
or JSON source when possible, and rerun setup after host or path changes. See
[`LIM-023`](../limitations.md#lim-023-known-source-rules-are-advisory).
