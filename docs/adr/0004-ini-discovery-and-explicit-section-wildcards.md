# INI Discovery And Explicit Section Wildcards

## Status

Accepted

## Decision

Discover project INI entries through the existing bounded traversal and key/URL
eligibility rules, persisting exact references by default. Also permit explicit
enrollment of one key across all current and future sections, including
sectionless entries. This broadens [issue #14](https://github.com/daniel-sc/contextveil/issues/14)'s
original store-only scope while
keeping future enrollment under deliberate user control.

Some consumers disregard sections, so moving a key can leave application behavior
unchanged while breaking an exact reference. An explicit section wildcard handles
that case without flattening all INI sources. It protects each section's current
value and may therefore protect more values than the application currently uses.
Use `all_sections = true` to preserve literal `*` section names. Setup exposes
this through manual INI enrollment and a contextual hint, avoiding ambiguous
conversion actions on groups containing multiple source references.

Parsing uses `rust-ini` with escape decoding disabled through a thin wrapper;
this retains a maintained grammar rather than introducing another custom parser.
It preserves unnamed sections and duplicate assignments, which `configparser`
would collapse before the wrapper could apply our source semantics.
The contract is in [CFG-019, SRC-019, and SET-024](../specification.md), with
dialect differences in [LIM-027](../limitations.md#lim-027-ini-uses-one-explicit-dialect).
