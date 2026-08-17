// Protocol fixtures for the managed OpenCode plugin (`OCO-001` - `OCO-004`).
//
// The plugin is loaded from the same template the installer ships, with the
// binary path substituted exactly as `integration::opencode::render` does, and it
// is driven against the real SecretSieve binary over the documented transport.
//
// Run it with `mise run test-plugin`.

import { afterAll, beforeAll, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const TEMPLATE = resolve(import.meta.dir, "../../assets/opencode/plugin.ts");
const BINARY = resolve(
  process.env.SECRETSIEVE_TEST_BINARY ?? "target/debug/secretsieve",
);

let workspace: string;

/** Writes a plugin instance pointing at `binary` and returns its hooks. */
async function loadPlugin(binary: string, client: any) {
  const source = await Bun.file(TEMPLATE).text();
  const instance = join(workspace, `plugin-${crypto.randomUUID()}.ts`);
  writeFileSync(
    instance,
    source.replace('"__SECRETSIEVE_BINARY__"', JSON.stringify(binary)),
  );
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

/** Points SecretSieve at a temporary configuration enrolling one variable. */
function enroll(name: string, value: string) {
  const root = mkdtempSync(join(tmpdir(), "secretsieve-plugin-config-"));
  mkdirSync(join(root, "secretsieve"), { recursive: true });
  writeFileSync(
    join(root, "secretsieve", "config.toml"),
    `version = 1\n\n[[secret]]\nsource = "env"\nname = "${name}"\n`,
  );
  process.env.XDG_CONFIG_HOME = root;
  process.env[name] = value;
  return root;
}

/** Points SecretSieve at an invalid configuration. */
function enrollInvalid() {
  const root = mkdtempSync(join(tmpdir(), "secretsieve-plugin-broken-"));
  mkdirSync(join(root, "secretsieve"), { recursive: true });
  writeFileSync(
    join(root, "secretsieve", "config.toml"),
    'version = 1\n\n[[secret]]\nsource = "nope"\n',
  );
  process.env.XDG_CONFIG_HOME = root;
  return root;
}

const CANARY = `SSCANARY-PLUGIN-${crypto.randomUUID()}`;

beforeAll(() => {
  workspace = mkdtempSync(join(tmpdir(), "secretsieve-plugin-"));
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
  const hooks = await loadPlugin("/nonexistent/secretsieve", recordingClient());

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
  const hooks = await loadPlugin("/nonexistent/secretsieve", recordingClient());
  const parts = [{ type: "text", text: CANARY }];

  // `RUN-003`: the plugin throws rather than passing content through.
  expect(
    hooks["chat.message"]({ sessionID: "s1" }, { message: {}, parts }),
  ).rejects.toThrow();
  expect(parts[0].text).toBe(CANARY);
});

test("invalid protocol output aborts the covered operation", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  const stub = join(workspace, "invalid-protocol.sh");
  writeFileSync(stub, "#!/bin/sh\necho 'not json'\n", { mode: 0o755 });
  const hooks = await loadPlugin(stub, recordingClient());

  expect(
    hooks["tool.execute.after"](
      { tool: "bash" },
      { title: "t", output: CANARY, metadata: {} },
    ),
  ).rejects.toThrow(/invalid protocol output/);
});

test("a nonzero exit status aborts the covered operation", async () => {
  enroll("PLUGIN_TOKEN", CANARY);
  const stub = join(workspace, "failing.sh");
  writeFileSync(stub, "#!/bin/sh\nexit 3\n", { mode: 0o755 });
  const hooks = await loadPlugin(stub, recordingClient());

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
  const root = mkdtempSync(join(tmpdir(), "secretsieve-plugin-empty-"));
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
