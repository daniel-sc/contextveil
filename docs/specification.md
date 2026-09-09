# ContextVeil V1 Specification

This document is the normative contract for observable ContextVeil V1 behavior.
The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are
to be interpreted as requirements. [architecture.md](architecture.md) defines
mandatory technical boundaries; [vision.md](vision.md) is non-normative product
intent.

Requirement IDs are stable references for tests, tasks, and limitations. Text
without an ID remains normative when it uses a requirement keyword.

## 1. Security Claim

**SEC-001** ContextVeil MUST help prevent currently resolved values from enrolled
local sources from entering model context through the explicitly covered paths
of installed, functioning adapters.

**SEC-002** ContextVeil MUST NOT claim to prevent direct local-process use or
network exfiltration, unknown secrets, transformed values, host bypass, or
content paths outside the current support matrix.

**SEC-003** Runtime resolution and redaction MUST make no network calls.
Installation and an explicitly selected Claude live canary are the only V1
network-capable ContextVeil workflows.

**SEC-004** ContextVeil MUST NOT persist resolved source values, include them in
configuration, or expose them in diagnostics, logs, telemetry, test artifacts,
or intervention metadata.

**SEC-005** ContextVeil MUST have no telemetry, crash upload, analytics, or
persistent runtime logging.

**SEC-006** Every untrusted string rendered to a terminal, including labels,
paths, key names, and masked value previews, MUST occupy one logical line. C0/C1
controls, DEL, escape, bidi controls, and Unicode line or paragraph separators
MUST be rendered as visible escapes. Non-UTF-8 path bytes MUST be rendered as
`\xNN`; they MUST NOT be emitted raw. Preview selection occurs before escaping,
so an escape representation does not reveal additional source characters.

## 2. Supported Platforms And Integrations

**SUP-001** V1 MUST support Linux and macOS on x86_64 and arm64.

**SUP-002** Claude Code is the production integration. Codex CLI, GitHub Copilot
CLI, and OpenCode are functional experimental integrations.

**SUP-003** Experimental integrations MUST be labeled `EXPERIMENTAL` in setup,
status, doctor, and the public support matrix. They MUST require affirmative
installation and MUST NOT be counted as production release health.

**SUP-004** V1 MUST NOT perform host version checks. Health derives from
observed configuration and synthetic protocol checks, subject to the host-level
limitations in [limitations.md](limitations.md).

**SUP-005** Coverage applies to local harness modes that honor the configured
machine/user integration. Cloud, remote, container, or managed-policy modes are
covered only when ContextVeil is separately installed and the configured hook is
honored there.

## 3. CLI

The public command surface is:

```text
contextveil setup
contextveil status
contextveil doctor
contextveil --help
contextveil --version
```

Harness protocol entry points MAY appear in process listings or configuration,
but MUST be hidden from ordinary help and treated as internal interfaces.

**CLI-001** `setup` MUST be the only configuration workflow. V1 MUST NOT expose
`init`, public integration install/remove subcommands, or harness slash commands.

**CLI-002** `setup` MUST require an interactive TTY. It MUST fail clearly and
without changing files when invoked non-interactively.

**CLI-003** Public commands MUST produce human-readable output. V1 provides no
stable JSON output contract for public commands.

**CLI-004** `setup` MUST return zero only when every requested write,
installation/removal action, and offline verification completes. Cancellation,
write failure, or requested integration failure MUST return nonzero.

**CLI-005** `status` MUST return zero whenever inspection completes, regardless
of inactive, warning, degraded, or experimental findings. It MUST return nonzero
when inspection itself cannot complete.

**CLI-006** `doctor` MUST return zero when all configured health checks pass,
one for diagnosed health failures, and two for usage or inspection failures.
A registry with zero currently resolved values is a health failure. Individual
unresolved sources, collision warnings, and user-approved hook conflicts are not
health failures by themselves.

**CLI-007** Diagnosed process-hook runtime failures SHOULD exit zero after
emitting valid host protocol output so the host can present the warning. An
unhandled crash remains governed by host behavior.

## 4. Configuration Locations And Selection

**CFG-001** The global config path MUST be:

```text
${XDG_CONFIG_HOME:-~/.config}/contextveil/config.toml
```

This location applies on both Linux and macOS. Newly created directories and
global files MUST be user-only on platforms supporting Unix permissions.

**CFG-002** The project config filename MUST be `.contextveil.toml`.

**CFG-003** Setup MUST select its project root as follows:

1. the nearest ancestor containing `.contextveil.toml`;
2. otherwise the enclosing Git worktree root;
3. otherwise the current directory.

Setup MUST create `.contextveil.toml` at that root even when the project registry
is empty.

**CFG-004** Runtime MUST select at most one project registry. Starting from the
adapter-provided project root, it MUST use the nearest ancestor project config.
If none exists, project enrollment is empty. Parent and multi-root project
registries MUST NOT be merged in V1.

**CFG-005** Claude MUST use its stable initial project directory when available,
falling back to event `cwd`. OpenCode MUST use its stable project/worktree field.
Codex and Copilot MAY use event `cwd` when no stable initial root exists.

## 5. Configuration Schema

Both global and project policy files use this V1 semantic schema:

```toml
version = 1

[[secret]]
source = "env"
name = "GITHUB_TOKEN"

[[secret]]
source = "dotenv"
file = ".env.local"
key = "STRIPE_API_KEY"

[[secret]]
source = "dotenv"
file = "~/shared/project.env"
all = true

[[secret]]
source = "json"
file = "~/.codex/auth.json"
pointer = "/tokens/access_token"

[[secret]]
source = "properties"
file = "src/main/resources/application-local.properties"
key = "spring.datasource.password"

[[secret]]
source = "npmrc"
file = "~/.npmrc"
key = "//registry.npmjs.org/:_authToken"
```

Equivalent field naming changes before the first persisted implementation MAY be
made tactically. Once shipped, the implemented V1 schema is the contract and
must follow the compatibility rule in `REL-007`.

**CFG-006** Every file MUST contain `version = 1`. Unknown fields, unknown source
types, malformed entries, and duplicate source identities within one file MUST
make that file invalid.

Source identity is defined after tilde expansion, relative-path resolution, and
lexical removal of `.` and `..`, without filesystem canonicalization or symlink
resolution:

```text
environment:       (env, exact name)
dotenv key:        (dotenv, normalized absolute path, exact key)
dotenv wildcard:   (dotenv-all, normalized absolute path)
JSON field:        (json, normalized absolute path, exact JSON Pointer)
properties key:    (properties, normalized absolute path, exact decoded key)
npmrc entry:       (npmrc, normalized absolute path, exact key)
```

Repeated identical tuples within one file are duplicates. A keyed entry and a
wildcard entry for the same file are distinct and MAY coexist; equal resolved
values are later deduplicated by `REG-002`.

**CFG-007** An environment entry MUST contain `source = "env"` and one non-empty
`name`. It MUST NOT contain dotenv-only fields.

**CFG-008** A dotenv entry MUST contain `source = "dotenv"`, one non-empty
`file`, and exactly one of:

- one non-empty `key`; or
- `all = true`.

**CFG-009** Global and project registries MAY contain the same source identity.
Project configuration MAY reference environment variables and arbitrary files,
including files outside the project.

**CFG-010** Source paths MUST be stored as entered. A leading `~/` MUST expand to
the current user's home. Relative paths MUST resolve relative to the config file
containing the reference. Environment-variable, glob, and shell expansion MUST
NOT occur.

**CFG-011** Effective enrollment is additive: all valid global source references
plus all valid selected-project source references. V1 MUST NOT provide negation,
disable, or override semantics.

**CFG-012** Parsing is strict per file and use of the effective registry is
all-or-nothing. Invalid or unreadable global or selected project config,
including permission denial and non-`NotFound` config I/O errors, MUST disable
all redaction for the event and produce a visible warning where the adapter
permits. No valid entries from either file may be used partially.

**CFG-013** A missing global config contributes an empty global registry and
means machine setup is incomplete; a valid selected project registry remains
usable. A missing project config is normal. A missing referenced source is
unresolved. None of these conditions is malformed config. A missing global
config is a non-clean configuration state: runtime SHOULD warn that global setup
is incomplete without discarding valid project redaction.

**CFG-014** Setup MUST NOT overwrite an invalid existing global or project
config. It MUST show a sanitized path and location/reason and leave all config
and integration files unchanged.

**CFG-015** Setup MUST preserve existing valid enrollment by default and permit
deliberate removal. It MUST NOT remove an entry merely because the source is
currently unresolved.

**CFG-016** A JSON entry MUST contain `source = "json"`, one non-empty `file`,
and one exact RFC 6901 JSON Pointer in its plain string form. The pointer MUST
begin with `/`, MUST have a non-empty final reference token, and MUST NOT use the
URI-fragment `#/...` form or wildcard extensions. It MUST NOT contain fields for
another source type.

**CFG-017** A properties entry MUST contain `source = "properties"`, one
non-empty `file`, and one non-empty exact decoded `key`. It MUST NOT contain
fields for another source type. Resolver type MUST NOT be inferred from a
filename, and properties wildcard enrollment is not supported.

**CFG-018** An npmrc entry MUST contain `source = "npmrc"`, one non-empty
`file`, and one non-empty case-sensitive exact `key`. It MUST NOT contain fields
for another source type. Resolver type MUST NOT be inferred from a filename, and
npmrc wildcard enrollment is not supported.

## 6. Source Resolution

**SRC-016** Every source resolver MUST apply Rust `str::trim()` to a decoded
value before deciding whether it is resolved. A value empty after trimming is
unresolved. Candidate grouping, collision analysis, registry construction, and
runtime matching MUST use that trimmed value. Keys, paths, labels, and
model-visible payload strings MUST NOT be trimmed by this rule.

**SRC-001** An environment reference resolves from the hook process's inherited
environment using a case-sensitive name.

**SRC-002** An unset, empty, or non-UTF-8 environment value is unresolved and
MUST NOT enter the matcher. Doctor SHOULD identify the source without showing a
value.

**SRC-003** A dotenv resolver MUST use this deterministic grammar:

- input is UTF-8 with an optional leading BOM and LF or CRLF line endings;
- grammar whitespace means ASCII space or tab; a CRLF line ending is normalized
  to LF, including the newline retained inside a multiline quoted value;
- blank lines and lines whose first non-whitespace character is `#` are ignored;
- an assignment may begin with the exact token `export` followed by one or more
  grammar-whitespace characters and then a valid key; `export=value` instead
  defines the key `export`, while `export =value` is malformed;
- keys match `[A-Za-z_][A-Za-z0-9_.-]*` and are followed by optional whitespace,
  `=`, and optional whitespace;
- an unquoted value continues to the physical line end, has surrounding
  grammar whitespace trimmed, treats backslashes literally, and starts a comment
  only at a `#` preceded by grammar whitespace;
- a single-quoted value is literal until the matching quote and may span physical
  lines;
- a double-quoted value may span physical lines and decodes only `\\`, `\"`,
  `\n`, `\r`, and `\t`; any other backslash pair retains the backslash;
- after a closing quote, only grammar whitespace and an optional `#` comment are
  valid;
- every other nonblank line or unterminated quote is malformed.

The resolver MUST NOT perform variable interpolation, command substitution, or
code execution.

**SRC-004** When a dotenv key occurs more than once, the last assignment wins.
Setup and doctor SHOULD warn about the duplicate without showing either value.

**SRC-005** An absent dotenv file, absent key, or empty resolved value is
unresolved and MUST NOT be treated as a malfunction.

**SRC-006** Permission denial, malformed dotenv syntax, invalid UTF-8 dotenv
content, or non-`NotFound` I/O failure is a malfunction and MUST disable use of
the entire effective registry for that event.

**SRC-007** A wildcard dotenv entry resolves every current non-empty key. Keys
added later become enrolled without another setup run. Empty keys remain
unresolved.

**SRC-008** V1 MUST impose no ContextVeil-specific dotenv file-size cap.

**SRC-009** Sources MUST be resolved afresh for every intercepted event. Values
MUST NOT be cached across hook processes or retained as rotation history.

**SRC-010** Dotenv changes are observable on subsequent events. Environment
changes are observable only after the harness is restarted with the new
environment.

**SRC-011** A JSON resolver MUST read UTF-8 using the full JSON5 grammar and
reject duplicate object member names at any nesting depth. Full JSON5 includes
its object-key, string, number, whitespace, comment, escape, and trailing-comma
forms; it is not a comments-only JSON extension. The resolver MUST resolve
exactly one value selected by the configured RFC 6901 JSON Pointer and MUST NOT
perform wildcard traversal, key-name search, interpolation, decoding, or other
transformation. The public name for this persisted `source = "json"` variant is
JSON source.

A JSON source with more than 128 nested object or array containers MUST be
treated as malformed before recursive deserialization.

**SRC-012** A selected non-empty JSON string is a resolved secret. A missing
pointer, empty string, `null`, number, boolean, object, or array is unresolved.

**SRC-013** An absent JSON source file is unresolved. Permission denial,
malformed or non-UTF-8 JSON5, duplicate object members, or a non-`NotFound` I/O
failure is a malfunction and MUST disable use of the entire effective registry
for that event.

**SRC-014** A JSON file referenced by multiple entries MUST be read and parsed
once per event where practical. Its current contents MUST be resolved afresh for
every intercepted event and MUST NOT be cached across hook processes.

**SRC-015** JSON5 grammar applies only to enrolled or discovered JSON source
documents persisted with `source = "json"`. Harness protocols, hook payloads,
integration configuration files, and other protocol JSON MUST remain strict JSON
unless their own requirements separately specify another grammar.

**SRC-017** A properties resolver MUST parse Java-style logical key/value entries
with the `java-properties` 2.0.0 default Windows-1252 behavior, including its
separators, continuation lines, escapes, and `\\uXXXX` handling. Decoded keys are
case-sensitive and the last occurrence wins; setup and doctor SHOULD warn about
duplicates without values. The resolver MUST select one exact decoded key and
MUST NOT interpolate, merge profiles, execute commands, or extract components
from values. An absent file, absent key, or value empty after `SRC-016` is
unresolved. Permission denial, non-`NotFound` I/O failure, or parser error is a
malfunction. A file referenced by multiple entries MUST be read and parsed once
per event where practical, and a parser error MUST discard all entries.

**SRC-018** An npmrc resolver MUST use this ContextVeil-owned scalar grammar:

- input is UTF-8 with an optional leading BOM and LF or CRLF line endings;
- blank lines and lines whose first non-whitespace character is `#` or `;` are
  ignored;
- top-level assignments split at the first `=`, trim their keys, and require a
  non-empty key;
- unquoted values end at the first unescaped `#` or `;`, are trimmed, and decode
  only `\\#`, `\\;`, and `\\\\` to their literal characters; other backslashes
  remain literal;
- single-quoted values are literal until the matching quote;
- double-quoted values decode only `\\\\`, `\\\"`, `\\n`, `\\r`, and `\\t`;
- quoted comment characters are literal, and after a closing quote only
  whitespace and an optional `#` or `;` comment are permitted;
- multiline values, sections, array fields, and key-only fields are unsupported.

The parser MUST retain valid top-level scalar entries and issues attributable to
recoverable exact keys while ignoring unrelated or unkeyed unparsable lines. A
selected key with any malformed or unsupported occurrence is a malfunction,
including a manually selected form that automatic discovery otherwise ignores.
Duplicate valid scalar keys use the last valid assignment and produce a
value-free setup and doctor warning; any malformed or unsupported occurrence of
that selected key still makes resolution malfunction.

The resolver MUST NOT interpolate environment expressions or otherwise decode,
canonicalize, or transform values. Text such as `${NAME}` is an ordinary literal
value. An absent file, absent key, or value empty after `SRC-016` is unresolved.
Invalid UTF-8, permission or non-`NotFound` I/O failure, or a selected keyed
syntax issue is a malfunction. One npmrc file referenced by multiple entries
MUST be read and parsed once per event where practical, read afresh on the next
event, and have no ContextVeil-specific size cap.

## 7. Setup Discovery And Enrollment

**SET-001** After TTY validation and successful preflight parsing of both
existing config files, every setup run MUST present these phases in order:

1. global enrollment;
2. project enrollment;
3. integration selection and removal;
4. offline verification.

Each config phase MUST present existing entries as selected and provide a
no-change path.

**SET-002** Setup MUST automatically inspect the current process environment for
sources eligible under every applicable Known Source Rule, then apply `SET-023`.
Rule application MUST NOT depend on which adapters are selected, installed, or
detected.

**SET-003** Project dotenv discovery MUST recursively include regular files named
`.env` or beginning `.env.`, including ignored and untracked files, when their
project-relative path is valid UTF-8 and can therefore be represented losslessly
in TOML. Matching non-UTF-8 paths MUST be shown as unavailable using `SEC-006`
and skipped. Discovery MUST exclude `.git` and maintained known dependency,
vendor, and build directories. It MUST NOT follow file or directory symlinks or
read FIFOs, devices, sockets, or other special files. A skipped UTF-8 path may
still be entered manually.

The same single bounded project traversal MUST supply eligible lowercase
`*.properties` files to `SET-021` and every regular file named exactly `.npmrc`
to `SET-022`; the project tree MUST NOT be walked again.

**SET-004** Global dotenv probing MUST inspect matching files directly under the
home directory and directly under the supported harness config directories,
including `~/.claude`, `~/.codex`, `~/.copilot`, and
`~/.config/{opencode,contextveil}`. It MUST NOT recursively crawl the home or
general config directory.

**SET-005** Both enrollment phases MUST allow manual dotenv paths, individual
keys, wildcard file enrollment, environment names, JSON file/pointer pairs,
properties file/decoded-key pairs, and npmrc file/exact-key pairs.
A currently absent manual file, key, or pointer MAY be saved after explicit
unresolved-source confirmation. Explicit manual addition MUST bypass `SET-023`.

**SET-006** The secret-like name Known Source Rule MUST identify automatic
candidate eligibility using a maintained, case-insensitive vocabulary, including
concepts such as token, secret, password, key, and credential. Final admission is
subject to `SET-023`. Format, entropy, length, and source type MUST NOT
independently introduce a name-eligible source or assign candidate rank,
admission weight, selection preference, or confidence.

V1 name gating uses ASCII case folding. It splits the name into tokens at every
run of non-ASCII-alphanumeric characters and also creates a compact form by
removing those separators. A name is gated when either:

- a token exactly equals `token`, `secret`, `password`, `passwd`, `passphrase`,
  `key`, `credential`, or `credentials`; or
- the compact form ends with `token`, `secret`, `password`, `passwd`,
  `passphrase`, `credential`, `credentials`, `apikey`, `accesskey`, `privatekey`,
  `clientsecret`, `authtoken`, or `refreshtoken`.

Characters outside ASCII are preserved for display but do not match the V1
vocabulary. Vocabulary changes are observable setup behavior and MUST update
this requirement and its fixtures.

**SET-007** Selection defaults MUST apply to each Candidate Group or standalone
source. Every wholly new automatically admitted enrollment unit MUST initially
be selected. Collision analysis finding another occurrence is the only reason
setup MUST automatically unselect an otherwise valid wholly new automatic unit.
Candidates with collisions MUST remain visible, and collision-driven automatic
non-selection MUST be explicit in the presentation. A unit containing existing
enrollment or an explicitly added manual Candidate MUST remain selected despite
collisions.

**SET-008** The user is authoritative. Setup MUST allow enrollment after a
collision warning, MUST allow explicit manual enrollment of a Common Literal,
and MUST NOT impose a minimum runtime value length.

**SET-009** Setup MUST offer wildcard enrollment for every current and future
key in a selected dotenv file. Before saving it, setup MUST require an additional
confirmation explaining that short, common, and future values bypass individual
review.

**SET-010** Complete candidate plaintext MUST NOT be shown. Length and first/last
units in this rule are Unicode scalar values, not UTF-8 bytes or grapheme
clusters. Preview masking MUST be:

| Character length | Preview |
| --- | --- |
| 0-4 | fully masked |
| 5-15 | first 2 and last 2 characters |
| 16-39 | first 4 and last 4 characters |
| 40+ | first 4 and last 4 characters, plus `****...skipped N chars...****` for the concealed middle |

For a 40+ character value, `N` is the number of concealed middle characters not
represented by the eight displayed mask characters. Previews MUST remain bounded
and `N` is counted in Unicode scalar values.

The total character length SHOULD be shown. Deterministic value fingerprints
MUST NOT be shown. Apart from this masked preview and length and the applicable
Known Source Rule names, setup MUST NOT derive or display value-shape advisory
details based on length, character classes, entropy, encoding-like form, or
format.

**SET-011** Collision analysis MUST search readable regular files no larger than
16 MiB (16,777,216 bytes, inclusive) under the current selected project root
using the discovery exclusions. It MUST include ignored files, exclude every
whole source file known to contribute an equal-value alias, not follow file or
directory symlinks, and skip FIFOs, devices, sockets, and other special files.
Files above the limit MUST be skipped in full. Analysis MUST search maximal valid
UTF-8 regions bounded by NUL or malformed UTF-8 bytes, including such regions in
binary files, without decoding or decompressing file contents. For each Candidate
Group it MUST count non-overlapping exact byte occurrences from left to right;
matches MUST NOT cross region boundaries.

Alias-file discovery for these exclusions MUST consider resolvable sources and
candidates from both enrollment phases even though Candidate Groups
remain phase-local. Exclusions MUST derive from all aliases known during
discovery, not only the references currently selected in the setup UI.

**SET-012** Collision output MUST show occurrence counts and affected sanitized
relative filenames. It MUST NOT show values, matched lines, snippets, or files
skipped by collision analysis because collision analysis is advisory.

**SET-013** An unreadable or malformed automatically discovered file that is not
already enrolled MUST be shown as unavailable and excluded from candidates; it
MUST NOT abort discovery. An existing enrolled malformed or unreadable source
MUST be repaired or removed before setup can complete.

**SET-014** Config files MUST be written atomically enough that runtime readers
cannot observe partial TOML. Each phase commits only after its own explicit user
confirmation. Cancellation or failure retains prior committed phases and leaves
the current and later phases unchanged. If global config is saved and project
config then fails, the complete global change remains, project config remains
unchanged, integration changes are skipped, and setup returns nonzero.

Each integration action is a separate resumable transaction. A failed install,
removal, or offline verification MUST restore that integration's exact prior
managed state where possible; already completed integration actions remain.
Setup MUST report any rollback failure, preserve unrelated host config, skip
remaining actions, and return nonzero.

**SET-015** Every visible Candidate Group or standalone source MUST have a
numeric toggle. Enrollment phases MUST offer select-all, select-none, manual
environment, dotenv key, wildcard, and JSON additions, save, skip, and quit.
Integration phases MUST offer apply, skip, and quit. Row-specific toggling and
bulk selection MUST be omitted when there are no rows. Every interaction that
continues a selection loop, including invalid, declined, duplicate, and
otherwise no-op interactions, MUST rerender the complete current screen and
available actions.

**SET-016** Within one enrollment phase, setup MUST represent candidate source
references with equal current resolved values as one Candidate Group. The group
MUST have one selection marker and numeric toggle, and toggling it MUST select or
deselect every represented source together. If any represented source was
enrolled at the start of the phase, the group MUST initially be selected. A
newly discovered equal-value alias that passes automatic admission MUST join that
selected group and MUST be persisted on `Save`. An explicitly added resolvable
alias MUST likewise select its group. If previously equal enrolled aliases later
resolve to different values, they MUST appear as separate selected enrollment
units. Collision defaults MUST NOT deselect a group containing an enrolled or
explicitly added source. `Skip` remains the exact no-change path and MUST NOT
persist newly discovered aliases or any selection changes.

Groups MUST NOT combine global and project references. Manual resolvable sources
MUST join an equal-value group immediately. Unresolved sources and dotenv
wildcard policies remain standalone. A selected wildcard suppresses redundant
keyed candidates from its own file, and its current values contribute alias-file
exclusions for equal-value groups elsewhere without placing the wildcard itself
in a group.

Every Candidate Group has one Group Representative: the represented Source
Reference with the least Source Identity. Group members MUST be ordered by Source
Identity after automatic discovery and grouping.

Source Identity order MUST compare source kind first in this V1 sequence:

1. environment;
2. dotenv key;
3. dotenv wildcard;
4. JSON.
5. properties.
6. npmrc.

A source kind added later in V1 MUST append after every source kind already in
the contractual sequence when that change lands. Within one source kind, the
identity fields in `CFG-006` MUST compare in tuple order by exact,
case-sensitive bytes, with no locale or filesystem case folding. Path fields use
their normalized absolute identity paths. A derived label or final JSON Pointer
token is not an identity ordering field.

After automatic discovery and grouping, each enrollment phase MUST show rows in
two tiers. A row is existing when it represented enrollment at the start of that
phase; toggling it MUST NOT change its tier during the phase. Existing rows come
first and wholly new rows second. Rows within each tier MUST be ordered by Group
Representative. An unresolved Source Reference or dotenv wildcard policy is a
standalone row and acts as its own representative under the same ordering.
Discovery iteration order MUST NOT affect grouping, representatives, or this
initial row order.

A manual addition during an active selection loop MUST NOT reorder the rows or
group members already displayed in that loop. A resolvable manual alias still
joins its equal-value group immediately, but its displayed insertion position
may remain until the phase ends.

On `Save`, setup MUST persist every represented reference from each selected
Candidate Group and every selected standalone reference in Source Identity
order, including previously enrolled references and references added manually.
This normalization occurs even when enrollment membership did not change and
may therefore change the canonical alias selected later under `REG-002`. `Skip`
remains the exact no-write, no-change path.

Each Candidate Group SHOULD show one masked value preview and a sanitized
description of every represented source. It MUST NOT show or derive a complete
value or deterministic value fingerprint. It MUST also show the display name of
each Known Source Rule that admitted at least one represented source, deduplicated
once per group and ordered by the fixed inventory order in
[`docs/known-sources.md`](docs/known-sources.md). A Known Source Rule's identity
and the number of matching rules MUST NOT affect admission after the first match,
selection, Group Representative choice, row order, or persistence order.

**SET-017** Under the credential-bearing URL Known Source Rule, every value
already surfaced by bounded automatic discovery that parses as an absolute
hierarchical URL with an authority and a non-empty password in userinfo MUST be
eligible for automatic admission even when its source name does not pass
`SET-006`; final admission is subject to `SET-023`. The complete URL value, not
an extracted or decoded component, is enrolled. This
value-shape rule MUST NOT introduce recursive inspection of JSON or any other
structured source.

**SET-018** A Known Source Rule is a maintained, deterministic setup-time rule
that identifies automatic candidate eligibility. The V1 rule inventory consists
of the secret-like name rule in `SET-006`, the credential-bearing URL rule in `SET-017`,
the properties configuration rule in `SET-021`, the npmrc credentials rule in
`SET-022`, and the recognized credential document rules in
[`docs/known-sources.md`](docs/known-sources.md). Filesystem enumeration and
manual source additions are inputs to setup, not Known Source Rules; explicit
manual addition admits a Candidate by user action. Every applicable rule MUST run
independently of adapter selection, installation, detection, and runtime
coverage. Rule applicability is binary eligibility and display attribution;
`SET-023` applies once after rule composition. Applicability MUST NOT carry a
score, admission weight, selection preference, ordering preference, or
confidence level.

A recognized credential document rule MUST produce ordinary exact source
references and MUST NOT be persisted as runtime indirection. All default and
valid override machine roots are additive. Path override environment variables
are resolved during setup; a later override change requires setup to be rerun.
Relative override values MUST resolve from setup's invocation directory and
receive lexical `.`/`..` normalization only. Override values MUST NOT receive
shell, environment-variable, glob, or tilde expansion. Default paths persist as
`~/...`; override paths persist as resolved explicit paths. Duplicate normalized
roots and duplicate resulting source identities MUST be inspected or retained
only once. See
[`ADR-0001`](docs/adr/0001-persist-explicit-source-references.md).

Recognized credential document rules MUST inspect only explicit machine roots,
setup-time override roots, explicit filenames, bounded directories, and the
anchored project patterns in `SET-003`. They MUST NOT recursively crawl a home
or project directory for generic filenames or field names. One shared project
traversal MUST recognize narrowly anchored project patterns at any depth using
the exclusions in `SET-003`.
Project traversal MUST NOT follow symlinks. An exact machine file path that is a
symlink MUST be followed only when its target is a regular file. An invalid
override MUST produce a safe unavailable notice without suppressing default-root
discovery.

**SET-019** A recognized credential document uses JSON5 and the duplicate-member
and nesting behavior in `SRC-011`. Any bounded probe that reaches a listed field
and selects a non-empty string MUST produce an automatically eligible exact
source reference, subject to `SET-023`; unrelated surrounding or sibling schema
MUST NOT be validated. Missing, empty, null, numeric, boolean, array, or object
targets silently no-match, and one unusable field MUST NOT suppress another
usable field in the same document. Malformed, non-UTF-8,
duplicate-member, or unreadable recognized documents follow `SET-013`. Dynamic
names MUST be representable as exact JSON Pointers under `CFG-016`; empty names
and `*` silently no-match. Setup MUST NOT recursively classify arbitrary JSON
strings by generic key substrings.

**SET-020** The supported recognized credential document rules MUST cover only
the exact locations, bounded containers, and credential leaves listed in
[`docs/known-sources.md`](docs/known-sources.md). Those locations are additive:
Codex uses `~/.codex` and `CODEX_HOME`; OpenCode uses
`~/.local/share/opencode` and `${XDG_DATA_HOME}/opencode`; Copilot uses
`~/.copilot` and `COPILOT_HOME`; Claude uses `~/.claude`, `~/.claude.json`, and
the corresponding `CLAUDE_CONFIG_DIR` paths. The inventory defines the exact
primary, provider, MCP, token, header, and environment leaves and the bounded
Copilot filename patterns. Claude plaintext `.credentials.json` primary fields
are inspected only on non-macOS platforms; macOS primary credentials are
keychain-backed and remain unqueried. `OPENCODE_AUTH_CONTENT` remains one whole
environment source. ContextVeil MUST NOT query OS keychains or execute
credential helpers. Raw `.secret`, `.verifier`, and `mcp-secrets` fallback files,
unlisted locations, and unlisted fields remain outside discovery.

**SET-021** The properties configuration Known Source Rule MUST inspect every
eligible regular lowercase `*.properties` file supplied by the single project
walk and the additive machine paths `~/.gradle/gradle.properties` and
`${GRADLE_USER_HOME}/gradle.properties`. Relative `GRADLE_USER_HOME` values
resolve from setup's invocation directory. The rule admits an exact properties
source only when its trimmed value is non-empty and its decoded key passes
`SET-006` or its value passes `SET-017`; final admission is subject to `SET-023`.

Project eligibility MUST exclude any file below an ASCII-case-insensitive
`i18n`, `l10n`, `locale`, `locales`, `lang`, or `languages` segment. It MUST also
exclude conventional bundle basenames `message`, `messages`, `label`, `labels`,
`string`, `strings`, `text`, `texts`, `errors`, and `ValidationMessages`, and
two-letter Java ResourceBundle-style locale suffixes with optional four-letter
script and two-letter or three-digit region. Recognized names `application`,
`application-*`, `bootstrap`, `bootstrap-*`, `microprofile-config`, `gradle`, and
`sonar-project` override basename and locale exclusions at any project depth;
localization-directory exclusions remain authoritative. These conventions affect
eligibility only, not admission, selection, attribution, or ordering. Manual
properties enrollment bypasses localization exclusions.

**SET-022** The npmrc credentials Known Source Rule MUST inspect the additive
machine files `~/.npmrc`, `${NPM_CONFIG_USERCONFIG}`, and
`${NPM_CONFIG_GLOBALCONFIG}`, plus every project `.npmrc` supplied by the shared
walk in `SET-003`. Override names are exact and uppercase. Their path values use
the override semantics in `SET-018`; valid defaults and overrides remain
additive and normalized duplicate paths are inspected once.

The rule MUST identify an exact npmrc source as automatically eligible for every
non-empty scalar entry whose case-sensitive key starts with `//`, has a non-empty
prefix before its final colon-delimited field, and ends in exactly `:_authToken`,
`:_auth`, or `:_password`; final admission is subject to `SET-023`. The registry
or scope prefix is not parsed or canonicalized and
credential values are not decoded. Every valid scalar entry is also offered
independently to the generic rules: `SET-006` receives only its final
colon-delimited key field, while `SET-017` receives its complete scalar value.
Applicable rule names are deduplicated and retain no admission, selection,
grouping, or ordering weight.

**SET-023** After `SRC-016` normalization and before grouping or presentation,
setup MUST exclude a wholly new Source Reference eligible only through automatic
Known Source Rules when its complete trimmed value equals, using ASCII
case-insensitive comparison, one of `true`, `false`, `yes`, `no`, `on`, `off`,
`0`, `1`, `enabled`, `disabled`, `null`, `nil`, `none`, `undefined`, `n/a`,
`default`, or `auto`. Comparison MUST be exact over the complete value and MUST
NOT use substring, token, locale-sensitive, or Unicode case-insensitive matching.
The exclusion MUST apply once after all applicable rules compose, regardless of
which or how many rules matched.

The Common Literal exclusion MUST NOT remove or deselect an existing Enrolled
Source, apply to an explicitly added manual Candidate, prevent dotenv wildcard
enrollment or expansion, or affect registry construction or runtime matching. A
newly discovered automatic alias MUST NOT bypass the exclusion because an
equal-value existing or manual Candidate is present. Unresolved and empty
name-eligible sources retain their existing behavior. Exclusion is silent and
carries no rule attribution, score, selection preference, or ordering weight.
Vocabulary changes are observable setup behavior and MUST update this requirement
and its fixtures.

## 8. Effective Registry

**REG-001** Every non-empty value normalized by `SRC-016` from an Enrolled Source,
including a Common Literal and every wildcard-resolved value, becomes an active
match pattern. Runtime MUST NOT apply `SET-023` or name, entropy, provider-format,
length, or collision heuristics.

**REG-002** If multiple references resolve to the same value, the matcher MUST
store one pattern. Its canonical source is the first project entry in file order,
otherwise the first global entry in file order. Doctor SHOULD report the aliases
without values.

**REG-003** Source and key names are case-sensitive. Safe placeholder labels MUST
derive from the environment name, dotenv or properties key, final JSON Pointer
reference token, or final colon-delimited npmrc key field only, never a file
path. An npmrc key without a colon uses the complete key.

**REG-004** A label MUST preserve ASCII letters, digits, `_`, `-`, and `.` and
replace every other non-empty run with `_`. Labels need not be globally unique.

## 9. Redaction Semantics

**RED-001** Matching MUST compare case-sensitive UTF-8 byte sequences with no
Unicode normalization, case folding, decoding, or transformation.

**RED-002** Matching MUST operate independently within each selected string
value. It MUST NOT join adjacent JSON fields, message parts, chunks, or binary
attachments.

**RED-003** Matching MUST use leftmost-longest semantics. At the earliest match
start, the longest active value wins. Equal resolved values use the canonical
source from `REG-002`. Scanning resumes after the chosen match.

**RED-004** Matching is substring matching. A value is replaced wherever its
exact bytes occur inside a selected string; token or word boundaries have no
special meaning.

**RED-005** Structured payload processing MUST parse the host structure and
redact decoded string values only. Object keys, numbers, booleans, nulls, binary
content, and attachment bytes MUST remain unchanged.

**RED-006** The preferred placeholder is `<SECRET:LABEL>`. Before insertion, it
MUST be checked against every active value. If unsafe, `<SECRET>` MUST be checked.
If that is also unsafe, the match MUST be replaced with an empty string.

**RED-007** Generated placeholders MUST NOT be recursively fed back through the
normal matcher. Unsafe labels MUST be omitted from feedback, and generated
model-visible feedback MUST be suppressed or reduced when it would reproduce an
active value.

**RED-008** Redaction MUST produce intervention metadata containing total and
per-source replacement counts. A canonical label is included only when emitting
it cannot reproduce any active value; unsafe labels are aggregated under an
unnamed count. Metadata MUST NOT contain matched values, deterministic hashes,
source content, matching lines, or value-derived previews.

**RED-009** Clean runtime events with valid global configuration MUST be silent.
An intervention SHOULD produce one safe named/count summary through host UI when
supported. Unresolved sources MUST remain silent during normal runtime. The
incomplete-global warning in `CFG-013` is a configuration warning, not an
intervention.

**RED-010** ContextVeil MUST NOT replace placeholders with source values in later
tool calls or offer another automatic rehydration path.

## 10. Runtime Failure Policy

**RUN-001** A source malfunction or invalid effective config MUST produce no
partial redaction. Process-hook adapters MUST pass the original host content and
warn where the host protocol permits.

**RUN-002** Claude, Codex, and Copilot MUST be documented as fail-open when their
process crashes, times out, produces invalid protocol output, is disabled, or is
bypassed by the host.

**RUN-003** The OpenCode plugin MUST abort the affected covered operation when
the subprocess crashes, times out, returns invalid protocol, or reports a
malfunction. Notification failure after successful mutation MUST NOT undo the
sanitized result.

**RUN-004** Every installed runtime hook or OpenCode subprocess invocation MUST
use a 5-second timeout.

**RUN-005** Runtime MUST target p95 below 100 ms on a typical local SSD for a
warm-cache 1 MiB textual payload, 100 resolved values, and 10 dotenv files. This
is an engineering benchmark, not a machine-independent pass/fail guarantee.

**RUN-006** A malformed host envelope or an unknown protocol event/version is a
diagnosed protocol malfunction. A process adapter MUST emit a valid secret-safe
host warning when the event protocol still permits one, MUST NOT echo the
malformed payload, and otherwise leave the host to its documented fail-open
behavior. An executing OpenCode plugin MUST throw and abort the covered
operation. A valid event containing intentionally uncovered content or fields is
not malformed; the adapter MUST preserve that content unchanged and need not
warn.

## 11. Integration Installation

**INT-001** Setup MUST detect all four harnesses. Claude MUST be selected by
default when detected. Experimental integrations MUST remain unselected unless
already installed by ContextVeil.

**INT-002** A user MAY explicitly install an integration whose executable was
not detected. Setup MUST disclose that verification is limited.

**INT-003** Every installed command MUST use the absolute current ContextVeil
binary path and direct argument arrays where supported. Hook payloads MUST use
stdin and responses MUST use structured stdout. Shell interpolation MUST NOT be
used.

**INT-004** Setup MUST avoid duplicate managed entries. Removal by deselection
MUST remove only an artifact whose ownership and unchanged identity can be
established. Modified or user-owned entries MUST be preserved with a warning.

**INT-005** Potentially competing mutating hooks MUST be shown for individual
approval where they can be statically identified. An approved conflict is not a
health failure, but doctor MUST continue showing it.

**INT-006** Installation success MUST NOT be represented as permanent proof of
protection. Status and doctor derive current state from config and host artifacts.

## 12. Claude Code Adapter

**CLA-001** Setup MUST manage one synchronous wildcard `PostToolUse` command hook
in `~/.claude/settings.json` with a 5-second timeout.

**CLA-002** The adapter MUST recursively redact every string value in successful
`tool_response` while preserving object keys, non-string values, and the host's
exact structural shape. It MUST return the result through
`hookSpecificOutput.updatedToolOutput`.

**CLA-003** On intervention, the adapter SHOULD return one safe `systemMessage`
with total count and safe canonical labels. It MUST NOT add `additionalContext`.

**CLA-004** The adapter MUST NOT claim coverage for failed tool results,
submitted prompts, outgoing tool arguments, host telemetry, original local
artifacts, or successful-result paths that do not accept replacement.

**CLA-005** Other matching `PostToolUse` hooks MUST trigger setup approval under
`INT-005`. Once approved, they do not prevent Claude adapter health from being
reported as healthy.

## 13. Codex CLI Adapter

**COD-001** Setup MUST manage one synchronous wildcard `PostToolUse` command hook
in `~/.codex/hooks.json` with a 5-second timeout and the host's required trust
workflow.

**COD-002** On a match in a supported `PostToolUse` payload, the adapter MUST
redact string values, prevent the original result from normal model consumption,
and provide a sanitized textual rendering through the host's blocking feedback
mechanism.

**COD-003** The adapter MUST disclose that intervention may turn a successful or
structured result into error-like text and lose structure, images, or typed
semantics.

**COD-004** The adapter MUST NOT claim that every tool emits the event, that MCP
results are shape-preserving, or that all failed result categories are covered.

## 14. GitHub Copilot CLI Adapter

**COP-001** Setup MUST manage a dedicated ContextVeil user hook file under
`~/.copilot/hooks/` with a 5-second timeout. It MUST NOT modify unrelated hook
files.

**COP-002** The adapter MUST redact `userPromptTransformed` model-facing text and
successful `postToolUse.toolResult.textResultForLlm` text while preserving each
documented host result shape.

**COP-003** On intervention, the adapter SHOULD emit one safe persistent progress
summary before its final mutation object.

**COP-004** The adapter MUST NOT claim coverage for failed tool errors, non-text
attachments, other context injection paths, or the original prompt displayed in
the local timeline.

## 15. OpenCode Adapter

**OCO-001** Setup MUST manage one ContextVeil-owned TypeScript plugin file under
`~/.config/opencode/plugins/`. The plugin MUST invoke the absolute Rust binary
with one JSON request on stdin and one JSON response on stdout.

**OCO-002** The plugin MUST use the documented V1 `chat.message` hook to redact
new textual user-message parts and `tool.execute.after` to redact successful
standard textual tool output.

**OCO-003** The plugin MUST show one safe named/count TUI notification when
redaction occurs and the host notification API is available.

**OCO-004** V1 MUST NOT implement OpenCode V2 plugin APIs, provider/language-model
wrappers, experimental full-history/system transforms, tool-definition
rewriting, or claims for failed tools, generic MCP output gaps, attachments,
existing history, or auxiliary model calls.

## 16. Status And Doctor

**DIA-001** Status MUST inspect the selected global/project config, resolve
current sources, and report active and unresolved counts without running adapter
protocol tests. Status and doctor select their project root using `CFG-003` from
their current working directory.

**DIA-002** Registry and integration health MUST be shown as independent facets.
Some unresolved sources do not degrade an otherwise functioning adapter. Zero
active values MUST be shown as `INACTIVE`.

**DIA-003** Doctor MUST additionally inspect config permissions, source errors,
duplicate aliases, current project collisions, integration ownership, disabled
hooks, approved/unapproved conflicts, executable availability, timeout settings,
and synthetic protocol behavior.

**DIA-004** Collision findings MUST be warnings only and MUST NOT change runtime
enrollment or doctor exit status. Doctor MUST canonicalize equal resolved values
in effective-registry order and exclude every enrolled alias source file using
the same grouped semantics as `SET-011`.

**DIA-005** Doctor MUST offer an optional paid/networked Claude live canary only.
It MUST be disabled by default, require confirmation, use a conspicuous random
non-credential value through a temporary source configuration, and describe the
single path it tested. It MUST treat the reply as passing only when the expected
placeholder is present and the generated value is absent. A reply containing the
generated value MUST be a health failure whether or not the placeholder is also
present. A reply containing neither MUST be reported as having proven nothing:
visible and never a pass, but not a health failure, because it diagnoses no
condition that prevents protection (`DIA-008`).

**DIA-006** Codex, Copilot, and OpenCode MUST have offline synthetic verification
only in V1. Passing verification MUST NOT remove their experimental label.

**DIA-007** A previous successful verification MUST NOT be represented as a
permanent certificate.

**DIA-008** Doctor MUST return one for any diagnosed condition that prevents
effective protection: invalid/unreadable config, an enrolled source malfunction,
zero active values, no installed integration, a disabled or untrusted installed
hook, a missing configured executable, an unapproved mutator conflict, a failed
synthetic check, or a failed selected live canary. It MUST return two only for
invalid CLI usage or an unexpected internal/OS failure that prevents doctor from
producing a complete classified report. Absent unselected integrations are
informational.

## 17. Installation And Release

**REL-001** V1 MUST publish standalone GitHub Release artifacts for the four
platform/architecture targets in `SUP-001` with SHA-256 checksums.

**REL-002** The project MUST provide a maintained installation script that
detects platform and architecture, downloads the selected release asset,
verifies its checksum, and atomically installs it. The default destination is
`~/.local/bin/contextveil` and MUST be overridable.

The script interface MUST be:

```text
install.sh [--install-dir DIR] [--version VERSION] [--allow-major-upgrade]
```

With no installed binary it selects the latest stable release. With an installed
binary it selects the latest release in that major. `--version` selects an exact
release, but crossing the installed major still requires
`--allow-major-upgrade`. With an installed binary, standalone
`--allow-major-upgrade` selects the latest stable release across majors. With no
installed binary, the flag has no additional effect because latest stable is
already the default.

A prerelease version such as `1.0.0-alpha.1` MUST NOT be selected automatically
by any of those rules. `--version` MUST be able to name one exactly, and the
major-version gate applies to it unchanged.

**REL-003** The install script MUST install or upgrade only the binary. It MUST
NOT launch setup, edit config, install adapters, or accept enrollment defaults.

**REL-004** Rerunning the install script MUST upgrade to the latest compatible
release in the installed major version. Crossing an incompatible major version
MUST require explicit opt-in.

**REL-005** Hooks and plugins MUST NOT download, install, or update the Rust
binary.

**REL-006** V1 MUST be licensed under MIT OR Apache-2.0 and include a public
security-reporting policy.

**REL-007** Every release in major version 1 MUST read earlier V1 config and
recognize earlier V1 managed integration state without requiring setup to run
before runtime protection resumes.

**REL-008** Release qualification MUST include a manual live Claude test proving
that a successfully redacted tool result remains sanitized after session resume.
If that cannot be established, resume coverage MUST be removed from claims and
recorded as a limitation before release.

## 18. Testing And Acceptance

**TST-001** Matcher tests MUST cover empty values, UTF-8, case sensitivity,
substrings, adjacent matches, same-start overlap, different-start overlap,
duplicate values, canonical labels, multiline values, placeholder fallback, and
no recursive replacement.

**TST-002** Config and source tests MUST cover strict unknown fields, duplicate
identities, cross-scope duplicates, missing sources, empty values, non-UTF-8
environment values, malformed/invalid-UTF-8 dotenv and JSON sources, the full
JSON5 grammar, npmrc scalar grammar and keyed issues, duplicate dotenv and npmrc
keys and JSON members, JSON Pointer escaping and wrong-type targets, path
expansion, wildcard future keys, and all-or-nothing malfunction behavior.

**TST-003** Filesystem tests MUST cover project-root selection, recursive ignored
file discovery, additive Known Source default and override locations, bounded
permissive field probes without sibling-schema gating, exact and anchored paths,
exclusions, symlink traversal, grouped collision source-file exclusion,
permissions, atomic writes, invalid-config preservation, repeat setup, and
partial multi-phase failure. They MUST retain malformed-file, JSON Pointer, and
secret-leak coverage, including npmrc project traversal and exact override path
semantics. Common Literal coverage MUST include the complete vocabulary,
normalization and exact-match boundaries, every automatic source family, manual
and existing enrollment, wildcard expansion, and exclusion before alias grouping.
Collision coverage MUST include textual regions in binary files, region and read
buffer boundaries, independent overlapping candidates, and the inclusive file
size limit.

**TST-004** Every shipped adapter path MUST have protocol decision fixtures for
clean, intervened, unresolved, and explicitly unsupported behavior. Every
covered path MUST retain one non-vacuous real boundary fixture. Malformed input,
registry malfunction, project-root selection, host failure mapping, installation
shape, timeout, conflicts, and diagnostics MUST be tested once at their owning
layer rather than repeated through every layer.

**TST-005** Every intervention boundary fixture MUST generate a canary, prove it
entered the covered input, prove intervention occurred, and assert the canary
and its unique token are absent from stdout, stderr, returned model-visible
content, and progress or notification records where applicable. Dedicated leak
tests MAY focus on setup persistence, diagnostics, logging, and telemetry rather
than repeat adapter conformance.

**TST-006** Fuzz targets MUST cover robustness of the matcher, sanitizer,
untrusted JSON5 source, strict adapter protocol JSON, TOML, dotenv, and npmrc
inputs.
Committed corpora MUST replay routinely with mutation disabled. Bounded mutation
MUST run separately through mise.

**TST-007** Routine CI MUST run formatting, linting with warnings denied, tests,
and builds through mise on supported targets. Release checks MUST consume the
exact package-job artifacts and exercise their checksums, clean installation,
and upgrade behavior without rebuilding substitutes.

**TST-008** Optional paid/networked tests MUST NOT gate routine CI; optional
automation is permitted. Publication MUST remain gated by the human Claude
resume qualification in `REL-008`.

## Setup Presentation Baseline

The following presentation is the maintained V1 design baseline. Equivalent
presentation changes are permitted when they preserve the normative product and
UX requirements above. One broad rendering snapshot and manual review maintain
this baseline. Exact wording, spacing, indentation, action order, one-action-per-
line layout, existing-before-new tiers, least-identity group placement,
rule-label order, headings, statuses, warnings, collision treatment, unavailable
treatment, and integration-row presentation are non-normative.

## 19. Examples

Given active values:

```text
GITHUB_TOKEN = ghp_CANARY_123456
SHORT_TOKEN  = CANARY_123
```

this string:

```text
Authorization: ghp_CANARY_123456; fallback=CANARY_123
```

becomes:

```text
Authorization: <SECRET:GITHUB_TOKEN>; fallback=<SECRET:SHORT_TOKEN>
```

If `TOKEN=TOKEN`, `<SECRET:TOKEN>` is unsafe because it reproduces the active
value. The implementation tries `<SECRET>` and then deletion according to
`RED-006`.

If patterns `abc` and `abcd` are enrolled, input `zabcd` replaces `abcd`. If
patterns overlap at different starts, scanning chooses the earliest start before
considering a later longer match.
