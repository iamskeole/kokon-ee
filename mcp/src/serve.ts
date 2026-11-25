// kokon-ee-mcp serve — Streamable HTTP mode: for HTTP MCP clients that cannot start a
// stdio subprocess (Open WebUI, the box's family chat). It speaks the Model Context
// Protocol over HTTP (the spec's Streamable HTTP transport: POST /mcp for requests,
// GET /mcp for the server's event stream, one session per client conversation) and
// drives the same Kokon service as the stdio mode.
//
// Placement on the box: a small sidecar container that shares the service's network
// namespace (docker compose network_mode: "container:kokon-ee"). Its 127.0.0.1 is the
// service's 127.0.0.1, so the service's token door (bound to loopback only) stays
// exactly as it is — the sidecar simply reaches it. The one door this mode opens is
// the listener on QLS_MCP_HTTP_PORT (39153 by default), guarded by a Bearer secret
// (QLS_MCP_HTTP_SECRET); on the box it is reachable from the docker network the
// service already sits on and nowhere else (nothing is published to the LAN). See
// docs/service-on-prod-box.md.

import crypto from "node:crypto";
import http from "node:http";
import { isInitializeRequest } from "@modelcontextprotocol/sdk/types.js";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import { makeServer } from "./server.js";
import { ServiceClient } from "./service.js";

const PORT = Number(process.env.QLS_MCP_HTTP_PORT ?? "39153");
const HOST = process.env.QLS_MCP_HTTP_HOST ?? "0.0.0.0";
const SECRET = (process.env.QLS_MCP_HTTP_SECRET ?? "").trim();

// A chat that goes quiet is dropped: sessions idle this long close (the service lease
// is per process, not per session, so closing a session drops MCP state only).
const IDLE_MS = 30 * 60_000;
const MAX_SESSIONS = 100;

interface Session {
  transport: StreamableHTTPServerTransport;
  open: number;
  lastActive: number;
}

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const parts: Buffer[] = [];
    req.on("data", (p: Buffer) => parts.push(p));
    req.on("end", () => resolve(Buffer.concat(parts).toString("utf8")));
    req.on("error", reject);
  });
}

function jsonrpcError(res: http.ServerResponse, status: number, code: number, message: string) {
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ jsonrpc: "2.0", error: { code, message }, id: null }));
}

function safeParse(s: string): unknown {
  if (!s) return undefined;
  try {
    return JSON.parse(s);
  } catch {
    return undefined;
  }
}

export async function runServe(): Promise<void> {
  if (!SECRET) {
    process.stderr.write("kokon-ee-mcp serve: QLS_MCP_HTTP_SECRET is required (a long random string the HTTP clients present as Bearer).\n");
    process.exit(1);
  }
  const client = new ServiceClient();
  const sessions = new Map<string, Session>();

  // A response counted as open keeps an SSE stream or a running call from reading as idle.
  const track = (s: Session, res: http.ServerResponse) => {
    if (!res.socket || res.destroyed) return;
    s.open += 1;
    res.on("close", () => {
      s.open -= 1;
      s.lastActive = Date.now();
    });
  };

  setInterval(() => {
    const cutoff = Date.now() - IDLE_MS;
    for (const s of sessions.values()) if (s.open === 0 && s.lastActive < cutoff) s.transport.close().catch(() => {});
  }, 60_000).unref();

  const authorized = (req: http.IncomingMessage) => {
    const h = Buffer.from(req.headers.authorization ?? "");
    const want = Buffer.from(`Bearer ${SECRET}`);
    return h.length === want.length && crypto.timingSafeEqual(h, want);
  };

  const handle = async (req: http.IncomingMessage, res: http.ServerResponse) => {
    if (!authorized(req)) {
      res.writeHead(401, { "Content-Type": "application/json", "WWW-Authenticate": "Bearer" });
      res.end(JSON.stringify({ error: "unauthorized: a Bearer secret is required (QLS_MCP_HTTP_SECRET)" }));
      return;
    }
    const sidHeader = req.headers["mcp-session-id"];
    const sid = Array.isArray(sidHeader) ? sidHeader[0] : sidHeader;

    if (req.method === "POST") {
      const body = safeParse(await readBody(req));
      const existing = sid ? sessions.get(sid) : undefined;
      if (existing) {
        existing.lastActive = Date.now();
        track(existing, res);
        await existing.transport.handleRequest(req, res, body);
        return;
      }
      if (!sid && isInitializeRequest(body)) {
        if (sessions.size >= MAX_SESSIONS) {
          jsonrpcError(res, 503, -32000, "Too many open sessions");
          return;
        }
        const transport = new StreamableHTTPServerTransport({
          sessionIdGenerator: () => crypto.randomUUID(),
          onsessioninitialized: (id) => {
            const s: Session = { transport, open: 0, lastActive: Date.now() };
            sessions.set(id, s);
            // Drop the session when the transport closes (client gone or session deleted).
            transport.onclose = () => {
              const cur = transport.sessionId;
              if (cur && sessions.get(cur) === s) sessions.delete(cur);
            };
          },
        });
        // Connect the transport to a fresh server BEFORE handling the request, so the
        // response flows back through the same transport (the SDK's documented pattern).
        await makeServer(client).connect(transport);
        await transport.handleRequest(req, res, body);
        return;
      }
      if (sid) {
        jsonrpcError(res, 404, -32001, "Session not found");
        return;
      }
      jsonrpcError(res, 400, -32000, "Bad request: no valid session ID provided");
      return;
    }

    if (req.method === "GET") {
      if (!sid) {
        res.writeHead(400, { "Content-Type": "text/plain" });
        res.end("Missing session ID");
        return;
      }
      const s = sessions.get(sid);
      if (!s) {
        jsonrpcError(res, 404, -32001, "Session not found");
        return;
      }
      track(s, res);
      await s.transport.handleRequest(req, res);
      return;
    }

    if (req.method === "DELETE") {
      const s = sid ? sessions.get(sid) : undefined;
      if (!s) {
        jsonrpcError(res, 404, -32001, "Session not found");
        return;
      }
      // The transport's DELETE handling closes the session; onclose removes it.
      await s.transport.handleRequest(req, res);
      return;
    }

    res.writeHead(405, { Allow: "GET, POST, DELETE" });
    res.end();
  };

  const httpServer = http.createServer((req, res) => {
    const path = new URL(req.url ?? "/", "http://localhost").pathname;
    // Liveness for the sidecar's healthcheck: says nothing, needs no secret.
    if (path === "/") {
      res.writeHead(200, { "Content-Type": "text/plain" });
      res.end(`kokon-ee-mcp serve — Streamable HTTP on /mcp (a Bearer secret is required)\n`);
      return;
    }
    if (path !== "/mcp") {
      res.writeHead(404, { "Content-Type": "text/plain" });
      res.end("not found\n");
      return;
    }
    void handle(req, res).catch((e) => {
      if (!res.headersSent) jsonrpcError(res, 500, -32603, "Internal server error");
      else res.end();
      process.stderr.write(`kokon-ee-mcp serve: ${e instanceof Error ? (e.stack ?? e.message) : String(e)}\n`);
    });
  });

  await new Promise<void>((resolve, reject) => {
    httpServer.once("error", reject);
    httpServer.listen(PORT, HOST, () => {
      httpServer.removeListener("error", reject);
      resolve();
    });
  });
  process.stderr.write(`kokon-ee-mcp serve: listening on http://${HOST}:${PORT}/mcp\n`);

  const shutdown = async () => {
    for (const s of sessions.values()) await s.transport.close().catch(() => {});
    await client.release();
    process.exit(0);
  };
  process.on("SIGINT", () => void shutdown());
  process.on("SIGTERM", () => void shutdown());
}
