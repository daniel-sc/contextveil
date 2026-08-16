# SecretSieve

SecretSieve is designed to help keep enrolled local credentials out of model
context through covered paths of installed, functioning coding-agent
integrations using deterministic exact-value redaction.

> **Status:** Pre-release. This README describes the intended V1 behavior; the
> binary and installer are not available yet.

Local operations still happen; SecretSieve changes only covered text before it
reaches the model:

```text
GITHUB_TOKEN=ghp_example  ->  GITHUB_TOKEN=<SECRET:GITHUB_TOKEN>
```

## Why SecretSieve

- **Keep useful output.** Tools and file reads still run; SecretSieve replaces
  enrolled values instead of broadly blocking access.
- **Get predictable protection.** Matching is literal, case-sensitive, and
  deterministic, without a runtime classifier deciding what looks secret.
- **Protect private formats.** Any enrolled textual credential works, not only
  tokens recognized by provider-specific patterns.
- **Follow rotations.** SecretSieve stores source references instead of copied
  values and resolves the current value for every covered event.
- **Stay local and lightweight.** Runtime uses no network calls, hosted service,
  account, subscription, or LLM classification.

## How It Works

1. Run `secretsieve setup` in a project.
2. Review suggested environment and dotenv sources. Complete values are never
   displayed.
3. Choose the coding-agent integrations to install.
4. Work normally. With valid global setup, clean events are silent;
   interventions show a short summary when the host supports one.

SecretSieve stores source references, not copied values. Sources are resolved for
each covered event and matched as case-sensitive, exact substrings. Dotenv changes
apply on the next event; environment changes require restarting the coding-agent
harness. A value rotated between tool output and hook resolution may be missed.

## Support

V1 targets Linux and macOS on x86_64 and arm64.

| Integration | Tier | Covered model-bound content | Failure behavior |
| --- | --- | --- | --- |
| Claude Code | Production | String values in successful, replaceable `PostToolUse` tool responses | Fail open |
| OpenAI Codex CLI | EXPERIMENTAL | Supported `PostToolUse` results, replaced as sanitized text with possible loss of structure | Fail open |
| GitHub Copilot CLI | EXPERIMENTAL | `userPromptTransformed` and successful `textResultForLlm` text | Fail open |
| OpenCode | EXPERIMENTAL | New V1 `chat.message` user text and successful standard `tool.execute.after` text | Abort when the executing plugin detects a covered malfunction |

Experimental integrations are functional and fixture-tested, but outside the
production support promise and always opt-in. Coverage applies only where a local
harness loads and honors the installed integration. See
[limitations.md](limitations.md) for host-specific gaps.

## Commands

```text
secretsieve setup
secretsieve status
secretsieve doctor
secretsieve --help
secretsieve --version
```

- `setup` is the interactive, rerunnable configuration workflow.
- `status` reports current source and integration state.
- `doctor` performs deeper source, configuration, and offline integration checks.
  An optional paid/networked Claude canary requires confirmation.

Configuration lives at:

- `${XDG_CONFIG_HOME:-~/.config}/secretsieve/config.toml` for global enrollment;
- `.secretsieve.toml` at the selected project root for project enrollment.

Global and project enrollment are additive. Review `.secretsieve.toml` before
using an untrusted project because it may reference environment variables or
dotenv files outside the repository. An invalid or unreadable selected config or
enrolled source disables all redaction for that event rather than using a partial
registry.

## Security Boundary

- Runtime resolution and redaction make no network calls.
- SecretSieve has no telemetry, analytics, crash upload, or persistent runtime
  logging.
- It does not persist resolved values or include them in diagnostics. Values do
  exist transiently in ordinary process memory.
- It protects only current, exact values from sources you enroll. Unknown,
  encoded, split, normalized, hashed, or otherwise transformed values are not
  detected.
- Matching covers selected text values independently, not object keys, binary
  data, images, attachments, or text split across fields.
- It is not a vault, sandbox, network firewall, DLP system, or protection against
  direct credential use by a local process.
- Claude, Codex, and Copilot hooks can be bypassed, disabled, rejected, or timed
  out by their hosts and therefore fail open.
- Other hooks may observe original content first or overwrite sanitized results.
- Placeholders are display markers only; SecretSieve never restores them into a
  later tool call.

Avoid enrolling short or common values: exact substring replacement can remove
unrelated text. Wildcard dotenv enrollment also protects future non-empty keys
without reviewing each value.

## Installation

V1 provides checksummed standalone GitHub Release binaries and an `install.sh`
installer that verifies release checksums. The default install location is
`~/.local/bin/secretsieve`. Installation does not run setup or alter agent
configuration automatically.

## More Detail

- [Specification](specification.md): authoritative V1 behavior
- [Limitations](limitations.md): complete security and host boundaries
- [Vision](vision.md): product intent and non-goals
- [Architecture](architecture.md): implementation boundaries

SecretSieve is free and open source under MIT OR Apache-2.0, with no account or
hosted runtime required.
