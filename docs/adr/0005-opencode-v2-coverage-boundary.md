# OpenCode V2 Coverage Boundary

## Status

Accepted. The user confirmed the complete design and implementation on
2026-10-12. The specification and architecture now describe the dual adapter;
production qualification remains blocked as recorded in
[the qualification evidence](../opencode-qualification.md).

## Agreed Decisions

For OpenCode V2, redact outgoing model requests without rewriting persisted
prompts or tool results. This preserves the model-context protection boundary
without introducing a separate, partial promise to sanitize local storage.

Retain the existing experimental OpenCode V1 integration while pursuing
production readiness for V2. Qualify their coverage separately rather than
requiring feature parity or removing existing users' integration.

Production support is the intended outcome, conditional on host-level evidence;
it is not a statement of current support.

The V2 boundary uses the separate `context`, `compaction`, `generate`,
and `title` session hooks. The [official plugin reference](https://dev.opencode.ai/v2/docs/build/plugins/#model-requests)
documents distinct request paths and outgoing-only mutations. These four hooks
cover complementary requests and share the Rust redaction core. Do not initially
add prompt-admission, tool-result, or transport interception; additional hooks
require evidence of a specific coverage gap.

Cover model-visible textual content exposed by these hooks, including system
instructions, conversation text, textual tool results, and tool descriptions
and schema text. Preserve structural identifiers and protocol fields; binary
attachments remain outside coverage. Field mappings are defined in `OCO-002` after schema review.

Do not gate execution or warn solely because the host version is untested.
Record tested versions in release evidence and diagnose observed problems.
This does not authorize claiming compatibility in the presence of a known gap.

Standalone plugin generation through `ctx.generate.text` is outside coverage;
the V2 claim covers the four session request paths rather than arbitrary plugin
model calls.

Resolve the known post-hook compaction restoration before qualifying V2 for
production. First reproduce it, then investigate a minimal upstream fix or a
verified safe workaround. Keep V2 experimental until qualification succeeds;
excluding ordinary compaction from the promise is not the chosen resolution.

Use ordinary offline adapter fixtures plus a small live Luna qualification
procedure covering the four session request paths, resume, and malfunction
behavior. Live Luna tests are authorized and their cost is acceptable. Reuse
OpenCode's existing regression-test harness for the compaction defect rather
than building a general host/fake-provider framework. Qualification must inspect
request-boundary evidence; absence from a model reply alone does not demonstrate
absence from the request. Routine CI remains independent of paid live calls.

Carry forward the existing abort-on-malfunction policy and five-second runtime
timeout. Preserve the existing requirements for review of competing plugins,
unchanged managed-artifact ownership, and preservation of user-edited or
unrelated configuration. Keeping V1 support requires recognizing earlier
managed artifacts and protocol requests during migration. Implementation must
reconcile the normative documents before changing these runtime boundaries.

## Open Design And Qualification

Field mappings, installation ownership migration, six-event offline checks, and
focused live qualification are implemented. Production qualification still
requires the upstream compaction fix and evidence for its shrink/retry paths.
Changes that broaden the agreed coverage boundary or weaken failure behavior
require renewed review.

Source inspection of [OpenCode v2.0.24 compaction](https://github.com/anomalyco/opencode/blob/v2.0.24/packages/core/src/session/compaction.ts#L369)
found that request shrinking can rebuild summary/transcript content from stored
history after hook mutation. Four-hook coverage needs verification against a
current pinned release before accepting compaction coverage; this is a source
finding, not a reproduced runtime leak. The same release's
[standalone generation path](https://github.com/anomalyco/opencode/blob/v2.0.24/packages/core/src/generate.ts#L60)
bypasses session request hooks. These findings do not establish the behavior of
later releases without inspection.

On 2026-10-12, the latest published V2 release verified was `2.0.27`. Its
[compaction implementation](https://github.com/anomalyco/opencode/blob/v2.0.27/packages/core/src/session/compaction.ts#L427)
retains the same post-hook restoration, including a comment acknowledging lost
hook edits. This blocks an unrestricted production claim for the four-hook design. The
upstream regression reproduces the defect; a focused patch passes the regression
and upstream compaction tests. Installed hosts remain unpatched.

The implementation revises `OCO-001` through `OCO-004`, affected
support and qualification requirements, the OpenCode architecture section, and
`LIM-016`. The updated contracts remain authoritative.
