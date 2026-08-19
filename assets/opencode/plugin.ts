// ContextVeil managed plugin. Do not edit; `contextveil setup` rewrites this file.
//
// This file is a thin translator (`architecture.md`, `OCO-004`): it carries no
// matcher, resolver, or replacement logic. It sends the model-visible strings of
// two documented V1 hooks to the ContextVeil binary and writes the answers back.
//
// `OCO-001`: one JSON request on stdin, one JSON response on stdout, per event.
// `OCO-002`: `chat.message` new textual user parts and `tool.execute.after`
// successful standard textual output.
// `OCO-003`: one safe named and count TUI notification when redaction occurred.
// `RUN-003`: a subprocess failure, a timeout, invalid protocol, or a reported
// malfunction throws, which aborts the covered operation. A notification failure
// after a successful mutation never undoes the sanitized result.
// `RUN-004`: the subprocess gets five seconds.

const CONTEXTVEIL_BINARY = "__CONTEXTVEIL_BINARY__";
const PROTOCOL_VERSION = 1;
const TIMEOUT_MS = 5000;

type Redaction = {
  changed: boolean;
  texts: string[];
  notification?: string;
  warnings?: string[];
};

async function redact(
  event: string,
  texts: string[],
  projectRoot: string | undefined,
): Promise<Redaction> {
  const request = JSON.stringify({
    version: PROTOCOL_VERSION,
    event,
    project_root: projectRoot,
    texts,
  });

  let process: ReturnType<typeof Bun.spawn>;
  try {
    process = Bun.spawn([CONTEXTVEIL_BINARY, "hook", "opencode"], {
      stdin: "pipe",
      stdout: "pipe",
      stderr: "pipe",
      // `SRC-001`: environment sources resolve from the environment this
      // process inherited. Bun.spawn's implicit default is a snapshot taken at
      // startup, so the current environment is forwarded explicitly.
      env: { ...globalThis.process.env },
    });
  } catch (cause) {
    throw new Error(`ContextVeil could not be started: ${cause}`);
  }

  const timer = setTimeout(() => process.kill(), TIMEOUT_MS);
  let stdout: string;
  let exitCode: number;
  try {
    process.stdin.write(request);
    process.stdin.end();
    [stdout, exitCode] = await Promise.all([
      new Response(process.stdout).text(),
      process.exited,
    ]);
  } catch (cause) {
    throw new Error(`ContextVeil could not be run: ${cause}`);
  } finally {
    clearTimeout(timer);
  }

  if (exitCode !== 0) {
    throw new Error(`ContextVeil exited with status ${exitCode}`);
  }

  let response: Record<string, unknown>;
  try {
    response = JSON.parse(stdout);
  } catch {
    throw new Error("ContextVeil returned invalid protocol output");
  }
  if (response.version !== PROTOCOL_VERSION) {
    throw new Error("ContextVeil returned an unsupported protocol version");
  }
  if (response.status === "ok") {
    const answer = response as unknown as Redaction;
    if (!Array.isArray(answer.texts) || answer.texts.length !== texts.length) {
      throw new Error("ContextVeil returned an unexpected number of values");
    }
    return answer;
  }
  // A malfunction or protocol error must abort the covered operation.
  const message =
    typeof response.message === "string"
      ? response.message
      : "ContextVeil reported a malfunction";
  throw new Error(message);
}

export const ContextVeilPlugin = async ({ client, worktree, directory }: any) => {
  const projectRoot: string | undefined = worktree ?? directory;

  const notify = async (message: string, variant: "info" | "warning") => {
    // A failed notification must not undo a successful mutation (`RUN-003`).
    try {
      await client.tui.showToast({ body: { message, variant } });
    } catch {
      // Intentionally ignored.
    }
  };

  const announce = async (answer: Redaction) => {
    if (answer.notification) {
      await notify(answer.notification, "info");
    }
    for (const warning of answer.warnings ?? []) {
      await notify(warning, "warning");
    }
  };

  return {
    "chat.message": async (_input: any, output: any) => {
      const parts = (output?.parts ?? []).filter(
        (part: any) =>
          part && part.type === "text" && typeof part.text === "string" && part.text.length > 0,
      );
      if (parts.length === 0) {
        return;
      }
      const answer = await redact(
        "chat.message",
        parts.map((part: any) => part.text),
        projectRoot,
      );
      if (answer.changed) {
        parts.forEach((part: any, index: number) => {
          part.text = answer.texts[index];
        });
      }
      await announce(answer);
    },

    "tool.execute.after": async (_input: any, output: any) => {
      // Only the standard textual output is covered (`OCO-002`, `OCO-004`).
      if (!output || typeof output.output !== "string" || output.output.length === 0) {
        return;
      }
      const answer = await redact("tool.execute.after", [output.output], projectRoot);
      if (answer.changed) {
        output.output = answer.texts[0];
      }
      await announce(answer);
    },
  };
};
