// QLS_MCP_MODEL (the MCP's default model): with a real service and no GPU, prove the
// alignment the MCP does before each job — it reads the service config and posts the
// matching picker entry through the same /models/switch the UI sends.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { ServiceClient } from "../src/service.js";

const root = path.resolve(__dirname, "..", "..");
const serviceExe = process.platform === "win32" ? "kokon-service.exe" : "kokon-service";
const serviceBuild = ["debug", "release"]
  .map((b) => path.join(root, "target", b, serviceExe))
  .filter((p) => fs.existsSync(p))
  .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)[0];

const TURBO_ID = "kokon-qwen-image-2.1-turbo";
const FULL_ID = "kokon-qwen-image-2.1-full";
// A plausible second pipeline: the same gateway, a different DiT + its prompt writer.
const models = [
  { label: "Turbo", model: TURBO_ID, modelName: "qwen-image-2.1-turbo-Q4_K_M.gguf", sample: {}, vlm: null, vlmVision: null, promptBundle: "" },
  { label: "Full", model: FULL_ID, modelName: "qwen-image-2.1-Q4_K_M.gguf", sample: { steps: 24 }, vlm: { entrypoint: "http://127.0.0.1:1/v", model: "pup", apiKey: "" }, vlmVision: null, promptBundle: "qwen21" },
];

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "qls-mcp-model-"));
const env = {
  ...process.env,
  QLS_DATA_DIR: path.join(tmp, "data"),
  QLS_CONFIG_DIR: path.join(tmp, "config"),
  QLS_SERVICE_EXE: serviceBuild ?? path.join(root, "target", "debug", serviceExe),
} as Record<string, string>;
delete env.CLAUDE_PROJECT_DIR;
delete env.QLS_MCP_MODEL;
fs.mkdirSync(env.QLS_CONFIG_DIR, { recursive: true });
fs.writeFileSync(
  path.join(env.QLS_CONFIG_DIR, "config.json"),
  JSON.stringify({
    servicePort: 39184,
    serverPort: 39183,
    outputDir: path.join(tmp, "out"),
    externalEngine: { gateway: "http://127.0.0.1:39992/upstream", model: TURBO_ID, modelName: models[0].modelName, sample: {} },
    models,
  }),
);

const client = new ServiceClient();

const activeModel = async () => (await client.call<{ externalEngine: { model: string } | null }>("GET", "/config")).externalEngine?.model ?? null;

// The client runs inside this process (unlike the protocol test, which spawns the server):
// the env the service sees is process.env.
const savedEnv: Record<string, string | undefined> = {};
beforeAll(async () => {
  for (const k of ["QLS_DATA_DIR", "QLS_CONFIG_DIR", "QLS_SERVICE_EXE"]) savedEnv[k] = process.env[k];
  for (const k of ["QLS_DATA_DIR", "QLS_CONFIG_DIR", "QLS_SERVICE_EXE"]) process.env[k] = env[k];
  await client.ensure();
}, 30_000);

afterAll(async () => {
  await client.release();
  for (const k of Object.keys(savedEnv)) {
    if (savedEnv[k] === undefined) delete process.env[k];
    else process.env[k] = savedEnv[k];
  }
  try {
    const info = JSON.parse(fs.readFileSync(path.join(env.QLS_DATA_DIR, "service.json"), "utf8"));
    await fetch(`http://127.0.0.1:${info.port}/api/v1/shutdown`, { method: "POST", headers: { Authorization: `Bearer ${info.token}` } });
  } catch {
    /* not started */
  }
});

describe("alignModel (QLS_MCP_MODEL)", () => {
  it("is a no-op when unset: the service keeps its active model", async () => {
    expect(env.QLS_MCP_MODEL).toBeUndefined();
    await client.alignModel();
    expect(await activeModel()).toBe(TURBO_ID);
  });

  it("aligns by gateway model id through the picker entry", async () => {
    process.env.QLS_MCP_MODEL = FULL_ID;
    try {
      await client.alignModel();
      expect(await activeModel()).toBe(FULL_ID);
      const cfg = await client.call<{ externalEngine: Record<string, unknown>; }>("GET", "/config");
      // The posted entry is the complete pipeline spec (sample overrides, prompt writer, bundle).
      expect(cfg.externalEngine.sample).toEqual({ steps: 24 });
      expect(cfg.externalEngine.vlm).toEqual(models[1].vlm);
      expect(cfg.externalEngine.promptBundle).toBe("qwen21");
    } finally {
      delete process.env.QLS_MCP_MODEL;
    }
  });

  it("aligns by picker label too", async () => {
    process.env.QLS_MCP_MODEL = "Turbo";
    try {
      await client.alignModel();
      expect(await activeModel()).toBe(TURBO_ID);
    } finally {
      delete process.env.QLS_MCP_MODEL;
    }
  });

  it("an unknown name leaves the active model alone", async () => {
    process.env.QLS_MCP_MODEL = "not-a-model-on-this-box";
    try {
      await client.alignModel();
      expect(await activeModel()).toBe(TURBO_ID);
    } finally {
      delete process.env.QLS_MCP_MODEL;
    }
  });

  it("an already-active model does not re-switch", async () => {
    process.env.QLS_MCP_MODEL = TURBO_ID;
    try {
      await client.alignModel();
      expect(await activeModel()).toBe(TURBO_ID);
    } finally {
      delete process.env.QLS_MCP_MODEL;
    }
  });
});
