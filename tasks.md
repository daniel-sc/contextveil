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

### [ ] T040: Implement Unified Interactive Setup

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

## Production Integration

### [ ] T050: Finish Claude Production Integration

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

### [ ] T060: Implement Status And Doctor

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

## Experimental Integrations

### [ ] T070: Implement Codex Experimental Adapter

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

### [ ] T080: Implement Copilot Experimental Adapter

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

### [ ] T090: Implement OpenCode Experimental Adapter

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

## Hardening And Release

### [ ] T100: Complete Security, Fuzz, And Performance Hardening

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

### [ ] T110: Build Release And Installer Pipeline

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

### [ ] T120: Qualify And Publish V1

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
