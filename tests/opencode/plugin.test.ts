// Protocol fixtures for the managed OpenCode plugin (`OCO-001` - `OCO-004`).
//
// The plugin is loaded from the same template the installer ships, with the
// binary path substituted exactly as `integration::opencode::render` does, and it
// is driven against the real ContextVeil binary over the documented transport.
//
// Run it with `mise run test-plugin`.

import { afterAll, beforeAll, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const TEMPLATE = resolve(import.meta.dir, "../../assets/opencode/plugin.ts");
const BINARY = resolve(
  process.env.CONTEXTVEIL_TEST_BINARY ?? "target/debug/contextveil",
);
const MISSING_BINARY = "/nonexistent/contextveil";

let workspace: string;
let invalidProtocolStub: string;
let failingStub: string;

/** Every binary path the fixtures substitute into the template, to its instance. */
const instances = new Map<string, string>();

/**
 * Writes one plugin instance per binary path, substituting it exactly as
 * `integration::opencode::render` does.
 *
 * Bun's resolver caches a directory's entries the first time it resolves inside
 * it, so a file written afterwards can stay invisible to `import`
 * (`oven-sh/bun#20013`, observed on macOS). Every instance is therefore written
 * before the first import rather than one per call.
 */
async function writeInstances(binaries: string[]) {
  const source = await Bun.file(TEMPLATE).text();
  for (const [index, binary] of binaries.entries()) {
    const instance = join(workspace, `plugin-${index}.ts`);
    writeFileSync(
      instance,
      source.replace('"__CONTEXTVEIL_BINARY__"', JSON.stringify(binary)),
    );
    instances.set(binary, instance);
  }
}

/** Loads the instance pointing at `binary` and returns its hooks. */
async function loadPlugin(binary: string, client: any) {
  const instance = instances.get(binary);
  if (!instance) {
    throw new Error(
      `no plugin instance was written for ${binary}; add it to writeInstances`,
    );
  }
  // The template holds no mutable state, so re-driving a cached instance with a
  // fresh client is equivalent to loading it again.
  const module = await import(instance);
  const plugin = Object.values(module).find(
    (value) => typeof value === "function",
  ) as (input: any) => Promise<any>;
  expect(plugin).toBeTypeOf("function");
  return plugin({ client, worktree: workspace, directory: workspace });
}

/** A TUI client that records toasts, or throws when asked to. */
function recordingClient(options: { failing?: boolean } = {}) {
  const toasts: any[] = [];
  return {
    toasts,
    tui: {
      showToast: async (call: any) => {
        if (options.failing) {
          throw new Error("the TUI is not available");
        }
        toasts.push(call);
        return call;
      },
    },
  };
}

/** Points ContextVeil at a temporary configuration enrolling one variable. */
function enroll(name: string, value: string) {
  const root = mkdtempSync(join(tmpdir(), "contextveil-plugin-config-"));
  mkdirSync(join(root, "contextveil"), { recursive: true });
  writeFileSync(
    join(root, "contextveil", "config.toml"),
    `version = 1\n\n[[secret]]\nsource = "env"\nname = "${name}"\n`,
  );
  process.env.XDG_CONFIG_HOME = root;
  process.env[name] = value;
  return root;
}

/** Points ContextVeil at an invalid configuration. */
function enrollInvalid() {
  const root = mkdtempSync(join(tmpdir(), "contextveil-plugin-broken-"));
  mkdirSync(join(root, "contextveil"), { recursive: true });
  writeFileSync(
    join(root, "contextveil", "config.toml"),
    'version = 1\n\n[[secret]]\nsource = "nope"\n',
  );
  process.env.XDG_CONFIG_HOME = root;
  return root;
}

const CANARY = `SSCANARY-PLUGIN-${crypto.randomUUID()}`;

beforeAll(async () => {
  workspace = mkdtempSync(join(tmpdir(), "contextveil-plugin-"));
  invalidProtocolStub = join(workspace, "invalid-protocol.sh");
  failingStub = join(workspace, "failing.sh");
  await writeInstances([
    BINARY,
    MISSING_BINARY,
    invalidProtocolStub,
    failingStub,
  ]);
});

afterAll(() => {
  rmSync(workspace, { recursive: true, force: true });
});

test("the plugin binary under test exists", async () => {
  expect(await Bun.file(BINARY).exists()).toBe(true);
});

test("new user text is redacted in place and announced", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  const client = recordingClient();
  const hooks = await loadPlugin(BINARY, client);

  const parts = [
    { type: "text", text: `deploy with ${CANARY}` },
    { type: "text", text: "and nothing else" },
    { type: "file", filename: "notes.txt" },
  ];
  await hooks["chat.message"]({ sessionID: "s1" }, { message: {}, parts });

  expect(parts[0].text).toBe("deploy with <SECRET:PLUGIN_TOKEN>");
  expect(parts[1].text).toBe("and nothing else");
  expect(parts[2]).toEqual({ type: "file", filename: "notes.txt" });
  // `OCO-003`: one safe named and count notification.
  expect(client.toasts).toHaveLength(1);
  expect(client.toasts[0].body.message).toContain("PLUGIN_TOKEN");
  expect(client.toasts[0].body.message).not.toContain(CANARY);
});

test("successful standard tool output is redacted in place", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  const client = recordingClient();
  const hooks = await loadPlugin(BINARY, client);

  const output = { title: "shell", output: `token=${CANARY}`, metadata: { exit: 0 } };
  await hooks["tool.execute.after"]({ tool: "bash", sessionID: "s1" }, output);

  expect(output.output).toBe("token=<SECRET:PLUGIN_TOKEN>");
  // Uncovered fields are left exactly as they were (`OCO-004`).
  expect(output.title).toBe("shell");
  expect(output.metadata).toEqual({ exit: 0 });
  expect(client.toasts).toHaveLength(1);
});

test("clean events change nothing and stay silent", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  const client = recordingClient();
  const hooks = await loadPlugin(BINARY, client);

  const parts = [{ type: "text", text: "nothing sensitive" }];
  await hooks["chat.message"]({ sessionID: "s1" }, { message: {}, parts });
  const output = { title: "shell", output: "all clear", metadata: {} };
  await hooks["tool.execute.after"]({ tool: "bash" }, output);

  expect(parts[0].text).toBe("nothing sensitive");
  expect(output.output).toBe("all clear");
  expect(client.toasts).toHaveLength(0);
});

test("explicitly unsupported paths are left alone without spawning", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  // A missing binary would throw if the plugin tried to run it, so these cases
  // also prove no subprocess is started.
  const hooks = await loadPlugin(MISSING_BINARY, recordingClient());

  const parts = [{ type: "file", filename: "a.txt" }];
  await hooks["chat.message"]({ sessionID: "s1" }, { message: {}, parts });
  expect(parts[0]).toEqual({ type: "file", filename: "a.txt" });

  for (const output of [
    { title: "t", output: "", metadata: {} },
    { title: "t", metadata: {} },
    { title: "t", output: { structured: true }, metadata: {} },
  ] as any[]) {
    await hooks["tool.execute.after"]({ tool: "bash" }, output);
  }
  await hooks["chat.message"]({ sessionID: "s1" }, { message: {}, parts: [] });
});

test("a subprocess failure aborts the covered operation", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  const hooks = await loadPlugin(MISSING_BINARY, recordingClient());
  const parts = [{ type: "text", text: CANARY }];

  // `RUN-003`: the plugin throws rather than passing content through.
  expect(
    hooks["chat.message"]({ sessionID: "s1" }, { message: {}, parts }),
  ).rejects.toThrow();
  expect(parts[0].text).toBe(CANARY);
});

test("invalid protocol output aborts the covered operation", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  writeFileSync(invalidProtocolStub, "#!/bin/sh\necho 'not json'\n", {
    mode: 0o755,
  });
  const hooks = await loadPlugin(invalidProtocolStub, recordingClient());

  expect(
    hooks["tool.execute.after"](
      { tool: "bash" },
      { title: "t", output: CANARY, metadata: {} },
    ),
  ).rejects.toThrow(/invalid protocol output/);
});

test("a nonzero exit status aborts the covered operation", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  writeFileSync(failingStub, "#!/bin/sh\nexit 3\n", { mode: 0o755 });
  const hooks = await loadPlugin(failingStub, recordingClient());

  expect(
    hooks["tool.execute.after"](
      { tool: "bash" },
      { title: "t", output: CANARY, metadata: {} },
    ),
  ).rejects.toThrow(/status 3/);
});

test("a reported malfunction aborts the covered operation", async () => {
  enrollInvalid();
  const hooks = await loadPlugin(BINARY, recordingClient());
  const output = { title: "t", output: CANARY, metadata: {} };

  // `RUN-001` and `RUN-003`: no partial redaction, and the operation aborts.
  expect(
    hooks["tool.execute.after"]({ tool: "bash" }, output),
  ).rejects.toThrow(/doctor/);
  expect(output.output).toBe(CANARY);
});

test("a notification failure does not undo the mutation", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  const hooks = await loadPlugin(BINARY, recordingClient({ failing: true }));
  const output = { title: "t", output: `token=${CANARY}`, metadata: {} };

  // `RUN-003`: notification failure after a successful mutation is ignored.
  await hooks["tool.execute.after"]({ tool: "bash" }, output);
  expect(output.output).toBe("token=<SECRET:PLUGIN_TOKEN>");
});

test("an incomplete global setup is surfaced as a warning toast", async () => {
  const root = mkdtempSync(join(tmpdir(), "contextveil-plugin-empty-"));
  process.env.XDG_CONFIG_HOME = root;
  const client = recordingClient();
  const hooks = await loadPlugin(BINARY, client);

  await hooks["chat.message"](
    { sessionID: "s1" },
    { message: {}, parts: [{ type: "text", text: "hello" }] },
  );
  expect(client.toasts).toHaveLength(1);
  expect(client.toasts[0].body.variant).toBe("warning");
  expect(client.toasts[0].body.message).toContain("setup is incomplete");
});
