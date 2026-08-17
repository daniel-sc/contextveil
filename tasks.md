# SecretSieve Implementation Tasks

This is a non-normative, dependency-ordered implementation map. Requirements
live in [specification.md](specification.md); tasks must not silently redefine
them. Update task state and completion evidence as implementation proceeds.

## Working Rules

- Complete a narrow production path before generalizing adapter abstractions.
- Every task includes its tests and documentation changes.
- A task is complete only when its listed mise checks pass.
- Record known behavioral deviations in [limitations.md](limitations.md) in the
  same change. A limitation does not by itself approve a contract violation.
- Use generated canaries, never real credentials.
- Keep at most one implementation task actively in progress per working branch.

Status markers:

```text
[ ] not started
[>] in progress
[x] complete
[!] blocked
```

## Dependency Map

```text
T000 -> T001 -> T010
T010 -> T020 + T030
T020 + T030 -> T040
T020 + T030 + T040 -> T050
T040 + T050 -> T060
T020 + T030 + T040 + T060 -> T070 + T080 + T090
T050 + T060 + T070 + T080 + T090 -> T100
T100 -> T110 -> T120
```

Tasks at the same indentation may proceed in parallel after their dependencies
are complete.

## Foundation

### [x] T000: Establish Documentation Baseline

**Depends on:** none

**Evidence:**

- product intent, domain language, normative behavior, architecture boundaries,
  accepted limitations, repository instructions, and this task graph exist;
- cross-document review resolved security-relevant ambiguity before bootstrap;
- implementation may begin at `T001` without another product decision.

**Closes:** planning prerequisite only; no runtime requirement is implemented.

### [x] T001: Bootstrap Rust And Mise

**Depends on:** `T000`

**Deliver:**

- initialize one lean Rust package with library and binary targets;
- commit a pinned Rust toolchain and lockfile;
- add mise configuration for tool installation and every canonical task;
- implement `--help` and `--version` with the public command skeleton;
- add rustfmt, Clippy with warnings denied, unit-test, build, fuzz-smoke, and
  release-check task entry points;
- add CI that calls mise tasks rather than duplicating commands;
- add MIT and Apache-2.0 license files plus security reporting policy;
- establish generated-canary test helpers and secret-safe assertion helpers.

**Acceptance:**

- `mise install` works on a clean supported development environment;
- `mise run format`, `lint`, `test`, `check`, and `build` pass;
- placeholder `fuzz-smoke` and `release-check` tasks fail clearly only if their
  later implementation is intentionally incomplete;
- no globally installed Rust utility is required outside mise/toolchain setup.

**Contributes to:** `SUP-001`, `CLI-001` through `CLI-003`, `REL-006`, `TST-005`
through `TST-007`.

**Evidence:**

- one package (`Cargo.toml`) with `src/lib.rs` and `src/main.rs`, Rust pinned to
  1.97.1 by `rust-toolchain.toml` and `mise.toml`, and a committed `Cargo.lock`;
- `mise.toml` provides `format`, `format-check`, `lint`, `test`, `check`,
  `build`, `fuzz-smoke`, and `release-check`; `check` composes format, lint, and
  test; `lint` denies warnings and every cargo task uses `--locked`;
- `src/cli.rs` implements `--help`, `--version`, the three public commands, and
  the hidden `hook <harness>` entry points; `CLI-001` is covered by a test that
  rejects `init`/install-style commands, and usage errors never echo argv;
- `src/testing.rs` provides generated canaries plus absence/presence assertions
  whose failure messages withhold the value;
- `tests/cli.rs` exercises the built binary for help, version, hidden entry
  points, usage exit code 2, and loud unimplemented commands;
- `.github/workflows/ci.yml` runs `mise run check` and `mise run build` on Linux
  and macOS runners without duplicating the task command lines; the fuzz and
  release workflows land with `T100` and `T110`, when their tasks stop failing;
- `scripts/fuzz-smoke.sh` and `scripts/release-check.sh` fail with an explicit
  "not implemented yet" message naming the task that implements them;
- `LICENSE-MIT`, `LICENSE-APACHE`, and `SECURITY.md` satisfy `REL-006`;
- `mise install`, `mise run check`, and `mise run build` pass locally; a system C
  linker (`cc`) is the only non-mise prerequisite, as documented in `README.md`.

### [x] T010: Build The Claude Walking Slice

**Depends on:** `T001`

**Deliver:**

- parse one valid V1 global environment reference;
- resolve its current value without logging it;
- perform direct exact replacement in one string;
- parse a representative Claude `PostToolUse` fixture;
- return shape-preserving `updatedToolOutput` and safe intervention metadata;
- expose the hidden Claude hook command over stdin/stdout;
- add end-to-end process tests for clean, matched, unresolved, and invalid input.

This slice should use direct concrete code. Do not build a generic adapter or
resolver framework yet.

**Acceptance:**

- a generated canary in successful Claude tool output is absent from stdout and
  stderr and replaced before the fixture's model-visible boundary;
- no-match output follows Claude's expected protocol without user chatter;
- malformed hook input produces a secret-safe diagnosed response;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `SEC-003` through `SEC-005`, `REG-001`, `RED-001`,
`RED-004`, `RED-008`, `RUN-006`, `CLA-002`, `TST-004`, `TST-005`.

**Evidence:**

- `src/config.rs` resolves the `CFG-001` global path and strictly parses
  `version = 1` plus environment entries; `src/source.rs` resolves them from a
  snapshot of the inherited environment, treating unset, empty, and non-UTF-8
  values as unresolved (`SRC-001`, `SRC-002`);
- `src/matcher.rs` implements leftmost-longest case-sensitive byte matching,
  duplicate canonicalization, the `RED-006` placeholder fallback chain, and
  count-only intervention metadata; `src/redact.rs` walks decoded JSON and
  transforms string values only;
- `src/registry.rs` composes the effective registry all-or-nothing: an invalid
  or unreadable config disables every redaction, a missing global config warns
  and keeps working, and unresolved sources stay silent;
- `src/adapter/claude.rs` parses the native `PostToolUse` envelope from stdin and
  returns `hookSpecificOutput.updatedToolOutput` with `hookEventName`, plus one
  safe `systemMessage` and never `additionalContext`;
- `src/cli.rs` exposes the hidden `hook claude` entry point over stdin/stdout;
- `tests/claude_hook.rs` drives the built binary end to end for matched, clean,
  unresolved, malformed, non-UTF-8, and invalid-config input, asserting canary
  absence from stdout and stderr and exit code zero (`CLI-007`);
- protocol facts were verified against the shipped Claude Code 2.1.233 binary:
  `updatedToolOutput` exists and works for all tools, `hookEventName` is
  required inside `hookSpecificOutput`, built-in tools validate the replacement
  against their own result schema and revert to the original on mismatch
  (`LIM-013`), hook `timeout` is expressed in seconds, `""`/`*`/omitted matchers
  all match every tool, `CLAUDE_PROJECT_DIR` is set for hook processes, and
  every failure path is fail-open (`LIM-012`). `PostToolUse` fires only for
  successful tools, which is consistent with `CLA-004`;
- `mise run check` passes.

**Deferred to later tasks by design:** dotenv sources and project registries
(`T020`), the settings installer and full fixture coverage (`T050`).

## Core Completion

### [x] T020: Implement Config, Registry, And Sources

**Depends on:** `T010`

**Deliver:**

- strict versioned global and project TOML parsing;
- environment, individual dotenv key, and wildcard dotenv source variants;
- path preservation and tilde/relative resolution;
- project-root selection for setup and runtime;
- dotenv parsing without expansion and last-key-wins diagnostics;
- unresolved versus malfunction classification;
- all-or-nothing effective registry construction;
- duplicate source validation and project-first equal-value canonicalization;
- secret-safe config/source diagnostics.

**Acceptance:**

- table-driven fixtures cover runtime/config portions of `CFG-001` through
  `CFG-013`, every `SRC-*`, and every `REG-*` rule;
- malformed or unreadable enrolled sources never create a partial matcher;
- absent/unset/empty sources remain silent and do not fail hook processing;
- config and diagnostics contain no generated canary value;
- `mise run check` passes.

**Contributes to:** `CFG-001` through `CFG-013`, `SRC-001` through `SRC-010`,
`REG-001` through `REG-004`, `TST-002`, relevant `TST-003` cases.

**Evidence:**

- `src/dotenv.rs` implements the `SRC-003` grammar exactly, with tests for BOM,
  CRLF, `export` token handling, key syntax, unquoted comment rules, single and
  double quoting including multiline values and the five decoded escapes,
  trailing content after a quote, and last-key-wins duplicate reporting
  (`SRC-004`); it performs no interpolation or substitution;
- `src/paths.rs` implements `CFG-010` expansion (`~/` only, relative against the
  config file's directory, no environment/glob/shell expansion), `CFG-006`
  lexical identity normalization without symlink resolution, `CFG-003` setup
  root selection, and `CFG-004` runtime project selection;
- `src/config.rs` parses the full V1 schema strictly for both scopes, enforces
  `CFG-006` through `CFG-010` per entry, rejects duplicate identities within one
  file, and reports only a classification plus a position;
- `src/source.rs` resolves environment and dotenv sources, separates unresolved
  (`SRC-002`, `SRC-005`, `SRC-007`) from malfunction (`SRC-006`), and reads each
  dotenv file at most once per event without caching across processes
  (`SRC-009`);
- `src/registry.rs` composes project-then-global entries additively (`CFG-011`),
  returns a malfunction instead of a partial matcher for any invalid config or
  source (`CFG-012`, `LIM-009`), warns on a missing global config (`CFG-013`),
  and canonicalizes equal values to the first project entry (`REG-002`);
- `src/sanitize.rs` implements `SEC-006` one-logical-line rendering, including
  `\xNN` for non-UTF-8 path bytes; its consumers are the terminal commands in
  `T040` and `T060`, so runtime diagnostics deliberately carry no paths at all;
- `src/adapter/claude.rs` selects the project root from `CLAUDE_PROJECT_DIR`
  with the event `cwd` as fallback (`CFG-005`);
- `mise run check` passes.

### [x] T030: Complete Matcher And Structured Redaction

**Depends on:** `T010`; may proceed in parallel with `T020`

**Deliver:**

- leftmost-longest matching across an ordered resolved registry;
- exact case-sensitive UTF-8 and multiline behavior;
- recursive transformation of JSON string values only;
- canonical intervention counts;
- label sanitization and safe named/generic/empty placeholder fallback;
- no recursive placeholder matching or cross-leaf joining;
- property tests comparing the production matcher with a simple reference model;
- benchmarks for the specified representative workload.

**Acceptance:**

- every vector in `TST-001` has a focused regression test;
- generated placeholders and feedback do not reproduce tested active values;
- object keys and non-string values remain byte/semantically unchanged;
- the benchmark records the 100 ms target without making CI timing-dependent;
- `mise run check` passes.

**Contributes to:** `RED-001` through `RED-010`, `RUN-005`, `TST-001`.

**Evidence:**

- `src/matcher.rs` scans an ordered registry with leftmost-longest selection
  over a first-byte index, chooses one replacement per value through the
  `RED-006` fallback chain, never rescans inserted text (`RED-007`), and reports
  counts with emit-safe labels only (`RED-008`);
- every `TST-001` vector has a focused test: empty registry and input, UTF-8
  without normalization, case sensitivity, substrings, adjacent matches,
  same-start and different-start overlap, duplicate values with canonical
  labels, multiline values, both placeholder fallbacks, and no recursive
  replacement;
- `src/redact.rs` transforms decoded string values only, preserving object keys,
  non-string values, and key order, and never joins adjacent leaves (`RED-002`,
  `RED-005`);
- `src/secret.rs` derives labels from the key or name only and reduces them to
  the `REG-004` character set, so escapes and separators cannot reach a
  placeholder;
- `tests/matcher_property.rs` compares the production matcher against an
  independent reference model over 4000 generated cases from a deterministic
  PRNG, plus survivor, count-consistency, and idempotence properties;
- `benches/redaction.rs` (run with `mise run bench`) measures one complete event
  for a 1 MiB payload, 100 resolved values, and 10 dotenv files. Observed on the
  development machine: p50 3.3 ms, p95 4.5 ms against the `RUN-005` 100 ms
  target. The benchmark reports and never fails on timing, so CI stays
  timing-independent;
- `RED-010` needs no code: no path maps a placeholder back to a source value;
- `mise run check` passes.

### [x] T040: Implement Unified Interactive Setup

**Depends on:** `T020`, `T030`

**Deliver:**

- one TTY-only `secretsieve setup` workflow;
- global and project phases with current selections preselected;
- an integration phase extension point initially exercised by the later Claude
  task, without introducing a general plugin framework;
- recursive project dotenv discovery including ignored/untracked files;
- maintained directory exclusions and no automatic file/directory-symlink or
  special-file traversal;
- bounded global probe locations and manual source entry;
- exact V1 name-gating vocabulary and suggestion scoring with explanatory
  signals;
- exact preview masking rules;
- current-project collision analysis and filename-only reporting;
- wildcard confirmation and unresolved-manual-source confirmation;
- safe atomic config writes and resumable multi-phase failure behavior;
- always-created valid project config;
- invalid-existing-config preservation.

**Acceptance:**

- isolated filesystem tests cover the enrollment/config portions of every
  `SET-*` rule and setup-related `CFG-*` rules on Linux and macOS;
- rerunning setup with no changes is idempotent;
- cancellation and non-TTY invocation produce no unintended writes;
- an invalid existing file is byte-for-byte unchanged;
- setup never prints a full canary or deterministic value fingerprint;
- non-UTF-8 paths and control-bearing previews are escaped and never persisted
  lossily;
- `mise run check` passes.

**Contributes to:** `CLI-001`, `CLI-002`, `CLI-004`, `CFG-003`, `CFG-014`,
`CFG-015`, `SET-001` through `SET-014`, `TST-003`.

**Evidence:**

- `src/setup/mod.rs` runs the four `SET-001` phases in order after preflight
  parsing of both files; each configuration phase lists existing entries as
  selected, offers `[s]kip` as the no-change path, and commits only on explicit
  confirmation;
- `src/cli.rs` enforces the `CLI-002` TTY requirement at the process boundary
  and fails with exit code 2 and no writes when invoked non-interactively, which
  keeps the workflow itself drivable by a scripted transcript in tests;
- `src/setup/discovery.rs` implements recursive project discovery and the
  bounded global probe, excluding `.git` and 29 maintained dependency, vendor,
  and build directories, never following file or directory symlinks, and never
  reading FIFOs, devices, or sockets; non-UTF-8 paths are reported safely and
  skipped (`LIM-022`);
- `src/setup/vocabulary.rs` implements the exact `SET-006` gating vocabulary with
  ASCII case folding, token and compact-suffix matching, and advisory-only value
  signals that rank but never introduce a candidate;
- `src/setup/preview.rs` implements the `SET-010` masking table over Unicode
  scalar values, escapes after selection, and derives no fingerprint;
- `src/setup/collision.rs` counts non-overlapping byte occurrences under the
  project root using the discovery exclusions, excludes the candidate's own
  dotenv file, includes binary and ignored files, and reports sanitized relative
  filenames with counts only;
- `src/setup/write.rs` renders configuration through the TOML serializer, writes
  through a temporary file plus rename, creates global files and directories
  user-only (`CFG-001`), and skips writing when content is unchanged;
- wildcard enrollment requires an extra confirmation (`SET-009`), an unresolved
  manual source requires confirmation (`SET-005`), a colliding candidate is
  visible but unselected (`SET-007`) while remaining enrollable (`SET-008`), and
  an enrolled malformed source blocks saving until it is repaired or removed
  (`SET-013`);
- `tests/setup.rs` covers 23 isolated filesystem cases: first run, idempotent
  rerun, cancellation and end-of-input with no writes, byte-for-byte
  preservation of an invalid config, project-root selection, discovery gating,
  collisions and override, wildcard and unresolved confirmations, preserved and
  deliberately removed enrollment, malformed-source blocking, non-UTF-8 paths,
  terminal-escape neutralization, duplicate-key warnings, and a project-phase
  failure that keeps the committed global phase;
- `src/setup/integrations.rs` is the phase-three extension point. It reports
  honestly that no integration installer exists yet; `T050` fills it in with
  concrete Claude code rather than a plugin framework;
- `mise run check` passes.

## Production Integration

### [x] T050: Finish Claude Production Integration

**Depends on:** `T020`, `T030`, `T040`

**Deliver:**

- complete Claude protocol fixture coverage for successful built-in and MCP tool
  response shapes that accept `updatedToolOutput`;
- user-settings installer with absolute exec-form command and 5-second timeout;
- preservation of unrelated settings and ownership-aware update/removal;
- potential `PostToolUse` conflict discovery and per-conflict approval;
- safe named/count `systemMessage` on intervention;
- diagnosed-malfunction warning and documented fail-open behavior;
- offline synthetic integration verification;
- explicit negative fixtures for failed tool results and schema rejection.

**Acceptance:**

- clean installation, repeat installation, upgrade, deselection removal, modified
  entry, malformed settings, disabled hook, and approved conflict are tested;
- every supported successful response retains its exact key/type shape;
- canaries are absent from model-visible replacement, stdout diagnostics, and
  stderr after intervention;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `RUN-001`, `RUN-002`, `RUN-004`, `RUN-006`,
`INT-001` through `INT-006`, `CLA-001` through `CLA-005`, `TST-004`, `TST-005`.

**Evidence:**

- `src/integration/claude.rs` manages exactly one wildcard `PostToolUse` command
  hook in `~/.claude/settings.json` with `timeout = 5` seconds (`CLA-001`,
  `RUN-004`) and an absolute binary path that is shell-quoted so the host cannot
  re-split or expand it (`INT-003`);
- ownership is established by the recorded command plus the exact managed shape,
  so an update replaces its own entry, a hand-modified entry is preserved with a
  warning, and a foreign hook is never touched (`INT-004`);
- other `PostToolUse` command hooks are discovered, sanitized, and offered for
  individual approval; approvals are recorded next to the global config and do
  not make the integration unhealthy (`INT-005`, `CLA-005`, `LIM-017`);
- `src/integration/state.rs` stores ownership and approvals only, user-only, and
  a missing or malformed record degrades safely to "not ours to remove";
- `claude::verify_offline` runs the installed binary against a synthetic
  `PostToolUse` payload through a temporary configuration, requires the generated
  value to be replaced, and enforces the same 5-second bound as the host; the
  setup phase rolls the integration back to its exact prior state when the check
  fails (`SET-014`, `INT-006`, `DIA-007`);
- `tests/claude_hook.rs` covers eight successful result shapes, including two MCP
  shapes, asserting that keys, key order, and non-string values are byte-identical
  and only string leaves change (`CLA-002`); negative cases cover the failed-tool
  event, cross-field values, secret-bearing object keys, malformed and non-UTF-8
  input, unresolved sources, and a 2 MiB payload answered inside the host timeout;
- `tests/setup.rs` adds six integration-phase cases: clean install with offline
  verification, deselection removing only the managed entry, conflict decline and
  approval, undetected-harness disclosure, malformed settings left unchanged, and
  skipping the phase;
- schema rejection is a host behavior that cannot be triggered locally; the
  adapter never synthesizes a shape, which the shape-preservation assertions
  enforce, and the exposure is recorded in `LIM-013`;
- `mise run check` passes.

### [x] T060: Implement Status And Doctor

**Depends on:** `T040`, `T050`

**Deliver:**

- independent registry and integration facets;
- active/unresolved counts and `INACTIVE` handling;
- config/source/permission/duplicate diagnostics;
- current collision recheck with warning-only findings;
- installer ownership, disabled-hook, conflict, executable, timeout, and
  synthetic protocol checks;
- stable public exit-code behavior without a JSON output contract;
- optional confirmed Claude live canary using temporary source configuration;
- secret-safe terminal sanitization for every untrusted path and label.

**Acceptance:**

- exit-code matrix tests cover healthy, partially unresolved, fully inactive,
  malformed, approved-conflict, and inspection-failure cases;
- status performs no adapter protocol tests;
- doctor performs no network call unless the Claude canary is selected;
- diagnostic snapshots contain no canary values or source contents;
- `mise run check` passes.

**Contributes to:** `CLI-005` through `CLI-007`, `DIA-001` through `DIA-005`,
`DIA-007`, `DIA-008`, `SEC-004` through `SEC-006`.

**Evidence:**

- `src/diagnose.rs` reports the registry and integration facets independently,
  shows `INACTIVE` for zero active values, and selects its project root with
  `CFG-003` from the working directory (`DIA-001`, `DIA-002`);
- status runs no adapter protocol test and returns zero whenever inspection
  completes, including for invalid configuration; both commands return two only
  when no configuration location can be determined (`CLI-005`, `CLI-006`);
- doctor adds config permission checks, per-source unresolved and malfunction
  classification, duplicate dotenv keys, duplicate value aliases, current
  project collisions as warnings only (`DIA-004`), installer ownership,
  managed-policy hook disabling, configured-executable existence, the hook
  timeout, per-conflict approval state, and the offline synthetic protocol check;
- exit codes follow `DIA-008`: failure for invalid config, a source malfunction,
  zero active values, no installed integration, a policy-disabled hook, a missing
  executable, an unapproved conflict, or a failed synthetic check; individual
  unresolved sources, collisions, an approved conflict, and a wrong timeout are
  warnings;
- the optional Claude live canary is offered only on a terminal, defaults to off,
  requires confirmation, uses a generated non-credential value through a
  temporary source configuration, and describes the single path it tested
  (`DIA-005`); it has no automated coverage by design, recorded as `DEV-001`;
- `tests/diagnose.rs` covers the exit-code matrix end to end through the binary:
  healthy, partially unresolved, fully inactive, malformed config, approved and
  unapproved conflict, missing integration, inspection failure, status running no
  protocol test, no canary without a terminal, no source content in output, and
  working-directory project selection;
- every untrusted name, path, and hook command is sanitized before output, and
  canary-absence assertions cover both stdout and stderr;
- `mise run check` passes.

## Experimental Integrations

### [x] T070: Implement Codex Experimental Adapter

**Depends on:** `T020`, `T030`, `T040`, `T060`

**Deliver:**

- capture official release protocol fixtures for the tested Codex release;
- native `PostToolUse` parser and sanitized textual replacement path;
- wildcard user hook installation in `~/.codex/hooks.json`;
- documented host trust/review workflow;
- ownership-aware update/removal and conflict approval;
- offline synthetic doctor checks and clear semantic-degradation messaging;
- fail-open diagnosed-malfunction behavior.

**Acceptance:**

- fixtures cover clean output, string and structured intervention, non-zero Bash
  output where supported, unsupported paths, malformed protocol, and failure;
- original canaries are absent from blocked model-facing results;
- setup requires explicit experimental opt-in;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `SUP-003`, `RUN-001`, `RUN-002`, `RUN-004`,
`RUN-006`, `INT-001` through `INT-006`, `COD-001` through `COD-004`, `DIA-006`,
`TST-004`, `TST-005`.

**Evidence:**

- protocol facts were taken from the openai/codex source at commit
  `c6058cca`: hooks live in `~/.codex/hooks.json` as
  `{"hooks": {"PostToolUse": [ matcher groups ]}}`, `timeout` is seconds, an
  omitted/empty/`*` matcher matches every tool, `updatedMCPToolOutput` is
  rejected by the host, a block decision's `reason` is the only text that
  replaces what the model sees, every other failure path is fail-open, a new or
  changed hook stays untrusted until the user accepts it, a failed tool call
  emits no event at all while a non-zero-exit shell command does, and `cwd` is
  the only root field;
- `src/integration/hooks_json.rs` is the shared JSON hooks-file installer,
  extracted only after Claude and Codex became two concrete uses; it preserves
  unrelated keys and hooks, refuses to rewrite an unreadable or unexpected file,
  never creates a second managed entry, prunes only containers it created, and
  proves in a test that two specs never claim each other's entries;
- `src/integration/codex.rs` manages `~/.codex/hooks.json` with a 5-second
  timeout, exposes the trust step setup prints after installing, and verifies
  offline that the hook blocks and that the sanitized reason carries the
  placeholder;
- `src/adapter/codex.rs` blocks the original result and renders sanitized text
  that discloses the tool succeeded and that structure may be lost (`COD-002`,
  `COD-003`); a diagnosed malfunction emits `systemMessage` and no decision, so
  the host keeps the original result (`RUN-001`, `RUN-002`);
- `tests/codex_hook.rs` covers clean output, string and structured intervention,
  non-zero-exit output, unresolved sources, malformed protocol, malfunction,
  project selection from `cwd`, and a 1 MiB payload inside the host bound;
- `tests/setup.rs` proves Codex is never selected by default even when detected,
  that installing it prints the `EXPERIMENTAL` label and the trust step, and that
  deselecting removes it (`SUP-003`, `INT-001`);
- `src/setup/integrations.rs` and `src/diagnose.rs` now iterate every harness, so
  the experimental label, conflicts, executable, timeout, and synthetic checks
  are reported per integration;
- `limitations.md` `LIM-014` records the trust requirement, the uninspected
  `config.toml` hook form, and the failed-versus-non-zero-exit distinction;
- `mise run check` passes.

### [x] T080: Implement Copilot Experimental Adapter

**Depends on:** `T020`, `T030`, `T040`, `T060`

**Deliver:**

- capture official release protocol fixtures for both covered events;
- shape-preserving transformed-prompt and successful-tool-text mutation;
- dedicated managed file under `~/.copilot/hooks/`;
- one safe intervention progress summary;
- ownership-aware update/removal and conflict warning;
- offline synthetic doctor checks and fail-open malfunction behavior.

**Acceptance:**

- fixtures cover prompt, successful result, failed result negative case, clean,
  malformed, timeout, and conflicting mutator scenarios;
- setup requires explicit experimental opt-in;
- canaries are absent from both covered model-facing outputs after intervention;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `SUP-003`, `RUN-001`, `RUN-002`, `RUN-004`,
`RUN-006`, `INT-001` through `INT-006`, `COP-001` through `COP-004`, `DIA-006`,
`TST-004`, `TST-005`.

**Evidence:**

- protocol facts come from the GitHub Copilot CLI hooks reference for CLI 1.0.80:
  every `*.json` file under `~/.copilot/hooks/` is loaded and merged, a handler is
  a flat object with `type`, `bash`, and a seconds-valued `timeoutSec`, events are
  camelCase, `userPromptTransformed` returns `modifiedTransformedPrompt`,
  `postToolUse` returns `modifiedResult` and is honored for command hooks, a
  progress line is a single-line JSON object with `"type": "progress"`, exit 2
  surfaces stderr as a warning while the run continues, and a failed tool result
  arrives on the separate `postToolUseFailure` event;
- `src/integration/copilot.rs` owns exactly one file,
  `~/.copilot/hooks/secretsieve.json`, declaring both covered events with a
  5-second `timeoutSec`. It never writes or deletes another file, refuses to
  overwrite a same-named file whose content is not SecretSieve's, updates an
  outdated file in place, and reports other files in that directory that act on a
  covered event as conflicts;
- `src/adapter/copilot.rs` redacts the transformed prompt and successful
  `toolResult.textResultForLlm`, preserving every other field of the host result
  shape, emits exactly one safe progress summary before the mutation object
  (`COP-003`), stays silent for failed results and clean content, and reports a
  diagnosed malfunction through the host's warning channel without mutating
  anything (`RUN-001`);
- the installed command names the event it serves, because Copilot payloads carry
  no event name; a payload that does not match its entry point is diagnosed rather
  than guessed;
- `tests/copilot_hook.rs` covers prompt and successful-result intervention, the
  failed-result negative case, clean events, unresolved sources, malformed input,
  an unknown event, project selection from `cwd`, the malfunction warning path,
  and a 1 MiB result inside the host timeout;
- `tests/setup.rs` proves Copilot is opt-in, that installation creates only the
  dedicated file, that an unrelated hook file survives installation and removal,
  and that its conflict is offered for approval;
- `limitations.md` `LIM-015` records the uninspected hook sources and the
  undocumented rewrite composition; `DEV-002` records that command-hook support
  for `modifiedTransformedPrompt` is inferred from the host schema rather than
  confirmed by a live run;
- `mise run check` passes.

### [x] T090: Implement OpenCode Experimental Adapter

**Depends on:** `T020`, `T030`, `T040`, `T060`

**Deliver:**

- a minimal TypeScript plugin using only `chat.message` and
  `tool.execute.after`;
- `Bun.spawn` one-shot JSON transport with absolute argv and 5-second timeout;
- managed global plugin-file installation, update, and removal;
- text-part/tool-output mutation using Rust responses;
- named/count TUI notification whose failure does not undo mutation;
- throw/abort behavior for subprocess, protocol, and source malfunction;
- offline synthetic doctor checks.

**Acceptance:**

- plugin code contains no resolver or matcher semantics;
- fixtures cover user text, successful standard tool output, clean events,
  subprocess failure, notification failure, and explicitly unsupported paths;
- setup requires explicit experimental opt-in;
- canaries are absent from covered mutated content and plugin diagnostics;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `SUP-003`, `RUN-003`, `RUN-004`, `RUN-006`,
`INT-001` through `INT-006`, `OCO-001` through `OCO-004`, `DIA-006`, `TST-004`,
`TST-005`.

**Evidence:**

- protocol facts come from the installed OpenCode 1.18.18: plugins load one level
  deep from `plugin/` and `plugins/`, `*.ts` and `*.js` are accepted, any exported
  function of the plugin type is used, `chat.message` receives
  `output.parts[i].text` and `tool.execute.after` receives `output.output` and both
  are mutated in place, `worktree` is the stable project root, the toast API is
  `client.tui.showToast({body: {message, variant}})`, a throwing hook aborts the
  covered operation, `tool.execute.after` runs only on success, and plugins execute
  inside OpenCode's own Bun process;
- `assets/opencode/plugin.ts` is the managed plugin: it registers only the two V1
  hooks, spawns the absolute binary with one JSON request and one JSON response,
  bounds it at five seconds, throws on subprocess failure, nonzero exit, invalid
  protocol, or a reported malfunction, and swallows notification failures after a
  successful mutation. A test asserts the file carries no matcher, resolver, or
  replacement logic and no V2 API use;
- `src/adapter/opencode.rs` is the Rust side of that transport: it redacts each
  string independently, returns them in request order, reports interventions and
  configuration warnings, and answers a malformed request with a protocol error
  that makes the plugin abort;
- `src/integration/opencode.rs` installs exactly one owned plugin file, embeds the
  binary path as a JSON literal so an awkward path cannot break the source, updates
  an outdated file in place, preserves a same-named file it did not write, removes
  only its own file, and lists other plugin files for approval;
- `tests/opencode/plugin.test.ts`, run by `mise run test-plugin` as part of
  `mise run check`, drives the real plugin against the real binary: user text,
  successful tool output, clean events, unsupported paths without spawning,
  subprocess failure, invalid protocol, nonzero exit, reported malfunction,
  notification failure that does not undo the mutation, and the incomplete-setup
  warning;
- `tests/setup.rs` proves OpenCode is opt-in, that installation writes only the
  managed plugin, and that an unrelated plugin survives installation and removal;
- while writing the fixtures, `Bun.spawn` was found to inherit a startup snapshot
  of the environment rather than the live one, so the plugin forwards the current
  environment explicitly to satisfy `SRC-001`; `LIM-016` records that and the
  success-only and approval-by-name limits;
- `mise run check` passes, now including the plugin suite.

## Hardening And Release

### [x] T100: Complete Security, Fuzz, And Performance Hardening

**Depends on:** `T050`, `T060`, `T070`, `T080`, `T090`

**Deliver:**

- matcher and parser fuzz targets with regression seed promotion;
- bounded `mise run fuzz-smoke` across JSON, TOML, dotenv, and matcher surfaces;
- full leak-regression suite across stdout, stderr, snapshots, and diagnostics;
- large-input and timeout behavior tests without adding product size caps;
- dependency and license review;
- terminal-control/path sanitization tests;
- benchmark report for the representative 100 ms workload;
- review every limitation against implemented behavior.

**Acceptance:**

- fuzz smoke runs without panic, unbounded recursion, or canary disclosure;
- maintained Rust builds warning-free and contains no undocumented `unsafe`;
- every open implementation deviation has an approved `DEV-*` entry;
- `mise run check` and `mise run fuzz-smoke` pass.

**Contributes to:** `SEC-003` through `SEC-006`, `RUN-004`, `RUN-005`, `TST-001`
through `TST-007`.

**Evidence:**

- `src/fuzz.rs` provides eight targets covering the matcher, the dotenv grammar,
  configuration TOML, terminal sanitization, and the JSON payload surface of all
  four adapters. Each asserts its own invariants, and every adapter target also
  asserts that the enrolled value never reaches any output channel;
- `src/bin/fuzz_smoke.rs` replays the committed corpus first, then mutates seeds
  with a deterministic generator, so a failure is reproducible and CI never
  depends on luck. `mise run fuzz-smoke` executed 32,016 inputs with no panic, no
  unbounded recursion, and no disclosure. The harness was verified by temporarily
  breaking a target: it reported the failure, wrote the input to
  `fuzz/regressions/`, and exited nonzero;
- `fuzz/regressions/` holds 16 seeded edge cases plus a README explaining seed
  promotion; the harness replays them on every run regardless of budget;
- `tests/leaks.rs` pushes one canary through all four adapters, `status`,
  `doctor`, and a complete `setup` run, then asserts absence from stdout, stderr,
  and every file under the isolated home, including the installed hooks and the
  integration record. It also asserts each path actually redacted something, so
  absence is never vacuous, and covers the malfunction path of every adapter;
- `tests/limits.rs` measures a 4 MiB dotenv file with about 90,000 wildcard keys,
  a 20,000-deep nested payload, a moderately nested payload that is still
  redacted, and 201 active values over a 512 KiB payload; all stay inside the
  five-second host bound and no size cap was added (`SRC-008`, `LIM-010`);
- those tests exposed two quadratic paths that are now fixed: the dotenv parser
  recomputed line numbers from the start of the file for every assignment, and the
  matcher deduplicated values and checked placeholder safety by scanning the whole
  registry per value. A 90,000-key file went from about 190 seconds to 0.27
  seconds, and cost is now linear in input size;
- `mise run bench` records p50 5.4 ms and p95 8.1 ms against the `RUN-005` 100 ms
  target for a 1 MiB payload, 100 values, and 10 dotenv files;
- terminal sanitization is covered by unit tests, by the fuzz target that asserts
  no rendering can contain a line break or escape, and end to end for `status`,
  `doctor`, and `setup`;
- dependency and license review: three direct dependencies (`serde`, `serde_json`,
  `toml`) and 20 crates in total, every one under a permissive license compatible
  with MIT OR Apache-2.0 distribution. No `unsafe` appears anywhere in the crate,
  and `lint` denies warnings;
- `.github/workflows/fuzz.yml` runs `mise run fuzz-smoke` weekly with a larger
  budget and uploads any promoted regression input;
- every limitation was reviewed against implemented behavior; `LIM-010`,
  `LIM-014`, `LIM-015`, and `LIM-016` were updated with what the implementation
  actually does, and the two open deviations are recorded as `DEV-001` and
  `DEV-002`;
- `mise run check` and `mise run fuzz-smoke` pass.

### [x] T110: Build Release And Installer Pipeline

**Depends on:** `T100`

**Deliver:**

- reproducible release builds for Linux/macOS x86_64 and arm64;
- GitHub Release asset naming and SHA-256 checksum generation;
- install/upgrade script with platform detection, checksum verification, atomic
  replacement, `--install-dir`, `--version`, `--allow-major-upgrade`, and
  same-major default upgrade;
- clean install, repeat install, older-V1 upgrade, corrupt-download, and explicit
  major-upgrade tests;
- release notes support matrix and limitation links;
- `mise run release-check` using produced artifacts.

**Acceptance:**

- the installer never runs setup or changes harness/config files;
- a clean target can install and invoke help/version from release artifacts;
- an existing V1 config remains runtime-readable after upgrade;
- checksums and failure handling are verified in CI;
- `mise run release-check` passes.

**Contributes to:** `REL-001` through `REL-007`, `TST-007`.

**Evidence:**

- `scripts/package.sh` builds one release artifact per target with the pinned
  toolchain, a locked dependency set, stripped symbols, and deterministic tar
  metadata, then writes its SHA-256 into
  `secretsieve-<version>-SHA256SUMS`. `mise run package [TARGET]` is the entry
  point, and only the four `SUP-001` targets are accepted. GNU tar and the bsdtar
  that macOS ships take separate invocations because bsdtar rejects `--sort` and
  `--owner`; both produce byte-identical archives across runs on one platform,
  which `release-check` asserts on Linux and macOS;
- `install.sh` implements the `REL-002` interface exactly: platform and
  architecture detection for Linux and macOS on x86_64 and arm64, checksum
  verification before anything is replaced, atomic replacement through a staged
  file beside the target, `--install-dir`, `--version`,
  `--allow-major-upgrade`, and same-major default upgrade. It installs only the
  binary and says so (`REL-003`);
- `scripts/release-check.sh` packages a real artifact and drives the real
  installer against it over `file://`, so it needs no network and no published
  release. Fifteen checks pass: artifact and checksum existence, checksum
  verification, byte-identical repackaging, clean install, running `--version`
  and `--help` from the artifact, no configuration or harness file created,
  no-op repeat install, upgrade from an older release in the same major, an
  existing V1 configuration still runtime-readable afterwards, a corrupt download
  that installs nothing and leaves no temporary file, major-version gating,
  explicit major upgrade, `--install-dir`, and rejection of an unknown option;
- writing those checks caught two flaws in the checks themselves, both fixed: the
  stub versions were hardcoded to major 1 while the crate is pre-1.0, and the
  corrupt-download case never downloaded because the target version was already
  installed;
- `.github/workflows/release.yml` packages all four targets on a version tag,
  runs `mise run release-check`, merges and verifies the checksum files, and
  publishes the GitHub Release with notes generated from
  `docs/release-notes-template.md`, which carries the support matrix and links to
  the governing limitations (`REL-001`, `TST-007`);
- `REL-005` needs no code: no hook or plugin downloads or updates the binary, and
  the installer is the only component that fetches anything;
- `REL-007` is exercised by the upgrade check, which reads a V1 configuration
  written before the upgrade with the newly installed binary;
- `mise run check` and `mise run release-check` pass.

### [!] T120: Qualify And Publish V1

**Depends on:** `T110`

**Deliver:**

- complete all automated gates on the four release targets;
- manually run the paid/networked Claude intervention and resume qualification;
- record tested host versions as release evidence without adding runtime version
  gates;
- reconcile support matrix, status wording, and all limitation entries;
- complete a requirement-to-test traceability audit covering every `SEC-*`,
  `SUP-*`, `CLI-*`, `CFG-*`, `SRC-*`, `SET-*`, `REG-*`, `RED-*`, `RUN-*`,
  `INT-*`, adapter, diagnostic, release, and test requirement;
- verify installation, setup, intervention, status, doctor, removal, and upgrade
  from final artifacts;
- publish checksummed artifacts and security reporting instructions.

**Acceptance:**

- the Claude resume canary remains absent after resume, or resume coverage is
  removed and documented before release;
- Claude is the only integration presented as production;
- all experimental adapters are functional, tested, and opt-in;
- no unresolved unapproved V1 contract deviation remains;
- every normative requirement has passing completion evidence or an explicitly
  approved release-blocking disposition;
- final `mise run release-check` passes against the release candidate.

**Closes:** all V1 requirements, including `SEC-002`, `SUP-002` through `SUP-005`,
`REL-008`, and `TST-008`.

**Blocked on:** three items that cannot be completed from this environment. Every
other part of this task is done and listed below.

1. **Automated gates on the other three release targets.** Only
   `x86_64-unknown-linux-gnu` can be built and exercised here. The release
   workflow builds and packages all four, but that requires CI runners.
2. **Publishing.** Tagging, pushing, and creating the GitHub Release are outside
   what may be done here, and the repository has no remote release yet.
3. **Human sign-off on the live qualification.** The `REL-008` run below was
   performed and passed, but by an automated session rather than by a human at
   the terminal. `REL-008` is a manual gate so that a human judges the result, so
   a release manager must repeat or confirm it before the release ships.
   `docs/qualification.md` records it as a reproducible test report with the
   host's own transcript records attached, not as a signed-off gate.

**Prerelease track:** `1.0.0-alpha.1` is prepared as a GitHub prerelease ahead of
stable 1.0.0, so the four-target packaging and installer paths are exercised
against real published artifacts before the stable release. `install.sh` never
selects a prerelease automatically and installs one only when `--version` names it
(`REL-002`); the release workflow publishes a `-`-suffixed tag as a prerelease,
and verifies artifacts on Linux and macOS rather than Linux alone; and
`mise run release-check` covers both the prerelease and stable selection paths.
The `REL-008` human sign-off above still gates the stable release.

**Evidence for the completed parts:**

- **The live Claude qualification (`REL-008`) was run and passed** on 2026-08-17
  against Claude Code 2.1.233 by an automated session, which is why human
  sign-off is still listed as a blocker above, in an isolated home so no real
  harness configuration or enrolled source took part. An enrolled generated value was
  replaced by its placeholder in the model's reply, the same session was resumed
  with `claude -r`, and the reply was still the placeholder; the value appeared
  nowhere under that home, including the stored transcript. The host persists the
  replaced result, which is why resume has no original to recover.
  `docs/qualification.md` is the record, including the scope of what one run
  proves. The run also found that the canary treated "value absent" as a pass,
  so a reply that never ran the command would have counted as proof; the pass
  condition now requires the placeholder, matching every offline synthetic check
  (`DIA-005`), and the classification has unit tests;
- **Traceability audit.** `docs/traceability.md` has one row for each of the 124
  requirement IDs in `specification.md`, naming its implementation and the test or
  check that would fail on regression. No row is a gap: 116 are covered by a test,
  four are prohibitions satisfied by the absence of a mechanism, and three are
  manual. The document was produced by an independent pass over the code and then
  corrected where it had gone stale or cited the wrong deviation;
- **Verification from final artifacts.** A packaged release artifact was installed
  with `install.sh`, then the complete journey was run against it: `setup` over a
  real pty, an intervention through the installed Claude hook that replaced a
  planted value, `status`, `doctor` exiting zero, and integration removal that
  left the host settings file valid. `mise run release-check` covers installation
  and upgrade from artifacts on this target;
- **Two defects found that way and fixed:** `status` reported its own installed
  hook as pointing at another binary because it passed no executable path, and the
  OpenCode inspection reported a five-second timeout even when no plugin was
  installed. Both have regression tests;
- **Public wording.** `tests/wording.rs` enforces `SEC-002`, `SUP-002`, `SUP-003`,
  `SUP-005`, and `TST-008` against the shipped documents: no overclaiming phrase,
  the required boundary statements present, every experimental integration
  labeled in every support matrix, no routine workflow that would make CI paid or
  networked, and every limitation entry carrying its required sections. It found
  that the README was missing the `SUP-005` scoping statement, now added;
- **Tested host versions** are recorded in `docs/release-notes-template.md` as
  evidence rather than as a supported range, since `SUP-004` forbids version
  gates: Claude Code 2.1.233, `openai/codex` at commit `c6058cca`, Copilot CLI
  1.0.80, and OpenCode 1.18.18;
- **Claude is the only production integration** in every surface: setup, status,
  doctor, the README matrix, the vision matrix, and the release notes. The other
  three are labeled `EXPERIMENTAL`, are never selected by default, and each has
  protocol fixtures and an offline synthetic check;
- **Deviations.** `DEV-001` (the live canary has no automated coverage) and
  `DEV-002` (Copilot prompt coverage rests on an inferred host rule) are the only
  open implementation deviations, both recorded with impact, workaround, and
  verification;
- **Independent reviews.** Two read-only reviews were run against the shipped
  code. The first covered the core and found that an empty value could reach the
  matcher in a release build and that `U+061C` escaped `SEC-006` sanitization;
  both were fixed under `T020`. The second covered the adapters, installers,
  plugin, and release tooling, and found that the Copilot and OpenCode installers
  treated a hand-edited managed file as merely stale, so an edit would have been
  reverted and the file deleted on removal; both now preserve it, matching the
  shared JSON hooks installer. A self-review of `install.sh` in the same pass
  found that a hostile release archive could ship its binary member as a symlink
  or carry extra paths, so extraction is now limited to that one member and
  refuses anything that is not a regular file;
- `mise run check`, `mise run fuzz-smoke`, and `mise run release-check` pass on
  this target.

## Deferred Until A Changed Requirement

- additional source formats, literal storage, keychains, or secret managers;
- daemons, caches, value history, or cross-event session state;
- public adapter/resolver plugin APIs;
- provider proxies or final-dispatch interception wrappers;
- environment stripping, command policy, or placeholder rehydration;
- noninteractive setup or stable machine-readable status output;
- Windows support and package-manager distribution;
- OpenCode V2 or experimental full-context transforms;
- production promotion of experimental adapters.
