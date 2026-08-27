# Known Source Rule Inventory

A **Known Source Rule** is a maintained, deterministic setup-time rule that
automatically admits candidates. Credential document rules are bounded location
and field probes: a listed non-empty string is admitted without validating
unrelated surrounding schema. Rules are advisory, run independently of
adapters, and persist only ordinary explicit source references. They do not
recursively classify arbitrary structured files.

All machine default and valid override roots are inspected additively. Unset or
empty overrides add nothing; relative overrides resolve from setup's invocation
directory and receive lexical `.`/`..` normalization only. No shell, environment,
glob, or tilde expansion occurs. Default paths persist as `~/...`; override paths
persist as resolved paths. Duplicate normalized roots and source identities are
retained once. Invalid overrides produce a safe notice while default discovery
continues. Missing files are silent; malformed, unreadable, duplicate-member,
excessively nested, and non-UTF-8 documents are unavailable notices.

Recognized JSON documents use JSON5, with duplicate object members rejected and
the exact RFC 6901 pointers persisted. A target must be a non-empty string.
Dynamic names encode `~` as `~0` and `/` as `~1`; empty names and `*` are skipped.
Exact machine file symlinks are followed only when their targets are regular
files. Project traversal does not follow symlinks. Copilot's MCP directory must
be a real directory and only its immediate qualifying regular files are read.

## Rule Inventory

| Rule | Locations | Bounded container | Credential leaves | Notes |
| --- | --- | --- | --- | --- |
| Secret-like source names | Environment and discovered dotenv sources | N/A | Maintained vocabulary in [`SET-006`](../specification.md) | Format and value shape do not affect admission or ordering. |
| Credential-bearing URLs | Environment and discovered dotenv sources | N/A | The complete URL | Absolute hierarchical URLs with authority and non-empty userinfo password, per [`SET-017`](../specification.md). |
| Codex primary credentials | `~/.codex`; `${CODEX_HOME}` | `auth.json` | `/OPENAI_API_KEY`, `/tokens/id_token`, `/tokens/access_token`, `/tokens/refresh_token`, `/personal_access_token`, `/bedrock_api_key/api_key`, `/agent_identity`, `/agent_identity/agent_private_key` | Both agent identity pointers are independent. Historical support: [`openai/codex@ff0e950`](https://github.com/openai/codex/commit/ff0e95007cca1edfc0877bbbbfaeb9eb77ed92b3). |
| Codex MCP credentials | `~/.codex`; `${CODEX_HOME}` | `.credentials.json`, then each immediate root member | `access_token`, `refresh_token` | No server metadata or sibling is required. |
| OpenCode provider credentials | `~/.local/share/opencode`; `${XDG_DATA_HOME}/opencode` | `auth.json`, then each immediate root member | `key`, `token`, `access`, `refresh` | No type, expiry, metadata, account, or enterprise field is inspected. Historical support: [`opencode@31406cc`](https://github.com/anomalyco/opencode/commit/31406ccc51b4bd2a4e1e086b2bcaa5f7f804f26d). |
| OpenCode MCP credentials | `~/.local/share/opencode`; `${XDG_DATA_HOME}/opencode` | `mcp-auth.json`, then each immediate root member | `tokens/accessToken`, `tokens/refreshToken`, `clientInfo/clientSecret`, `codeVerifier` | `oauthState`, `serverUrl`, direct `accessToken`, and `clientInfo/clientId` are not inspected. |
| OpenCode whole environment credential | Inherited environment | N/A | Non-empty `OPENCODE_AUTH_CONTENT` | The whole environment source is persisted; it is not parsed. |
| Copilot token configuration | `~/.copilot`; `${COPILOT_HOME}` | `config.json` then immediate `/copilotTokens` members | Every immediate member value | Values below those members are not inspected. Historical support: [`copilot-cli@ef627e1`](https://github.com/github/copilot-cli/commit/ef627e1baad937d3c8da45f8a5541c6fc3c97b6a). |
| Copilot MCP OAuth credentials | `~/.copilot`; `${COPILOT_HOME}` | Immediate regular files under real `mcp-oauth-config` | `<64 lowercase hex>.tokens.json`: `/access_token`, `/refresh_token`, `/id_token`; `<64 lowercase hex>.json`: `/client_secret` | Each leaf is independent. `client_id` and unrelated token fields in the client family are ignored. |
| Claude primary OAuth credentials | `~/.claude`; `${CLAUDE_CONFIG_DIR}` | `.credentials.json` | `/claudeAiOauth/accessToken`, `/claudeAiOauth/refreshToken` | Plaintext credentials are inspected on every supported platform; keychains remain unqueried. Historical support: [`claude-code@8a8e81d`](https://github.com/anthropics/claude-code/commit/8a8e81d098cbd0fae4ee5b9c853542945fe87016). |
| Claude configured environment | `~/.claude`; `${CLAUDE_CONFIG_DIR}`; project-anchored `.claude/settings.json` | `settings.json`, then immediate `/env` members | `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_AWS_API_KEY`, `ANTHROPIC_FOUNDRY_API_KEY`, `ANTHROPIC_FOUNDRY_AUTH_TOKEN`, `AWS_BEARER_TOKEN_BEDROCK`, `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_CLIENT_KEY_PASSPHRASE` | Other `/env` names are not admitted by this rule. |
| Claude MCP OAuth state | `~/.claude/.credentials.json`, `~/.claude.json`; `${CLAUDE_CONFIG_DIR}/.credentials.json`, `${CLAUDE_CONFIG_DIR}/.claude.json` | Immediate members under `/mcpOAuth` and `/mcpOAuthClientConfig` | `/mcpOAuth`: `accessToken`, `refreshToken`, `clientSecret`; `/mcpOAuthClientConfig`: `clientSecret` | No sibling fields are required. |
| Claude MCP server credentials | `~/.claude.json`; `${CLAUDE_CONFIG_DIR}/.claude.json`; project-anchored `.mcp.json` | Each immediate `/mcpServers` member, then immediate `/headers` and `/env` maps | Headers, case-insensitive: `authorization`, `proxy-authorization`, `x-api-key`, `api-key`, `x-auth-token`, `x-subscription-token`; environment, exact: `API_KEY`, `ACCESS_TOKEN`, `AUTH_TOKEN`, `BEARER_TOKEN`, `CLIENT_SECRET`, `PASSWORD`, `SECRET`, `TOKEN`, plus the eight Claude names above | Other server fields and deeper values are not inspected. |

## Boundaries

These rules do not query OS keychains, execute credential helpers, read raw
sidecars, decode values, or add runtime wildcard traversal. Copilot `.secret`,
`.verifier`, and `mcp-secrets` files remain unsupported. Planned formats such as
npmrc and generic INI are not scanned. Rerun setup after host locations or field
inventories change. See [`LIM-023`](../limitations.md#lim-023-known-source-rules-are-advisory).
