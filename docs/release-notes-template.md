# SecretSieve __VERSION__

SecretSieve keeps local credentials out of coding-agent context using
deterministic exact-value redaction. Runtime resolution and redaction make no
network calls, and no value is ever written into SecretSieve configuration.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/secretsieve/secretsieve/v__VERSION__/install.sh | bash
```

The installer verifies the release checksum before replacing anything, installs
into `~/.local/bin/secretsieve` by default, and never runs setup or changes
coding-agent configuration. Rerunning it upgrades within the installed major
version; crossing a major version needs `--allow-major-upgrade`.

Then, in a project:

```bash
secretsieve setup
secretsieve doctor
```

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

## Platforms

Linux and macOS on x86_64 and arm64. Each asset is listed in
`secretsieve-__VERSION__-SHA256SUMS`.

## Known boundaries

SecretSieve is a safety primitive, not a guarantee that credentials cannot leave
the machine. Read [limitations.md](../limitations.md) before relying on it. The
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

## Reporting a vulnerability

See [SECURITY.md](../SECURITY.md). Never include a real credential in a report.
