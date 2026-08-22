# ContextVeil

ContextVeil is a local redaction primitive that keeps user-enrolled secrets out
of supported coding-agent model-context boundaries.

## Language

**Source Reference**:
A durable pointer naming where a protected value can be resolved without storing
the value itself.
_Avoid_: Secret snapshot, stored secret

**Known Source Rule**:
A maintained, deterministic setup-time rule that automatically admits
candidates. The supported rule families are the secret-like name rule, the
credential-bearing URL rule, and recognized store schema-family rules. Every
applicable rule runs regardless of which adapters are selected or installed.
_Avoid_: Detector, source adapter, adapter-specific discovery

**Known Source**:
A local source recognized by a Known Source Rule. Use this shorter phrase only
when referring to a source or to the inventory collectively, not to the rule
itself.
_Avoid_: Rule name, runtime source type

**Enrolled Source**:
A source reference or file policy the user has chosen to protect.
_Avoid_: Detected secret, scanned secret

**Candidate**:
A source that setup presents for possible enrollment based on discovery and
advisory heuristics.
_Avoid_: Detected secret, confirmed secret

Manual source additions and filesystem enumeration make sources available for
review but are not Known Source Rules because they do not themselves admit a
candidate.

**JSON Source**:
An enrolled or discovered UTF-8 JSON5 document persisted with `source = "json"`
and resolved through one exact RFC 6901 JSON Pointer. `JSON source` is the public
phrase even though the accepted document grammar is JSON5.
_Avoid_: JSONC source, JSON5 source type

**Candidate Group**:
A setup choice within one enrollment scope containing candidate source references
whose currently resolved values are equal. Selecting the group enrolls every
represented source.
_Avoid_: Duplicate secret, merged source

**Resolved Secret**:
The current non-empty textual value obtained from an enrolled source.
_Avoid_: Credential record, stored secret

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
