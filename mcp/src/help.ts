// Text printed by `kokon-ee-mcp --help`. Kept apart from index.ts so it can be tested without
// starting the server. The environment variables listed here are read in paths.ts,
// service.ts and index.ts; update this text when one is added or renamed.

export function helpText(version: string): string {
  return `kokon-ee-mcp ${version}

MCP server for Kokon EE. Two entry points:
  - stdio (the default): an MCP client (Claude Code, Cursor, Claude Desktop...) starts
    this process; running it by hand just waits for input.
  - "serve": the same server over Streamable HTTP (one session per client), for HTTP
    clients such as Open WebUI. Needs QLS_MCP_HTTP_SECRET; on the box it runs as a
    small sidecar that shares the service's network namespace.
It talks to the running Kokon EE service (the box: docker image + browser UI) through
the service's own service.json (port + token); against a bare-binary install it starts
the local engine on demand instead.

Usage:
  kokon-ee-mcp             start the MCP server on stdio
  kokon-ee-mcp serve       start the MCP server over Streamable HTTP (needs a secret)
  kokon-ee-mcp --version   print the version and exit
  kokon-ee-mcp --help      print this text and exit

Register it with Claude Code (QLS_DATA_DIR points at the service's data folder):
  claude mcp add --scope user kokon-ee --env QLS_DATA_DIR=/path/to/service/data -- npx -y kokon-ee-mcp

Environment variables:
  QLS_MCP_ALLOWED_DIRS  extra folders the tools may read and write, separated by semicolons
  QLS_SERVICE_EXE       path to kokon-service, if Kokon is installed in an unusual place
  QLS_DATA_DIR          the service's data folder (holds service.json: port + token;
                        default: the service's data folder)
  QLS_MCP_MODEL         the model this MCP's jobs run on: a gateway model id or a picker
                        label from the service's model list. Before each job the server
                        aligns the service to it (the browser UI is free to wander; the
                        next chat job re-aligns). Unset: follow the service's active model.
  QLS_MCP_HTTP_PORT     port "serve" listens on (default 39153)
  QLS_MCP_HTTP_HOST     interface "serve" binds (default 0.0.0.0; on the box the sidecar
                        sits on the docker network, not on the internet)
  QLS_MCP_HTTP_SECRET   required by "serve": the Bearer secret HTTP clients present
  CLAUDE_PROJECT_DIR    project folder the tools may use (set by Claude Code; default: the
                        working directory)
`;
}
