# ContextVeil __VERSION__

ContextVeil keeps enrolled local secrets out of coding-agent model context
through deterministic local redaction. Runtime resolution and redaction make no
network calls, and no value is ever written into ContextVeil configuration.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/daniel-sc/contextveil/v__VERSION__/install.sh | bash
```

The installer verifies the release checksum before replacing anything, installs
into `~/.local/bin/contextveil` by default, and never runs setup or changes
coding-agent configuration. Rerunning it upgrades within the installed major
version; crossing a major version needs `--allow-major-upgrade`.

Then, in a project:

```bash
contextveil setup
contextveil doctor
```

Setup guides enrollment from environment variables, dotenv files, manual exact
JSON fields, and maintained Known Source Rules. These rules are advisory and
version-sensitive, not adapter coverage guarantees, and every applicable rule
runs independently of selected integrations.

## Support matrix

| Integration | Tier | Covered model-bound content | Failure behavior |
| --- | --- | --- | --- |
| Claude Code | Production | String values in successful, replaceable `PostToolUse` tool responses | Fail open |
| OpenAI Codex CLI | EXPERIMENTAL | Supported `PostToolUse` results, replaced as sanitized text with possible loss of structure | Fail open |
| GitHub Copilot CLI | EXPERIMENTAL | `userPromptTransformed` and successful `textResultForLlm` text | Fail open |
| OpenCode | EXPERIMENTAL | New V1 `chat.message` user text and successful standard `tool.execute.after` text | Abort when the executing plugin detects a covered malfunction |

Experimental integrations are functional and fixture-tested, but outside the
production support promise, and always require an affirmative choice during
setup. Coverage applies only where a local harness loads and honors the installed
integration.

## Known Source Rules

Supported setup-time rules admit candidates from secret-like names,
credential-bearing URLs, and recognized coding-agent credential store schemas.
Manual additions and filesystem enumeration are not rules. JSON source documents
use the full JSON5 grammar, so common comment-bearing Copilot configuration is
supported; duplicate members remain invalid.

Valid unknown schemas silently no-match; malformed matched JSON sources are
shown as unavailable. Override values resolve during setup, relative overrides
use the invocation directory, changes require a rerun, and no shell or tilde
expansion occurs. Raw sidecars, OS keychains, and credential helpers are not
covered. Planned npmrc and recognized INI store rows are non-contract and are not
currently scanned. See the [exact rule inventory and pinned evidence](known-sources.md) and
[`LIM-023`](../limitations.md#lim-023-known-source-rules-are-advisory).

## Tested host versions

Protocol behavior was verified against these host versions. V1 performs no host
version checks (`LIM-018`), so these are evidence rather than a supported range:
run `contextveil doctor` after upgrading a coding agent.

| Host | Verified against |
| --- | --- |
| Claude Code | Adapter: 2.1.233 live qualification. Known Sources: 2.1.238, public release commit `8a8e81d098cbd0fae4ee5b9c853542945fe87016` plus shipped-artifact-derived private structures |
| OpenAI Codex CLI | Adapter: `openai/codex` commit `c6058cca`. Known Sources: `ff0e95007cca1edfc0877bbbbfaeb9eb77ed92b3` (also issue-time `d9fd91edab298c2423c0c82526513e4e000284cf`) |
| GitHub Copilot CLI | Adapter and Known Sources: 1.0.80 release commit `ef627e1baad937d3c8da45f8a5541c6fc3c97b6a`, official docs commit `838d18789ba2c51cfe5544b3e5bf1ca3168c2795`, plus shipped-artifact-derived private structures |
| OpenCode | Adapter and Known Sources: 1.18.18 commit `31406ccc51b4bd2a4e1e086b2bcaa5f7f804f26d` |

## Platforms

Linux and macOS on x86_64 and arm64. Each asset is listed in
`contextveil-__VERSION__-SHA256SUMS`.

## Known boundaries

ContextVeil is a model-context safety primitive, not a guarantee that credentials
cannot leave the machine. Read [limitations.md](../limitations.md) before relying on it. The
most important entries:

- [`LIM-001`](../limitations.md#lim-001-model-context-not-credential-use): model
  context only, not credential use or egress.
- [`LIM-002`](../limitations.md#lim-002-unknown-and-transformed-values): unknown
  and transformed values are not recognized.
- [`LIM-003`](../limitations.md#lim-003-string-values-only): string values only,
  not object keys or binary content.
- [`LIM-004`](../limitations.md#lim-004-common-values-can-be-destructive):
  enrolling a short or common value can replace unrelated text.
- [`LIM-012`](../limitations.md#lim-012-process-hooks-fail-open): process hooks
  fail open when a host crashes, times out, disables, or bypasses them.
- [`LIM-013`](../limitations.md#lim-013-claude-coverage-gaps) through
  [`LIM-016`](../limitations.md#lim-016-opencode-v1-api-only): per-host coverage
  gaps.
- [`LIM-023`](../limitations.md#lim-023-known-source-rules-are-advisory): Known
  Source Rules are advisory; raw sidecars, keychains, helpers, and unknown or
  changed schemas remain outside coverage.

## Reporting a vulnerability

See [SECURITY.md](../SECURITY.md). Never include a real credential in a report.
