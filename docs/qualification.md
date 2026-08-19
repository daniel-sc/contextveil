# Live Claude Qualification Record (`REL-008`)

`REL-008` requires a manual live Claude test proving that a successfully redacted
tool result is still sanitized after session resume. It cannot be automated:
`TST-008` forbids gating routine CI on paid or networked tests. This file is the
release evidence for that run.

Rerun this procedure for each release. A passing run is evidence about the host
version it ran against, never a permanent certificate (`DIA-007`, `SUP-004`).

## Run of 2026-08-17

| Field | Value |
| --- | --- |
| Host | Claude Code 2.1.233 |
| Platform | `x86_64-unknown-linux-gnu`, Linux 7.0.0-1008-gcp |
| Binary | `target/release/contextveil` built from the qualified tree |
| Session id | `11111111-2222-4333-8444-555555555555`, pinned with `--session-id` |
| Billed requests | 6: one auth smoke test, two qualification turns, three `doctor` canaries |
| Result | **Passed**: intervention redacted, and the redaction survived resume |

### Attestation

This run was executed by an automated Claude Code session on the repository
operator's explicit instruction, not by a human working the terminal directly.
That is weaker evidence than `REL-008` intends: the gate is manual precisely so a
human judges the result. The artifacts below are quoted from the host's own
session transcript so a second reviewer can check the claim instead of taking
this file's word for it, and the procedure is written to be repeated.

**A human release manager must repeat or confirm this run before the release
ships.** Until that happens, treat this record as a reproducible test report
rather than as the signed-off gate.

### Isolation

The run used a throwaway `HOME` so that neither the operator's real
`~/.claude/settings.json` nor any real enrolled source took part, and so the
managed hook could be installed and inspected without touching live harness
configuration. The host credential was reached through a symlink rather than
copied, so no second copy of it was written to disk.

### What was done

1. `contextveil setup` was driven over a real pty in the isolated home. It
   discovered sources, masked every value it displayed (`SET-010`), wrote a
   configuration holding only source references, and installed the Claude
   `PostToolUse` hook with a 5-second timeout (`CLA-001`, `RUN-004`).
2. **Intervention.** One request was asked to run `printenv` for an enrolled
   generated non-credential value and reply with the output verbatim:

   ```
   claude -p "Run exactly this shell command and reply with its output verbatim,
     nothing else: printenv <VARIABLE>"
     --allowedTools "Bash(printenv *)" --session-id <SESSION> --output-format json
   ```

3. **Resume.** The same session was resumed and asked to repeat that command's
   output:

   ```
   claude -r <SESSION> -p "What was the exact output of the printenv command you
     ran earlier in this conversation? Reply with that output verbatim, nothing else."
   ```

4. **Sweep.** The generated value appeared nowhere under the isolated home: not
   in the stored session transcript, its caches, or its history.

### Artifacts

The host's own `PostToolUse` record for the intervention, showing the hook's
protocol response, its exit code, and its duration:

```json
{"type": "hook_success", "hookName": "PostToolUse:Bash", "exitCode": 0,
 "durationMs": 7,
 "command": "…/contextveil hook claude",
 "stdout": "{\"hookSpecificOutput\":{\"hookEventName\":\"PostToolUse\",\"updatedToolOutput\":{\"stdout\":\"<SECRET:VARIABLE>\",\"stderr\":\"\",\"interrupted\":false,\"isImage\":false,\"noOutputExpected\":false}},\"systemMessage\":\"ContextVeil redacted 1 value (VARIABLE)\"}"}
```

The stored tool result, the reply in the original turn, and the reply after the
resume, in transcript order:

```json
{"type": "tool_result", "content": "<SECRET:VARIABLE>", "is_error": false}
{"type": "assistant", "text": "```\n<SECRET:VARIABLE>\n```"}
{"type": "assistant", "text": "```\n<SECRET:VARIABLE>\n```"}
```

The last line is the resume proof. Across the whole transcript the placeholder
occurred three times and the generated value zero times. The real variable name
and generated value are elided above; both were synthetic and neither was a
credential.

### Why resume stays sanitized

The host persists the *replaced* tool result, as the stored `tool_result` record
above shows. Because the transcript records the placeholder rather than the
original text, a resume replays the redacted text and there is no second copy for
it to recover. This is a host behavior observed at 2.1.233, not a contract the
host offers, which is why `REL-008` demands a fresh run rather than a one-time
argument.

### Scope of the evidence

This run tested exactly one path: a successful `Bash` `PostToolUse` result
flowing through the installed hook, and its survival across one resume. It says
nothing about failed tool results, other result shapes, host telemetry
(`LIM-013`), the three experimental integrations, or any other host version.

### Defect found by this run

The live canary passed on "the generated value is absent from the reply" alone.
A reply that declined the request, or never ran the command, also contains no
value, so a request that tested nothing would have been reported as a pass while
every offline synthetic check already required the placeholder to be *present*.
The canary now classifies the reply as redacted, inconclusive, or disclosed, and
reports an inconclusive reply as a warning that proves nothing (`DIA-005`). Both
the classification and the severity mapping are pure functions covered by unit
tests, and the canary was rerun live against the tightened check, and again
after the classification and severity mapping were factored out for testing. Each
rerun reported `the live canary placeholder reached Claude's reply and the
generated value did not`, so this record describes the code that ships rather
than an earlier revision of it.
