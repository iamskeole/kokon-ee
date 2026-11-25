// kokon-ee-mcp — MCP server for Kokon EE. Two entry points: stdio (the default — an MCP
// client such as Claude Code or Cursor starts this process) and `kokon-ee-mcp serve`
// (Streamable HTTP, for HTTP clients such as Open WebUI; see serve.ts). It uses the same
// local service and generation engine as the box's service. Works when the browser UI is
// closed: the service and the inference server are started on demand. Everything stays on
// 127.0.0.1.

import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { ServiceClient } from "./service.js";
import { makeServer } from "./server.js";
import { runServe } from "./serve.js";
import { helpText } from "./help.js";
import pkg from "../package.json" with { type: "json" };

// Bundled in by esbuild: the one place to bump is package.json (see docs/releasing.md).
const VERSION = pkg.version;

const client = new ServiceClient();

async function main() {
  if (process.argv.includes("--version")) {
    process.stdout.write(`kokon-ee-mcp ${VERSION}\n`);
    return;
  }
  if (process.argv.includes("--help") || process.argv.includes("-h")) {
    process.stdout.write(helpText(VERSION));
    return;
  }
  if (process.argv[2] === "serve") {
    // Never returns on success: the HTTP server runs until SIGINT/SIGTERM (runServe
    // exits non-zero when QLS_MCP_HTTP_SECRET is missing).
    await runServe();
    return;
  }
  const server = makeServer(client);
  const transport = new StdioServerTransport();
  await server.connect(transport);
  const shutdown = async () => {
    await client.release();
    process.exit(0);
  };
  process.stdin.on("close", shutdown);
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

main().catch((e) => {
  process.stderr.write(`kokon-ee-mcp failed: ${e instanceof Error ? e.stack : String(e)}\n`);
  process.exit(1);
});
