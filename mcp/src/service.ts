// Client for the box's kokon-service. Finds it via service.json, starts the bare binary if
// needed (the service adopts the engine through the box's gateway; it never spawns one),
// and holds an "mcp" lease while alive.

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const WIN = process.platform === "win32";
const SERVICE_EXE = WIN ? "kokon-service.exe" : "kokon-service";

// The shapes of the service config fields the MCP reads (camelCase on the wire;
// crates/qls-core/src/config.rs).
interface VlmEndpointShape {
  entrypoint: string;
  model: string;
  apiKey?: string;
}
interface ExternalEngineShape {
  gateway: string;
  apiKey?: string;
  model: string;
  modelName: string;
  sample?: Record<string, unknown>;
  vlm?: VlmEndpointShape | null;
  vlmVision?: VlmEndpointShape | null;
  promptBundle?: string;
}
interface ExternalModelShape {
  label: string;
  sub?: string | null;
  model: string;
  modelName: string;
  sample?: Record<string, unknown>;
  gateway?: string | null;
  vlm?: VlmEndpointShape | null;
  vlmVision?: VlmEndpointShape | null;
  promptBundle?: string;
}
interface ConfigShape {
  outputDir?: string;
  externalEngine?: ExternalEngineShape | null;
  models?: ExternalModelShape[];
}

export interface ServiceInfo {
  port: number;
  token: string;
  pid: number;
  version: string;
}

export class ServiceError extends Error {
  constructor(
    message: string,
    public code = "service_error",
    public suggestion?: string,
    public detail?: string,
  ) {
    super(message);
  }
}

/** The service's `dirs::data_local_dir()`: %LOCALAPPDATA%, ~/Library/Application Support, or
 * $XDG_DATA_HOME (default ~/.local/share) on Linux. */
export function localDataBase(platform: NodeJS.Platform, env: NodeJS.ProcessEnv, home: string): string {
  if (platform === "win32") return env.LOCALAPPDATA ?? path.join(env.USERPROFILE ?? "C:\\", "AppData", "Local");
  if (platform === "darwin") return path.join(home, "Library", "Application Support");
  return env.XDG_DATA_HOME && path.isAbsolute(env.XDG_DATA_HOME) ? env.XDG_DATA_HOME : path.join(home, ".local", "share");
}

export function dataDir(): string {
  if (process.env.QLS_DATA_DIR) return process.env.QLS_DATA_DIR;
  const base = localDataBase(process.platform, process.env, os.homedir());
  // The service moves the folder of the app's old name (QwenLocalStudio) to KokonData on its
  // first start; until then the old one is read.
  const [next, old] = [path.join(base, "KokonData"), path.join(base, "QwenLocalStudio")];
  return !fs.existsSync(next) && fs.existsSync(old) ? old : next;
}

function readInfo(): ServiceInfo | null {
  try {
    return JSON.parse(fs.readFileSync(path.join(dataDir(), "service.json"), "utf8"));
  } catch {
    return null;
  }
}

async function ping(port: number): Promise<boolean> {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/api/v1/ping`, { signal: AbortSignal.timeout(1500) });
    return r.ok;
  } catch {
    return false;
  }
}

export function findServiceExe(): string | null {
  const installed = WIN
    ? [
        path.join(process.env.LOCALAPPDATA ?? "", "Kokon", SERVICE_EXE),
        path.join(process.env.ProgramFiles ?? "C:\\Program Files", "Kokon", SERVICE_EXE),
      ]
    : [path.join("/Applications", "Kokon.app", "Contents", "MacOS", SERVICE_EXE), path.join(os.homedir(), "Applications", "Kokon.app", "Contents", "MacOS", SERVICE_EXE)];
  const candidates = [
    process.env.QLS_SERVICE_EXE,
    // Next to kokon-mcp
    path.join(path.dirname(process.execPath), SERVICE_EXE),
    ...installed,
    // The box image and a PATH install
    "/usr/local/bin/" + SERVICE_EXE,
    "/usr/bin/" + SERVICE_EXE,
    // Development checkout (mcp/build -> target/...)
    path.resolve(__dirname, "..", "..", "target", "release", SERVICE_EXE),
    path.resolve(__dirname, "..", "..", "target", "debug", SERVICE_EXE),
  ];
  for (const c of candidates) {
    if (c && fs.existsSync(c)) return c;
  }
  return null;
}

export class ServiceClient {
  private info: ServiceInfo | null = null;
  private starting: Promise<ServiceInfo> | null = null;
  private heartbeat: NodeJS.Timeout | null = null;
  readonly clientId = `mcp-${process.pid}`;

  async ensure(): Promise<ServiceInfo> {
    if (this.info && (await ping(this.info.port))) return this.info;
    if (!this.starting) {
      this.starting = this.start().finally(() => (this.starting = null));
    }
    this.info = await this.starting;
    this.startHeartbeat();
    return this.info;
  }

  private async start(): Promise<ServiceInfo> {
    const existing = readInfo();
    if (existing && (await ping(existing.port))) return existing;
    const exe = findServiceExe();
    if (!exe) {
      throw new ServiceError(
        `The Kokon service is not running and ${SERVICE_EXE} was not found.`,
        "service_unavailable",
        `Start the box (docker compose up -d, or kokon-service --persistent), or set QLS_SERVICE_EXE to the path of ${SERVICE_EXE} if it lives in an unusual place.`,
      );
    }
    // Detached: the service outlives this MCP process and exits on its own when idle.
    const child = spawn(exe, [], { detached: true, stdio: "ignore", windowsHide: true });
    child.unref();
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, 300));
      const i = readInfo();
      if (i && (await ping(i.port))) return i;
    }
    throw new ServiceError("The Kokon service did not start.", "service_unavailable", `See ${path.join(dataDir(), "logs", "service.log")}`);
  }

  private startHeartbeat() {
    if (this.heartbeat) return;
    const beat = () => this.call("POST", "/session/heartbeat", { client_id: this.clientId, kind: "mcp", pid: process.pid }).catch(() => {});
    beat();
    this.heartbeat = setInterval(beat, 15_000);
    this.heartbeat.unref();
  }

  async release() {
    if (this.heartbeat) clearInterval(this.heartbeat);
    if (!this.info) return;
    try {
      await fetch(`http://127.0.0.1:${this.info.port}/api/v1/session/release`, {
        method: "POST",
        headers: { Authorization: `Bearer ${this.info.token}`, "Content-Type": "application/json" },
        body: JSON.stringify({ client_id: this.clientId }),
        signal: AbortSignal.timeout(2000),
      });
    } catch {
      /* service already gone */
    }
  }

  /**
   * The MCP's default model (QLS_MCP_MODEL): the model this MCP's jobs run on. The
   * service is shared with the browser UI, and its active model follows whoever
   * switched it last — so before each job the MCP aligns the service to the
   * configured value: it reads the service config (GET /config already carries the
   * picker's `models` list, each entry a complete pipeline spec) and posts the
   * matching entry through the same POST /models/switch the UI picker sends (a
   * config write — the engine loads on the job itself, so aligning is free).
   * Contract: a chat conversation is pinned to the configured model; the browser
   * is free to wander; the next chat job re-aligns. Unset (or the service has no
   * external engine, or the name matches no picker entry) = follow whatever model
   * the service has active, as before.
   */
  async alignModel(): Promise<void> {
    const target = (process.env.QLS_MCP_MODEL ?? "").trim();
    if (!target) return;
    let cfg: ConfigShape;
    try {
      cfg = await this.call<ConfigShape>("GET", "/config");
    } catch {
      return; // the service will be reached again by the job itself, with a real error
    }
    const active = cfg.externalEngine;
    if (!active || active.model === target) return;
    const entries = cfg.models ?? [];
    const entry = entries.find((m) => m.model === target) ?? entries.find((m) => m.label === target);
    if (!entry) return; // unknown to this service: leave it, and the status tool tells the truth
    const req: Record<string, unknown> = {
      // The entry may carry its own gateway; otherwise keep the active engine's.
      // The key is engine-level, not per model: keep the active engine's.
      gateway: entry.gateway ?? active.gateway,
      apiKey: active.apiKey ?? "",
      model: entry.model,
      modelName: entry.modelName,
      sample: entry.sample ?? {},
    };
    if (entry.vlm) req.vlm = entry.vlm;
    if (entry.vlmVision) req.vlmVision = entry.vlmVision;
    if (entry.promptBundle) req.promptBundle = entry.promptBundle;
    await this.call("POST", "/models/switch", req);
  }

  async call<T = unknown>(method: string, route: string, body?: unknown, retry = true): Promise<T> {
    const info = await this.ensure();
    let res: Response;
    try {
      res = await fetch(`http://127.0.0.1:${info.port}/api/v1${route}`, {
        method,
        headers: { Authorization: `Bearer ${info.token}`, ...(body !== undefined ? { "Content-Type": "application/json" } : {}) },
        body: body !== undefined ? JSON.stringify(body) : undefined,
      });
    } catch (e) {
      if (retry) {
        this.info = null;
        return this.call(method, route, body, false);
      }
      throw new ServiceError("The Kokon service is not reachable.", "service_unavailable", undefined, String(e));
    }
    const text = await res.text();
    const data = text ? JSON.parse(text) : null;
    if (!res.ok) {
      const err = data?.error ?? {};
      throw new ServiceError(err.message ?? `HTTP ${res.status}`, err.code ?? "error", err.suggestion, err.detail);
    }
    return data as T;
  }
}
