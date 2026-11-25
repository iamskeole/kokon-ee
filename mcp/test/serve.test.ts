// The serve entry point (Streamable HTTP): the protocol over HTTP against a real service
// and no GPU — the same coverage as protocol.test.ts, plus the door's own rules (the
// Bearer secret, the sessions) and the QLS_MCP_MODEL alignment seen end to end.
import { spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

const root = path.resolve(__dirname, "..", "..");
const bundle = path.join(root, "mcp", "build", "kokon-ee-mcp.cjs");
const serviceExe = process.platform === "win32" ? "kokon-service.exe" : "kokon-service";
const serviceBuild = ["debug", "release"]
  .map((b) => path.join(root, "target", b, serviceExe))
  .filter((p) => fs.existsSync(p))
  .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)[0];

const TURBO_ID = "kokon-qwen-image-2.1-turbo";
const FULL_ID = "kokon-qwen-image-2.1-full";
const SECRET = "serve-test-secret-0123456789abcdef";
const PORT = 39984;
const URL = `http://127.0.0.1:${PORT}/mcp`;

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "qls-mcp-serve-"));
const proj = path.join(tmp, "project");
fs.mkdirSync(proj);
const env = {
  ...process.env,
  QLS_DATA_DIR: path.join(tmp, "data"),
  QLS_CONFIG_DIR: path.join(tmp, "config"),
  QLS_SERVICE_EXE: serviceBuild ?? path.join(root, "target", "debug", serviceExe),
  QLS_MCP_HTTP_PORT: String(PORT),
  QLS_MCP_HTTP_SECRET: SECRET,
  // The MCP's default model: its first job must align the service to it (D-039-7).
  QLS_MCP_MODEL: FULL_ID,
} as Record<string, string>;
delete env.CLAUDE_PROJECT_DIR;
fs.mkdirSync(env.QLS_CONFIG_DIR, { recursive: true });
fs.writeFileSync(
  path.join(env.QLS_CONFIG_DIR, "config.json"),
  JSON.stringify({
    servicePort: 39187,
    serverPort: 39186,
    outputDir: path.join(tmp, "out"),
    externalEngine: {
      gateway: "http://127.0.0.1:39993/upstream",
      model: TURBO_ID,
      modelName: "qwen-image-2.1-turbo-Q4_K_M.gguf",
      sample: {},
    },
    models: [
      { label: "Turbo", model: TURBO_ID, modelName: "qwen-image-2.1-turbo-Q4_K_M.gguf", sample: {}, vlm: null, vlmVision: null, promptBundle: "" },
      { label: "Full", model: FULL_ID, modelName: "qwen-image-2.1-Q4_K_M.gguf", sample: { steps: 24 }, vlm: null, vlmVision: null, promptBundle: "" },
    ],
  }),
);

// One JSON-RPC exchange over the HTTP door. Answers come back as plain JSON or as an SSE
// stream of `data:` lines; either way the JSON-RPC response is the one whose id matches.
async function rpc(
  body: { id?: number; method: string; params?: unknown } | unknown[],
  opts: { sid?: string; auth?: string } = {},
): Promise<{ status: number; session: string | null; json: any }> {
  // The JSON-RPC 2.0 envelope every request must carry (the transport's schema checks it).
  const msg = Array.isArray(body) ? body.map((b) => ({ jsonrpc: "2.0", ...b })) : { jsonrpc: "2.0", ...body };
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    // The Streamable HTTP spec: the client must accept both answer types.
    Accept: "application/json, text/event-stream",
  };
  if (opts.sid) headers["mcp-session-id"] = opts.sid;
  if (opts.auth) headers.Authorization = opts.auth;
  const res = await fetch(URL, { method: "POST", headers, body: JSON.stringify(msg) });
  const session = res.headers.get("mcp-session-id");
  const text = await res.text();
  const ct = res.headers.get("content-type") ?? "";
  let json: any = null;
  if (ct.includes("application/json")) {
    try {
      json = JSON.parse(text);
    } catch {
      /* body is not JSON (e.g. a plain-text error) */
    }
  } else {
    const want = Array.isArray(msg) ? new Set(msg.map((m) => m.id)) : new Set([msg.id]);
    for (const line of text.split("\n")) {
      if (!line.startsWith("data:")) continue;
      try {
        const m = JSON.parse(line.slice(5).trim());
        if (m && want.has(m.id)) json = m; // first match wins
      } catch {
        /* not a message line */
      }
    }
  }
  return { status: res.status, session, json };
}

let server: ChildProcess | null = null;

beforeAll(async () => {
  // The door comes up; the service itself is started by the server on its first call.
  server = spawn(process.execPath, [bundle, "serve"], { cwd: proj, env, stdio: ["ignore", "pipe", "pipe"] });
  server.stdout!.on("data", () => {});
  server.stderr!.on("data", () => {});
  const deadline = Date.now() + 10_000;
  for (;;) {
    const r = await fetch(`http://127.0.0.1:${PORT}/`).catch(() => null);
    if (r) break;
    if (Date.now() > deadline) throw new Error("the serve door never came up");
    await new Promise((r2) => setTimeout(r2, 200));
  }
}, 15_000);

afterAll(async () => {
  server?.kill("SIGTERM");
  await new Promise((r) => setTimeout(r, 500));
  server?.kill("SIGKILL");
  try {
    const info = JSON.parse(fs.readFileSync(path.join(env.QLS_DATA_DIR, "service.json"), "utf8"));
    await fetch(`http://127.0.0.1:${info.port}/api/v1/shutdown`, { method: "POST", headers: { Authorization: `Bearer ${info.token}` } });
  } catch {
    /* not started */
  }
});

describe("the serve door", () => {
  it("answers the healthcheck without a secret", async () => {
    const r = await fetch(`http://127.0.0.1:${PORT}/`);
    expect(r.status).toBe(200);
    expect(await r.text()).toContain("kokon-ee-mcp serve");
  });

  it("rejects requests without the secret and with the wrong one", async () => {
    const noAuth = await rpc({ id: 1, method: "initialize", params: {} });
    expect(noAuth.status).toBe(401);
    const wrong = await rpc({ id: 1, method: "initialize", params: {} }, { auth: "Bearer no" });
    expect(wrong.status).toBe(401);
  });

  it("starts a session with the secret (initialize)", async () => {
    const r = await rpc(
      { id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "qls-serve-test", version: "1" } } },
      { auth: `Bearer ${SECRET}` },
    );
    expect(r.status).toBe(200);
    expect(r.json?.result?.serverInfo?.name).toBe("kokon");
    expect(r.session).toBeTruthy();
    expect(r.json?.result?.capabilities?.tools).toBeTruthy();
  });

  it("lists the eight tools inside a session", async () => {
    const init = await rpc(
      { id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "qls-serve-test", version: "1" } } },
      { auth: `Bearer ${SECRET}` },
    );
    const r = await rpc({ id: 2, method: "tools/list", params: {} }, { sid: init.session!, auth: `Bearer ${SECRET}` });
    expect(r.status).toBe(200);
    expect(r.json?.result?.tools?.map((t: any) => t.name).sort()).toEqual([
      "edit_image",
      "edit_images",
      "generate_image",
      "get_generator_status",
      "get_image_info",
      "inpaint_image",
      "outpaint_image",
      "upscale_image",
    ]);
  });

  it("runs a read-only tool over HTTP (the service starts on demand)", async () => {
    const init = await rpc(
      { id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "qls-serve-test", version: "1" } } },
      { auth: `Bearer ${SECRET}` },
    );
    const r = await rpc({ id: 2, method: "tools/call", params: { name: "get_generator_status", arguments: {} } }, { sid: init.session!, auth: `Bearer ${SECRET}` });
    expect(r.status).toBe(200);
    const res = r.json?.result;
    expect(res?.isError).toBeFalsy();
    const structured = JSON.parse(res.content.find((c: any) => c.type === "text").text);
    expect(typeof structured.server_running).toBe("boolean");
  }, 45_000);

  it("a job aligns the service to QLS_MCP_MODEL before running (D-039-7)", async () => {
    const init = await rpc(
      { id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "qls-serve-test", version: "1" } } },
      { auth: `Bearer ${SECRET}` },
    );
    // No engine behind the (absent) gateway: the job fails, but only after the alignment.
    const r = await rpc({ id: 2, method: "tools/call", params: { name: "generate_image", arguments: { prompt: "A red bicycle in a sunlit field." } } }, { sid: init.session!, auth: `Bearer ${SECRET}` });
    expect(r.status).toBe(200);
    expect(r.json?.result?.isError).toBe(true); // the engine is absent on purpose
    // The service's active model is now the one the MCP is pinned to.
    const info = JSON.parse(fs.readFileSync(path.join(env.QLS_DATA_DIR, "service.json"), "utf8"));
    const cfg = await (
      await fetch(`http://127.0.0.1:${info.port}/api/v1/config`, { headers: { Authorization: `Bearer ${info.token}` } })
    ).json();
    expect(cfg.externalEngine.model).toBe(FULL_ID);
  }, 60_000);

  it("a second client gets its own independent session", async () => {
    const a = await rpc(
      { id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "a", version: "1" } } },
      { auth: `Bearer ${SECRET}` },
    );
    const b = await rpc(
      { id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "b", version: "1" } } },
      { auth: `Bearer ${SECRET}` },
    );
    expect(a.session).toBeTruthy();
    expect(b.session).toBeTruthy();
    expect(a.session).not.toBe(b.session);
    for (const s of [a.session, b.session]) {
      const r = await rpc({ id: 2, method: "tools/list", params: {} }, { sid: s!, auth: `Bearer ${SECRET}` });
      expect(r.status).toBe(200);
      expect(r.json?.result?.tools?.length).toBe(8);
    }
  });

  it("rejects an unknown session and a sessionless non-initialize", async () => {
    const unknown = await rpc({ id: 2, method: "tools/list", params: {} }, { sid: "no-such-session", auth: `Bearer ${SECRET}` });
    expect(unknown.status).toBe(404);
    const bare = await rpc({ id: 2, method: "tools/list", params: {} }, { auth: `Bearer ${SECRET}` });
    expect(bare.status).toBe(400);
  });
});

// Needs `npm run build` first, like the protocol test; skipped when the bundle is missing.
describe.skipIf(!fs.existsSync(bundle))("serve process rules", () => {
  it("refuses to start without a secret (and exits non-zero)", async () => {
    const noSecret = { ...env };
    delete noSecret.QLS_MCP_HTTP_SECRET;
    const p = spawn(process.execPath, [bundle, "serve"], { env: noSecret, stdio: ["ignore", "pipe", "pipe"] });
    let err = "";
    p.stderr!.on("data", (d) => (err += d));
    const code = await new Promise<number | null>((resolve) => p.on("exit", (c) => resolve(c)));
    expect(code).toBe(1);
    expect(err).toContain("QLS_MCP_HTTP_SECRET");
  }, 15_000);
});
