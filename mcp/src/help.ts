// Text printed by `kokon-ee-mcp --help`. Kept apart from index.ts so it can be tested without
// starting the server. The environment variables listed here are read in paths.ts,
// service.ts and index.ts; update this text when one is added or renamed.

export function helpText(version: string): string {
  return `kokon-ee-mcp ${version}

MCP server for Kokon EE. It speaks the Model Context Protocol over stdio, so an MCP client
(Claude Code, Cursor, Claude Desktop...) starts it; running it by hand just waits for input.
It talks to the running Kokon EE service (the box: docker image + browser UI) through the
service's own service.json (port + token); against a bare-binary install it starts the
local engine on demand instead.

Usage:
  kokon-ee-mcp             start the MCP server on stdio
  kokon-ee-mcp --version   print the version and exit
  kokon-ee-mcp --help      print this text and exit

Register it with Claude Code (QLS_DATA_DIR points at the service's data folder):
  claude mcp add --scope user kokon-ee --env QLS_DATA_DIR=/path/to/service/data -- npx -y kokon-ee-mcp

Environment variables:
  QLS_MCP_ALLOWED_DIRS  extra folders the tools may read and write, separated by semicolons
  QLS_SERVICE_EXE       path to kokon-service, if Kokon is installed in an unusual place
  QLS_DATA_DIR          the service's data folder (holds service.json: port + token;
                        default: the service's data folder)
  CLAUDE_PROJECT_DIR    project folder the tools may use (set by Claude Code; default: the
                        working directory)
`;
}
