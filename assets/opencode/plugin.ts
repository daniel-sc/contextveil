// ContextVeil managed plugin. Do not edit; `contextveil setup` rewrites this file.
// Thin V1/V2 protocol translation; all resolution and matching stay in Rust.
// See OCO-001 through OCO-005 and LIM-016 for coverage and host limitations.

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
      stderr: "ignore",
      // `SRC-001`: environment sources resolve from the environment this
      // process inherited. Bun.spawn's implicit default is a snapshot taken at
      // startup, so the current environment is forwarded explicitly.
      env: { ...globalThis.process.env },
    });
  } catch {
    throw new Error("ContextVeil could not be started; run contextveil doctor");
  }

  let timer: ReturnType<typeof setTimeout>;
  const timeout = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(() => {
      process.kill();
      reject(new Error("ContextVeil timed out"));
    }, TIMEOUT_MS);
  });
  let stdout: string;
  let exitCode: number;
  try {
    process.stdin.write(request);
    process.stdin.end();
    [stdout, exitCode] = await Promise.race([
      Promise.all([new Response(process.stdout).text(), process.exited]),
      timeout,
    ]);
  } catch {
    process.kill();
    throw new Error("ContextVeil could not be run or timed out; run contextveil doctor");
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
  if (!response || typeof response !== "object" || response.version !== PROTOCOL_VERSION) {
    throw new Error("ContextVeil returned an unsupported protocol version");
  }
  if (response.status === "ok") {
    const answer = response as unknown as Redaction;
    if (
      typeof answer.changed !== "boolean" ||
      !Array.isArray(answer.texts) || answer.texts.length !== texts.length ||
      !answer.texts.every((text) => typeof text === "string") ||
      (answer.notification !== undefined && typeof answer.notification !== "string") ||
      (answer.warnings !== undefined && (!Array.isArray(answer.warnings) ||
        !answer.warnings.every((warning) => typeof warning === "string"))) ||
      (!answer.changed && answer.texts.some((text, index) => text !== texts[index]))
    ) {
      throw new Error("ContextVeil returned an unexpected number of values");
    }
    return answer;
  }
  // A malfunction or protocol error must abort the covered operation.
  throw new Error("ContextVeil reported a malfunction; run contextveil doctor");
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

type Slot = { object: any; key: string | number };

function object(value: any): Record<string, any> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("ContextVeil received malformed OpenCode content");
  }
  return value;
}

function array(value: any): any[] {
  if (!Array.isArray(value)) {
    throw new Error("ContextVeil received malformed OpenCode content");
  }
  return value;
}

// Copies only selected branches. Opaque media instances must retain their
// prototypes, and original history must never be mutated through shared objects.
function requestDraft(event: any) {
  const slots: Slot[] = [];
  const text = (target: any, key: string | number, optional = false) => {
    if (optional && target[key] === undefined) return;
    if (typeof target[key] !== "string") {
      throw new Error("ContextVeil received malformed OpenCode text");
    }
    slots.push({ object: target, key });
  };
  const json = (target: any, key: string | number) => {
    const pending: (Slot | { leave: object })[] = [{ object: target, key }];
    const ancestors = new Set<object>();
    while (pending.length) {
      const slot = pending.pop()!;
      if ("leave" in slot) { ancestors.delete(slot.leave); continue; }
      const value = slot.object[slot.key];
      if (typeof value === "string") slots.push(slot);
      else if (value !== null && typeof value === "object") {
        if (ancestors.has(value)) throw new Error("ContextVeil received cyclic OpenCode content");
        ancestors.add(value);
        pending.push({ leave: value });
        const copy = Array.isArray(value) ? [...value] : { ...value };
        slot.object[slot.key] = copy;
        for (const key of Object.keys(copy)) pending.push({ object: copy, key });
      }
    }
  };
  const toolContent = (value: any) => array(value).map((original) => {
    const part = { ...object(original) };
    if (part.type === "text") text(part, "text");
    else if (part.type === "file") text(part, "name", true);
    else throw new Error("ContextVeil received an unknown OpenCode tool-content type");
    return part;
  });
  const content = (value: any) => array(value).map((original) => {
    const part = { ...object(original) };
    switch (part.type) {
      case "text": case "reasoning": text(part, "text"); break;
      case "compaction":
        if (part.text !== null) text(part, "text", true);
        break;
      case "media": text(part, "filename", true); break;
      case "effort": break;
      case "tool-call": json(part, "input"); break;
      case "tool-result": {
        part.result = { ...object(part.result) };
        if (["text", "json", "error"].includes(part.result.type)) json(part.result, "value");
        else if (part.result.type === "content") part.result.value = toolContent(part.result.value);
        else throw new Error("ContextVeil received an unknown OpenCode result type");
        break;
      }
      default: throw new Error("ContextVeil received an unknown OpenCode content type");
    }
    return part;
  });
  const schema = (input: any): any => {
    if (typeof input === "boolean") return input;
    const root = { ...object(input) };
    const pending: { source: any; node: any; leave?: boolean }[] = [{ source: input, node: root }];
    const ancestors = new Set<object>();
    while (pending.length) {
      const { source, node, leave } = pending.pop()!;
      if (leave) { ancestors.delete(source); continue; }
      if (ancestors.has(source)) throw new Error("ContextVeil received cyclic OpenCode schema");
      ancestors.add(source);
      pending.push({ source, node, leave: true });
      for (const key of ["title", "description", "$comment"]) text(node, key, true);
      for (const key of ["default", "examples", "const", "enum"]) {
        if (Object.hasOwn(node, key)) json(node, key);
      }
      const child = (parent: any, key: string | number) => {
        if (typeof parent[key] === "boolean") return;
        const source = object(parent[key]);
        parent[key] = { ...source };
        pending.push({ source, node: parent[key] });
      };
      for (const key of ["properties", "patternProperties", "$defs", "definitions", "dependentSchemas"]) {
        if (node[key] === undefined) continue;
        node[key] = { ...object(node[key]) };
        for (const name of Object.keys(node[key])) child(node[key], name);
      }
      if (node.dependencies !== undefined) {
        node.dependencies = { ...object(node.dependencies) };
        for (const name of Object.keys(node.dependencies)) {
          if (!Array.isArray(node.dependencies[name])) child(node.dependencies, name);
        }
      }
      for (const key of ["allOf", "anyOf", "oneOf", "prefixItems"]) {
        if (node[key] === undefined) continue;
        node[key] = [...array(node[key])];
        node[key].forEach((_value: any, index: number) => child(node[key], index));
      }
      for (const key of ["items", "additionalItems", "additionalProperties", "contains", "not", "if", "then", "else", "propertyNames", "unevaluatedItems", "unevaluatedProperties", "contentSchema"]) {
        if (node[key] === undefined) continue;
        if (key === "items" && Array.isArray(node[key])) {
          node[key] = [...node[key]];
          node[key].forEach((_value: any, index: number) => child(node[key], index));
        } else child(node, key);
      }
    }
    return root;
  };
  const system = array(event.system).map((original) => {
    const part = { ...object(original) };
    if (part.type !== "text") throw new Error("ContextVeil received an unknown OpenCode system type");
    text(part, "text");
    return part;
  });
  const messages = array(event.messages).map((original) => {
    const message = { ...object(original) };
    message.content = content(message.content);
    return message;
  });
  const tools = event.tools === undefined ? undefined : { ...object(event.tools) };
  for (const name of Object.keys(tools ?? {})) {
    tools[name] = { ...object(tools[name]) };
    text(tools[name], "description");
    tools[name].input = schema(tools[name].input);
  }
  return { system, messages, tools, slots };
}

export default {
  id: "contextveil",
  server: ContextVeilPlugin,
  async setup(ctx: any) {
    for (const eventName of ["context", "compaction", "generate", "title"]) {
      await ctx.session.hook(eventName, async (event: any) => {
        let draft: ReturnType<typeof requestDraft>;
        let root: string;
        try {
          const session = await ctx.session.get({ sessionID: event.sessionID });
          root = session.location.directory;
          if (typeof root !== "string" || !root.startsWith("/")) throw new Error();
          draft = requestDraft(event);
        } catch {
          throw new Error("ContextVeil could not prepare the OpenCode request; run contextveil doctor");
        }
        // Even an empty request checks the registry; malformed enrollment must
        // not become a silent success just because no text was selected.
        const answer = await redact(eventName, draft.slots.map(({ object, key }) => object[key]), root);
        if (answer.changed) {
          draft.slots.forEach(({ object, key }, index) => { object[key] = answer.texts[index]; });
          event.system = draft.system;
          event.messages = draft.messages;
          if (draft.tools !== undefined) event.tools = draft.tools;
        }
      });
    }
  },
};
