# ContextVeil

ContextVeil is a local redaction primitive that keeps user-enrolled secrets out
of supported coding-agent model-context boundaries.

## Language

**Source Reference**:
A durable pointer naming where a protected value can be resolved without storing
the value itself.
_Avoid_: Secret snapshot, stored secret

**Source Identity**:
The stable equality and ordering key of a Source Reference. It contains the
source kind and identifying fields, never a resolved value or advisory detail.
_Avoid_: Candidate rank, confidence key

**Known Source Rule**:
A maintained, deterministic setup-time rule that identifies a source as eligible
for automatic Candidate admission. The supported rule families are the
secret-like name rule, credential-bearing URL rule, properties configuration
rule, npmrc credentials rule, and recognized credential document rules. Every
applicable rule runs regardless of which adapters are selected or installed.
_Avoid_: Detector, source adapter, adapter-specific discovery

Recognized credential document rules are bounded location and field probes that
identify eligible non-empty string values without validating unrelated
surrounding schema. Automatic eligibility remains subject to the Common Literal
exclusion.

**Known Source**:
A local source recognized by a Known Source Rule. Use this shorter phrase only
when referring to a source or to the inventory collectively, not to the rule
itself.
_Avoid_: Rule name, runtime source type

**Enrolled Source**:
A source reference or file policy the user has chosen to protect.
_Avoid_: Detected secret, scanned secret

**Candidate**:
A Source Reference that setup presents for possible enrollment after Known Source
Rule eligibility and automatic-candidate exclusions, or after explicit manual
addition.
_Avoid_: Detected secret, confirmed secret

Filesystem enumeration supplies inputs to Known Source Rules but does not admit
a Candidate by itself. Manual addition admits a Candidate but is not a Known
Source Rule and bypasses automatic-candidate exclusions.

**Common Literal**:
A decoded, trimmed textual value excluded from wholly new automatic Candidates
when its complete value exactly matches a small maintained vocabulary under
ASCII case folding. This setup-only exclusion does not affect existing
enrollment, explicit manual additions, dotenv wildcard enrollment, source
resolution, or runtime matching.
_Avoid_: Non-secret value, runtime ignore list, secret denylist

**JSON Source**:
An enrolled or discovered UTF-8 JSON5 document persisted with `source = "json"`
and resolved through one exact RFC 6901 JSON Pointer. `JSON source` is the public
phrase even though the accepted document grammar is JSON5.
_Avoid_: JSONC source, JSON5 source type

**Candidate Group**:
One setup enrollment unit within one scope containing candidate source
references whose currently resolved values are equal. The group has one
selection state; selecting it enrolls every represented source so aliases remain
protected if their values later diverge.
_Avoid_: Duplicate secret, merged source

**Group Representative**:
The least Source Reference in a Candidate Group under Source Identity order. It
identifies the group for deterministic setup presentation and is distinct from
runtime canonicalization in the Effective Registry.
_Avoid_: Canonical source, preferred source

**Resolved Secret**:
The current textual value obtained from an enrolled source, trimmed with Rust's
standard Unicode whitespace definition and non-empty afterward.
_Avoid_: Credential record, stored secret

**Properties Source**:
An enrolled or discovered Java-style properties file persisted with
`source = "properties"` and resolved through one exact decoded key.
_Avoid_: Properties wildcard, inferred file source

**npmrc Source**:
An enrolled or discovered npm configuration file persisted with
`source = "npmrc"` and resolved through one exact case-sensitive key using
ContextVeil's narrow scalar grammar.
_Avoid_: Generic INI source, npm configuration snapshot

**Global Registry**:
The user's machine-scoped collection of enrolled sources.
_Avoid_: Global vault, system policy

**Project Registry**:
The project-scoped collection of enrolled sources described by the project's
`.contextveil.toml`.
_Avoid_: Repository vault, project secrets

**Effective Registry**:
The additive combination of the global registry and the one selected project
registry for a runtime event.
_Avoid_: Merged config, override policy

**Unresolved Source**:
An enrolled source that currently has no usable value because it is absent,
unset, empty, or is a non-UTF-8 environment value. Failure to decode or parse a
required textual source is a malfunction instead.
_Avoid_: Failure, invalid secret

**Malfunction**:
A configuration, source, protocol, or execution error that prevents trustworthy
use of the effective registry.
_Avoid_: Unresolved source, missing optional secret

**Match**:
An occurrence of a resolved secret inside one model-visible string value.
_Avoid_: Finding, heuristic detection

**Redaction**:
The deterministic replacement of a match before covered content reaches a
model.
_Avoid_: Encryption, masking, deletion

**Placeholder**:
The non-secret marker inserted by a redaction when a safe marker can be emitted.
_Avoid_: Token, grant, secret handle

**Intervention**:
The semantic result that one or more redactions occurred, including counts and
optional emit-safe labels but never matched values.
_Avoid_: Alert, policy violation

**Adapter**:
A harness-specific translator between a coding agent's extension protocol and
the shared ContextVeil behavior.
_Avoid_: Security core, provider proxy

**Coverage**:
The model-bound content paths a particular adapter can demonstrably mutate
before model consumption.
_Avoid_: Protection certificate, universal support

**Collision**:
An occurrence of a candidate value elsewhere in the current project that warns
the user the value may be too common for useful literal redaction.
_Avoid_: Match, duplicate source
