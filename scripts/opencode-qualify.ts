// Optional live qualification, not a routine CI test. Requires authenticated
// OpenCode and uses its existing credentials only for provider authentication.
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import { join, resolve } from "node:path";

const root = mkdtempSync(join(tmpdir(), "contextveil-opencode-live-"));
const project = join(root, "project");
const config = join(root, "config");
const data = join(root, "data");
const report = join(root, "requests.jsonl");
const model = process.env.CONTEXTVEIL_QUALIFY_MODEL ?? "openai/gpt-6-luna";
const binary = resolve(process.env.CONTEXTVEIL_TEST_BINARY ?? "target/debug/contextveil");
const canary = `CONTEXTVEIL-LIVE-CANARY-${crypto.randomUUID()}`;
const port = 20000 + Math.floor(Math.random() * 30000);
const password = crypto.randomUUID();
const base = `http://127.0.0.1:${port}`;
const headers = { "content-type": "application/json", authorization: `Basic ${btoa(`opencode:${password}`)}` };
let server: ReturnType<typeof Bun.spawn> | undefined;
let stage = "prepare";
const sessions: string[] = [];

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function api(path: string, body?: unknown) {
  const response = await fetch(base + path, {
    method: body === undefined ? "GET" : "POST", headers,
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.timeout(120000),
  });
  if (!response.ok) {
    const body = await response.text();
    console.error(JSON.stringify({ status: response.status,
      preparationFailure: body.includes("could not prepare"),
      authenticationFailure: /unauthorized|authentication|invalid_grant/i.test(body),
      modelUnavailable: /model.*not found|model.*unavailable/i.test(body),
    }));
  }
  assert(response.ok, `OpenCode API returned ${response.status}`);
  return response.status === 204 ? undefined : response.json();
}

function records(): any[] {
  return readFileSync(report, "utf8").trim().split("\n").filter(Boolean).map((line) => JSON.parse(line));
}

try {
  for (const path of [project, join(project, ".git"), join(config, "contextveil"), join(config, "opencode", "plugins"), join(data, "opencode")]) {
    mkdirSync(path, { recursive: true, mode: 0o700 });
  }
  // Let the host use its current credential database normally, without copying
  // credentials into fixtures. Only the temporary qualification sessions are removed.
  const database = process.env.CONTEXTVEIL_QUALIFY_DB ?? join(homedir(), ".local/share/opencode/opencode.db");
  const enrollment = join(config, "contextveil", "config.toml");
  writeFileSync(enrollment, 'version = 1\n[[secret]]\nsource = "env"\nname = "CONTEXTVEIL_QUALIFY_CANARY"\n', { mode: 0o600 });
  const plugin = (await Bun.file(resolve("assets/opencode/plugin.ts")).text())
    .replace('"__CONTEXTVEIL_BINARY__"', JSON.stringify(binary));
  writeFileSync(join(config, "opencode", "plugins", "contextveil.ts"), plugin);
  const observer = join(root, "observer");
  mkdirSync(observer);
  // Capture only booleans, never request bodies, headers, or credential values.
  writeFileSync(join(observer, "index.ts"), `
    import { appendFileSync } from "node:fs";
    export default { id: "contextveil-qualification", async setup(ctx) {
      await ctx.session.hook("http.request", async event => {
        const body = await event.request.clone().text();
        const leaked = body.includes(process.env.CONTEXTVEIL_QUALIFY_CANARY);
        const placeholder = body.includes("<SECRET:CONTEXTVEIL_QUALIFY_CANARY>");
        appendFileSync(process.env.CONTEXTVEIL_QUALIFY_REPORT,
          JSON.stringify({kind: event.kind, session: event.sessionID, leaked, placeholder}) + "\\n");
        if (leaked) throw new Error("Qualification blocked an unsanitized canary");
      });
    }};
  `);
  writeFileSync(report, "", { mode: 0o600 });
  writeFileSync(join(config, "opencode", "opencode.json"), JSON.stringify({
    model, update: "disable", share: "disabled", snapshots: false,
    agents: { title: { model }, compaction: { model } },
    providers: { openai: { settings: { transport: "http", compaction: { type: "summary" } } } },
    plugins: [observer],
  }));
  server = Bun.spawn(["opencode", "serve", "--hostname", "127.0.0.1", "--port", String(port), "--log-level", "none"], {
    cwd: project, stdin: "ignore", stdout: "pipe", stderr: "pipe",
    env: { ...process.env, XDG_CONFIG_HOME: config, XDG_DATA_HOME: data,
      XDG_STATE_HOME: join(root, "state"), XDG_CACHE_HOME: join(root, "cache"),
      OPENCODE_CONFIG_DIR: join(config, "opencode"), OPENCODE_CONFIG: undefined,
      OPENCODE_CONFIG_CONTENT: undefined, OPENCODE_DB: database,
      OPENCODE_PASSWORD: password, OPENCODE_USERNAME: "opencode",
      CONTEXTVEIL_QUALIFY_CANARY: canary, CONTEXTVEIL_QUALIFY_REPORT: report,
    },
  });
  const output = Promise.all([new Response(server.stdout).text(), new Response(server.stderr).text()]);
  stage = "start host";
  let ready = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    try { await api("/api/session"); ready = true; break; } catch {}
    if (server.exitCode !== null) break;
    await Bun.sleep(100);
  }
  assert(ready, "OpenCode did not become ready");
  stage = "create session";
  const session = (await api("/api/session", { location: { directory: project },
    model: { providerID: model.split("/")[0], id: model.slice(model.indexOf("/") + 1) },
  })).data;
  sessions.push(session.id);
  const wait = () => api(`/api/experimental/session/${session.id}/wait`, {});
  stage = "context and title";
  await api(`/api/session/${session.id}/prompt`, { text: `Qualification token: ${canary}. Reply OK only. Do not use tools.` });
  await wait();
  stage = "generate";
  await api(`/api/session/${session.id}/generate`, { prompt: "Reply OK only. Do not use tools." });
  stage = "resume";
  await api(`/api/session/${session.id}/prompt`, { text: "Reply OK again. Do not use tools." });
  await wait();
  const history = await api(`/api/session/${session.id}/context`);
  assert(JSON.stringify(history).includes(canary), "The original persisted history was unexpectedly rewritten");
  stage = "compaction";
  await api(`/api/session/${session.id}/compact`, {});
  await wait();
  const successful = records();
  assert(successful.every((record) => !record.leaked), "A request contained a canary");
  for (const kind of ["primary", "title", "generate", "compaction"]) {
    assert(successful.some((record) => record.kind === kind && record.placeholder), `${kind} lacks non-vacuous request evidence`);
  }
  stage = "malfunction";
  writeFileSync(enrollment, 'version = 1\n[[secret]]\nsource = "invalid"\n');
  const failed = (await api("/api/session", { location: { directory: project }, title: "Failure qualification" })).data;
  sessions.push(failed.id);
  await api(`/api/session/${failed.id}/prompt`, { text: "Reply OK only." });
  await api(`/api/experimental/session/${failed.id}/wait`, {});
  assert(!records().some((record) => record.session === failed.id), "A malfunction reached model dispatch");
  const outcome = (await api(`/api/session/${failed.id}`)).data.outcome;
  assert(outcome === "failed", "The malfunction did not fail the operation");
  for (const id of sessions) {
    const removed = await fetch(base + `/api/session/${id}`, { method: "DELETE", headers });
    assert(removed.ok, "A qualification session could not be removed");
  }
  sessions.length = 0;
  server.kill();
  await server.exited;
  const streams = await output;
  assert(streams.every((stream) => !stream.includes(canary)), "Host stdout or stderr contained a canary");
  console.log(JSON.stringify({ model, result: "passed", requestKinds: [...new Set(successful.map((r) => r.kind))],
    observedRequests: successful.length, malfunctionDispatches: 0,
    scope: "HTTP session requests; ordinary compaction only, not overflow/shrink recovery" }));
} catch (error) {
  // Never surface arbitrary host errors or response payloads.
  console.error(`OpenCode qualification failed during ${stage}: ${error instanceof Error && error.message.startsWith("OpenCode API returned") ? error.message : "required evidence was not established"}`);
  try { console.error(JSON.stringify({ observations: records() })); } catch {}
  process.exitCode = 1;
} finally {
  for (const id of sessions) {
    try {
      const removed = await fetch(base + `/api/session/${id}`, { method: "DELETE", headers, signal: AbortSignal.timeout(5000) });
      if (!removed.ok && removed.status !== 404) throw new Error();
    } catch {
      console.error("A temporary qualification session could not be removed; inspect OpenCode sessions.");
      process.exitCode = 1;
    }
  }
  if (server && server.exitCode === null) { server.kill(); await server.exited; }
  rmSync(root, { recursive: true, force: true });
}
