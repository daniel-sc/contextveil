# Compaction shrinking restores text removed by the compaction hook

### Description

When a compaction request needs shrinking, `flattenAndDropOldest` rebuilds the previous checkpoint's summary and recent text from stored history. This discards edits made by `ctx.session.hook("compaction")`, so text a redaction plugin removed can reach the model again.

The [documented hook contract](https://dev.opencode.ai/v2/docs/build/plugins/#model-requests) permits editing outgoing messages without changing stored history. Expected: shrinking preserves those edits. Actual: the original checkpoint text is restored, and the compaction hook is not rerun.

### Steps to reproduce

1. Seed a completed checkpoint with a generated canary in both `summary` and `recent`.
2. Add an oversized older exchange and smaller newer exchanges; use a small model context limit and `keep: 0` to force shrinking.
3. Register a compaction hook that replaces the canary in `event.messages`.
4. Capture the request passed to the LLM client. The hook output has no canary, but the dispatched request contains it.

Reproduced in the existing compaction test harness without a live provider or real secrets. A [regression test and focused fix](https://github.com/daniel-sc/opencode/tree/rehook-compaction) are prepared for a follow-up PR.

### Plugins

Minimal compaction redaction hook.

### OpenCode version

2.0.27; also reproduced on `dev` at `5a4dd5b4301182591d48a93bbacdc5b7926cd3ca`.

### Operating System

Linux x86_64
