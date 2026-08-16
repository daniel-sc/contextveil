# SecretSieve V1 Limitations

This document records accepted product gaps and implementation deviations. It is
not normative and does not authorize violating [specification.md](specification.md).
New deviations must include impact, workaround, and verification. Broad gaps
belong here; code comments should explain only local, non-obvious consequences
and may link to a limitation ID.

## Product Boundary

### LIM-001: Model Context, Not Credential Use

**Reality:** SecretSieve intervenes only at supported model-bound harness paths.
A process can read a credential and send it directly over the network without
that value entering model context.

**Impact:** SecretSieve is not an egress control, sandbox, capability broker, or
DLP boundary.

**Workaround:** Use environment isolation, least-privilege credentials, sandboxing,
and network policy when credential use itself must be controlled.

**Verification:** Threat-model and public wording tests must reject claims that
secrets can never leave the machine.

### LIM-002: Unknown And Transformed Values

**Reality:** Runtime matches only current exact values from enrolled sources.
Unknown credentials and encoded, hashed, split, normalized, partially revealed,
or otherwise transformed values are not recognized.

**Impact:** A model or tool can receive a semantically equivalent representation
that has no exact enrolled byte sequence.

**Workaround:** Enroll all relevant sources and use a separate secret scanner or
stronger execution boundary for unknown/adversarial disclosure.

**Verification:** Negative conformance fixtures demonstrate that transformed and
cross-field values are intentionally unchanged.

### LIM-003: String Values Only

**Reality:** Structured redaction processes decoded string values independently.
JSON object keys, binary data, images, attachment bytes, and values split across
fields or message parts are not covered.

**Impact:** A secret represented in an object key or non-text content may remain
model-visible on a host that forwards it.

**Workaround:** Avoid secret-bearing keys and binary embedding; use host-specific
controls for attachments.

**Verification:** Adapter fixtures preserve keys and non-string values and mark
these paths unsupported.

### LIM-004: Common Values Can Be Destructive

**Reality:** The user may enroll any non-empty UTF-8 value. Runtime has no minimum
length or collision heuristic. Wildcard files automatically enroll future keys.

**Impact:** A value such as `foo` can replace unrelated text extensively and
degrade tool semantics. Future wildcard values receive no enrollment-time review.

**Workaround:** Heed setup and doctor collision warnings; avoid wildcard policies
for files containing non-secret settings.

**Verification:** Setup requires explicit wildcard confirmation and unselects
currently colliding candidates by default.

### LIM-005: No Automatic Rehydration

**Reality:** A placeholder is a display marker, not a credential handle.
SecretSieve never restores a value inside a later tool call.

**Impact:** Tasks that require literal credentials may need the user to arrange
symbolic environment access or perform the operation outside the agent.

**Workaround:** Reference credentials symbolically through the tool environment
where appropriate; do not paste them back into prompts.

**Verification:** No public or internal adapter path maps placeholders to sources.

## Source And Configuration Limits

### LIM-006: Resolution Race

**Reality:** A tool may emit a dotenv value and rotate or delete its source before
the post-tool hook resolves current values.

**Impact:** The old emitted value is not matched.

**Workaround:** Restart/retry after rotation and avoid commands that print and
rotate a credential in one operation.

**Verification:** A regression fixture documents the accepted miss; no previous
value history is persisted.

### LIM-007: Environment Rotation Requires Restart

**Reality:** Hooks inherit the harness process environment. Changing a parent
shell does not modify an already-running harness environment.

**Impact:** Rotated environment values become active only in a newly launched
harness process. Dotenv values remain per-event fresh.

**Workaround:** Restart the coding-agent harness after rotating an enrolled
environment variable.

**Verification:** Status and documentation distinguish environment and dotenv
rotation behavior.

### LIM-008: Project Config Is Trusted To Read Host Paths

**Reality:** Automatically loaded project config may reference arbitrary dotenv
paths and environment names, including paths outside the project.

**Impact:** A cloned project can cause local host-file reads, influence redaction,
or act as a limited presence/equality oracle. Source values are still never
returned in diagnostics.

**Workaround:** Review `.secretsieve.toml` before working in an untrusted project
and prefer global config for machine-specific external paths.

**Verification:** Security tests assert external paths resolve as specified while
diagnostics remain value-free.

### LIM-009: Invalid Project Config Disables Global Protection

**Reality:** Registry use is all-or-nothing. Invalid or unreadable selected
project policy disables otherwise valid global redaction for that event.

**Impact:** Project-controlled config can cause denial of protection. Process-hook
hosts may then pass original content.

**Workaround:** Run `secretsieve doctor`, repair/remove the invalid file, and
review project policy before starting the harness.

**Verification:** Conformance tests assert no partial global fallback occurs.

### LIM-010: Unbounded Input Size

**Reality:** V1 imposes no SecretSieve-specific size cap on dotenv files or
intercepted payloads.

**Impact:** Very large files or payloads can consume excessive memory or exceed
the five-second host timeout, causing fail-open behavior in process-hook hosts.

**Workaround:** Keep credential files small and rely on normal harness output
limits. Diagnose slow paths with `secretsieve doctor` and benchmarks.

**Verification:** Large-input tests measure behavior without promising a fixed
maximum.

### LIM-011: Limited Source Formats

**Reality:** V1 resolves only inherited environment variables and dotenv files.
It has no literal-value storage, JSON resolver, keychain integration, secret
manager, shell-profile evaluation, or provider lookup.

**Impact:** Credentials available only through other stores cannot be enrolled
directly.

**Workaround:** Expose the value through a dedicated environment variable or
dotenv source without copying it into SecretSieve config.

**Verification:** Strict config rejects unknown source types.

## Host Integration Limits

### LIM-012: Process Hooks Fail Open

**Reality:** Claude, Codex, and Copilot continue with original content when the
hook crashes, times out, is disabled, is not trusted, emits malformed output, or
is bypassed by the host. Diagnosed SecretSieve malfunctions also pass original
content with a warning by product choice.

**Impact:** These integrations are safety guardrails, not reliable fail-closed
security boundaries.

**Workaround:** Use `status` and `doctor`, keep the configured executable path
valid, and address every host warning before continuing sensitive work.

**Verification:** Adapter failure fixtures assert warning and original-content
behavior; support material uses fail-open wording.

### LIM-013: Claude Coverage Gaps

**Reality:** Claude V1 rewrites successful `PostToolUse` results only. Failed
tool-result text cannot be replaced through the documented failure event. Tool
execution and host telemetry see the original result before intervention, and
replacement schema rejection can expose the original.

**Impact:** A secret printed by a failing command, unsupported result shape, or
host telemetry path may remain visible outside the covered model result.

**Workaround:** Treat command failures as uncovered, inspect doctor output, and
avoid tools that emit credentials before failing.

**Verification:** Protocol fixtures cover successful replacement and explicitly
negative failed-result cases. Manual release qualification checks resume replay.

### LIM-014: Codex Textual Replacement

**Reality:** Codex does not provide shape-preserving `tool_response` replacement.
On intervention, SecretSieve blocks the original model-facing result and supplies
a sanitized textual rendering for supported `PostToolUse` events.

**Impact:** A successful or typed result may appear error-like and lose structure,
images, or code-mode semantics. Hosted or specialized tools may not emit the
event, and failed-result coverage is not universal.

**Workaround:** Retry with narrower text-producing tools when the sanitized
replacement no longer gives Codex enough structure.

**Verification:** Experimental protocol fixtures assert original suppression and
document semantic degradation.

### LIM-015: Copilot Coverage Gaps

**Reality:** Copilot V1 covers transformed prompt text and successful textual
tool results. Failed errors, non-text attachments, other context injection paths,
and the original prompt displayed in the local timeline are not rewritten.

**Impact:** Enrolled values may remain in uncovered model paths or local UI.

**Workaround:** Avoid pasting credentials into attachments and treat failed tool
output as uncovered.

**Verification:** Fixtures cover both mutable paths and negative failure/non-text
paths.

### LIM-016: OpenCode V1 API Only

**Reality:** The adapter uses documented V1 `chat.message` and
`tool.execute.after` hooks. It does not use the V2 plugin API, experimental full
context transforms, provider wrappers, generic MCP special cases, failed-tool
paths, attachments, existing history, or auxiliary model requests. Throw/abort
behavior applies only after the plugin has loaded and is executing; load failure,
disablement, and host bypass cannot be made fail-closed by the plugin.

**Impact:** OpenCode coverage is broad enough to be useful but incomplete and
version-sensitive. A malfunction detected by an executing plugin aborts the
covered operation; a plugin that never loads cannot intervene.

**Workaround:** Keep the integration explicitly experimental and rerun doctor
after OpenCode upgrades.

**Verification:** Tests target only the two documented hook paths and assert
abort-on-malfunction behavior.

### LIM-017: Hook Composition Is Not A Security Boundary

**Reality:** Hosts may run multiple hooks concurrently or with undocumented
mutation ordering. Other hooks can see original content before SecretSieve and
may replace its result.

**Impact:** Installing SecretSieve cannot prevent another hook from logging,
exfiltrating, or reintroducing a value. User-approved Claude conflicts are still
reported as healthy by product choice.

**Workaround:** Review every competing hook presented by setup and remove
untrusted mutators.

**Verification:** Doctor continues listing approved conflicts; installers never
delete or reorder unrelated hooks.

### LIM-018: No Harness Version Gate

**Reality:** V1 performs no minimum or maximum host version checks despite hook
APIs evolving independently.

**Impact:** A host upgrade can change behavior before SecretSieve's compatibility
fixtures are updated.

**Workaround:** Run doctor after upgrades and use optional Claude live canary when
assurance is needed.

**Verification:** Health relies on configuration and synthetic checks, never a
version-range certificate.

### LIM-019: Project Roots And Multi-Root Sessions

**Reality:** Each event uses one project registry. Claude and OpenCode use stable
roots where available; Codex and Copilot may fall back to event `cwd`. Added or
multi-root workspaces are not merged.

**Impact:** A secondary workspace's project enrollment may be absent, or an
experimental adapter may select a different config after a directory change.

**Workaround:** Put universally required references in global config or launch a
separate session from the secondary project.

**Verification:** Project-selection tests cover nearest-config and cwd fallback.

## Operational Limits

### LIM-020: No Memory-Erasure Guarantee

**Reality:** V1 uses ordinary process memory. It does not guarantee zeroization,
locked pages, core-dump exclusion, swap exclusion, or resistance to same-user
debugging.

**Impact:** Resolved values may transiently exist in process or operating-system
memory outside SecretSieve's model-context claim.

**Workaround:** Apply operating-system hardening where local memory disclosure is
in scope.

**Verification:** Documentation avoids memory-protection claims.

### LIM-021: Interactive Configuration Only

**Reality:** Setup requires a TTY, and status/doctor expose no stable JSON output
contract.

**Impact:** Fully unattended enrollment and structured fleet diagnostics are not
supported in V1.

**Workaround:** Manage the documented TOML and host configuration through
external automation, then run human-readable diagnostics.

**Verification:** Non-TTY setup fails without writes.

### LIM-022: Non-UTF-8 Source Paths

**Reality:** TOML can represent only UTF-8 strings. Automatic discovery skips
dotenv files whose project-relative path contains non-UTF-8 bytes, although it
renders the unavailable path safely in setup.

**Impact:** A dotenv source at such a path cannot be enrolled directly in V1.

**Workaround:** Rename the file or an ancestor directory to a UTF-8 name, or
expose the credential through an enrolled environment variable.

**Verification:** Unix filesystem tests create a non-UTF-8 matching path and
assert it is safely reported, not parsed or persisted.

## Implementation Deviations

No implementation deviations exist yet because implementation has not started.

Add future deviations using this template:

```text
### DEV-NNN: Short title

Contract: requirement IDs
Observed behavior:
Reason:
Impact:
Workaround:
Verification:
Resolution or accepted status:
```
