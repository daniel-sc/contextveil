# JSON Sources Use JSON5

## Status

Accepted

## Context

Some recognized credential stores, including common GitHub Copilot CLI
configuration, use comments or other JSON5 syntax. Treating those documents as
strict JSON excludes a normal credential location even though ContextVeil's
source model already persists an exact file and RFC 6901 pointer.

Using a comments-only dialect would create an ambiguous partial grammar and would
not provide one stable rule for manually enrolled and automatically discovered
documents.

## Decision

Every enrolled or discovered document persisted with `source = "json"` uses the
full JSON5 grammar. Duplicate object members remain invalid at every depth.
Documents deeper than 128 nested object or array containers are rejected before
recursive deserialization to protect the one-shot hook process stack.
Selection remains an exact plain-string RFC 6901 JSON Pointer, and only a selected
non-empty string resolves. Interpolation, wildcard traversal, key-name search,
decoding, and other transformations remain prohibited.

The public phrase remains **JSON source**. This decision changes source-document
parsing only. Harness protocols, hook payloads, integration configuration files,
and other protocol JSON remain strict unless their own contract separately says
otherwise.

## Consequences

- Comment-bearing Copilot configuration can participate in recognized store
  schema-family rules.
- One grammar applies consistently to manual and discovered JSON sources.
- Source parsing and strict protocol parsing must remain separate boundaries.
- Existing JSON documents remain valid JSON5; duplicate-member rejection and
  RFC 6901/string-resolution behavior do not change.
