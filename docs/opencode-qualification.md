# OpenCode V2 Qualification

V2 remains **EXPERIMENTAL**. The four session request hooks work in the tested
ordinary paths, but OpenCode's post-hook compaction shrinking can restore a
redacted value. This record is evidence about the tested host, not a version gate
or a production certificate. See `OCO-005`, `LIM-016`, and
[ADR-0005](adr/0005-opencode-v2-coverage-boundary.md).

## Latest Released Host Check — 2026-11-23

The latest published OpenCode V2 release available at qualification time was
`v2.3.2`, published 2026-11-18. The `v2.4.0` source tag was created on
2026-11-23 but had no published GitHub release or release binary, so it was
inspected separately rather than treated as an installed host.

| Field | Evidence |
| --- | --- |
| Host | OpenCode v2.3.2, Linux x86_64 |
| Adapter | Pending ContextVeil V1/V2 managed plugin with the debug binary path substituted |
| Invocation | `mise run qualify-opencode` |
| Result | Passed the available live HTTP qualification |
| Observed requests | Five: primary, title, generate, resumed primary, and compaction |
| Malfunction behavior | Invalid enrollment produced zero provider dispatches for that session |
| Scope | Ordinary session requests only; no overflow or compaction-shrink recovery |

The run used temporary configuration, project, cache, and logs and a generated
non-credential canary. The observer recorded only request kind, session ID, and
canary/placeholder booleans. No canary appeared in observed requests or host
output, and temporary sessions were removed. Source inspection of the
`v2.4.0` tag found the same compaction implementation comment acknowledging
that shrinking rebuilds hooked checkpoint text from storage.

## Live Luna Run — 2026-10-12

| Field | Evidence |
| --- | --- |
| Host | Installed OpenCode 2.0.27, Linux x86_64 |
| Model | `openai/gpt-6-luna` |
| Adapter | Shipped `assets/opencode/plugin.ts` with the built binary path substituted |
| Invocation | `mise run qualify-opencode`; repeated with `CONTEXTVEIL_TEST_BINARY=target/release/contextveil` after `mise run build` |
| Result | Passed the limited HTTP qualification below |
| Observed requests | Five: title, primary, generate, resumed primary, compaction |
| Malfunction | Invalid enrollment failed the primary operation with zero provider dispatches for that session |

The run used temporary configuration, project, cache, and logs, with enrollment
containing only a generated non-credential canary. The host used its existing
credential database normally for authentication; credentials were not printed or
copied into fixtures. The two qualification sessions were deleted afterward.
An initial attempt using the host's legacy auth import failed authentication;
it established no successful live qualification.

A separate qualification-only plugin inspected outgoing HTTP request bodies. It
recorded only request kind, temporary session ID, and booleans for canary absence
and placeholder presence. Each of the four request kinds had a placeholder and
no original canary. Host stdout/stderr also excluded the canary. The observer
would abort on a leaked canary rather than send it. It is not installed by setup.

This run did **not** qualify WebSocket transport, overflow/shrink recovery,
arbitrary plugins, raw request overlays, binary attachments, or standalone
generation. The resume check was a second turn on the same persisted session,
not a host-process restart. The live procedure also asserts the original canary
remains in persisted history before compaction. Plugin fixtures cover source
rotation behavior. The live malformed-registry check exercised the primary
request; fixtures cover all four hook callbacks.

## Repeating The Live Check

Use the pinned project tooling and an authenticated OpenCode V2 installation:

```bash
mise install
mise run qualify-opencode
```

The task is optional, may incur provider charges, and is not part of `check` or
routine CI. It defaults to Luna. `CONTEXTVEIL_QUALIFY_MODEL` may select another
`provider/model`; the qualification config currently forces HTTP and summary
compaction for the OpenAI provider. `CONTEXTVEIL_QUALIFY_DB` may select an existing
OpenCode credential database; the default is
`~/.local/share/opencode/opencode.db`. `CONTEXTVEIL_TEST_BINARY` may select the
binary under qualification. The task creates and removes its own sessions in
that database, preserves unrelated configuration, and removes temporary data.

## Reproduced Upstream Compaction Defect

The reproduction uses OpenCode's existing `session-compaction.test.ts` harness
at tag `v2.0.27`. It seeds a previous checkpoint containing generated canaries,
registers a redacting compaction hook, and forces request shrinking with a small
context budget and an older oversized exchange. It proves the hook removed the
canary, then checks the actual request passed to the upstream LLM client.

Before the fix, the request-canary assertion failed (`expected false, received
true`). No canary value was printed in the failure output. After the fix, all 12
tests in that file passed. Including `session-native-compaction.test.ts` gave
**18 passing tests, 158 assertions**. The initial native-test attempt was denied
localhost socket access by the sandbox; rerunning with that access passed.

The third [upstream patch](upstream/opencode-2.0.27-compaction.patch) rebuilds each
reduced candidate from history, then reruns `compaction` and `model.request` on
that fresh draft. Earlier hook edits do not accumulate, and dispatch uses the
latest prepared request and options. A hook-supplied `result` completes
compaction immediately, without a model call or further reduction. Unchanged
requests retain their original checkpoint representation and cache prefix;
shrinking still drops previous recent text before newer exchanges. Native
endpoint provenance is checked again for each prepared request.

The third fix is pushed to
[`daniel-sc/opencode:rehook-compaction`](https://github.com/daniel-sc/opencode/tree/rehook-compaction)
at `e00cae164a43599cc25e5e769dfe55276abf3897`, based on upstream `dev`
`80fb31ebb7e8dfa5c26fc51ca81da8e06b618a52`. The original third approach was also tested on newer
`dev` at `5a4dd5b4301182591d48a93bbacdc5b7926cd3ca`; the fork base avoids
unrelated workflow changes that the available push token cannot publish. Both
earlier proposals remain on their separate branches: `preserve-hook-edits` protected recent text and could sacrifice
newer exchanges; `split-checkpoint-context` changed the checkpoint representation
in every compaction request, reducing prefix-cache reuse. Neither is the current
proposal. No issue or pull request has been opened; the compact
[issue draft](upstream/opencode-compaction-issue.md) points to the third branch.

Validation on 2026-10-13: the two compaction suites and adjacent model-request
suite pass with **33 tests and 326 assertions**. Six regressions fail on
unmodified upstream. The branch adds seven focused cases (six sharing a fixture
and one native routing case); three overlapping cases were removed. Coverage includes both reduction triggers, oldest-first
dropping, unchanged checkpoint layout, fresh hook state and request settings,
hook-supplied summaries, bounded retries despite hook
growth, and native routing checks after reduction. Core typechecking, focused
lint, formatting, website typechecking, generated-file checks, and the website
build pass. The first website build failed during search-index generation; both
the baseline rerun and updated build subsequently passed. The full workspace
`bun run check` also passes: lint completes with existing upstream warnings and
all 35 typecheck tasks succeed. The initially missing `ast-grep` and other root
dependencies were installed at the locked versions; moving the upstream checkout
to the larger workspace disk allowed a complete dependency install.
ContextVeil does not patch the installed host automatically.

Production qualification requires a verified host fix and repeat evidence for
the affected compaction shrink/retry paths. Passing the ordinary Luna run above
does not satisfy that prerequisite.
