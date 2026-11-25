# kokon-ee-mcp

MCP server for [Kokon EE](https://github.com/iamskeole/kokon-ee). It lets Claude Code, Cursor,
Claude Desktop and other MCP clients generate and edit images with the box's image model, running
offline on your own GPU. No API key, no cloud. Everything stays on your box.
Two entry points: **stdio** (an MCP client starts this process — the default) and **`serve`**
(the same server over Streamable HTTP, for HTTP clients such as Open WebUI; below).

> **Against the Kokon EE service** — the box deployment: a Docker image with the browser UI,
> pointed at the box's gateway of stock engines
> ([docs/fresh-box.md](https://github.com/iamskeole/kokon-ee/blob/main/docs/fresh-box.md)).
> The server connects through the service's own `service.json` (port + token): point
> `QLS_DATA_DIR` at the service's data folder (on the box, the compose's `data/` bind) and it
> picks the running service up. No separate configuration, nothing leaves `127.0.0.1`.

## Setup

Claude Code, on the box (the data folder beside the compose file):

```bash
claude mcp add --scope user kokon-ee \
  --env QLS_DATA_DIR=/home/<you>/ml/frontends/kokon-ee/data \
  -- npx -y kokon-ee-mcp
```

Any client that takes the standard JSON config (`.mcp.json`, Claude Desktop, Cursor...):

```json
{
  "mcpServers": {
    "kokon-ee": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "kokon-ee-mcp"],
      "env": { "QLS_DATA_DIR": "/home/<you>/ml/frontends/kokon-ee/data" }
    }
  }
}
```

Needs Node.js 22 or newer. Against a desktop install of
[Kokon](https://github.com/GabrieleLozupone/kokon) the same server works as before (it finds
the app's data folder, or starts the app's service if none is running; the installer also ships
it as a standalone executable, `kokon-mcp.exe` / `kokon-mcp`, next to the service).

Then ask: *"Generate a hero image for this README and save it in docs/"* or *"Remove the
background clutter from screenshot.png"*. On the box the engines load through the gateway on
the first call; the first call of a session takes as long as the engine boot.

## Serve mode (HTTP)

`kokon-ee-mcp serve` runs the same server over the MCP **Streamable HTTP** transport:
`POST /mcp` for requests, `GET /mcp` for the server's event stream, one session per client
conversation. It is for MCP clients that speak HTTP instead of starting a subprocess —
Open WebUI's tool servers, in the first place.

```bash
QLS_DATA_DIR=/path/to/service/data QLS_MCP_HTTP_SECRET=<a-long-random-string> \
  node kokon-ee-mcp.cjs serve        # listens on 0.0.0.0:39153/mcp by default
```

- `QLS_MCP_HTTP_SECRET` is **required**: every request must present it as a Bearer token.
  It is the door; generate one (`openssl rand -hex 32`) and keep it in the sidecar's env.
- `QLS_MCP_HTTP_PORT` (default `39153`) and `QLS_MCP_HTTP_HOST` (default `0.0.0.0`) place
  the listener. On the box the server runs as a small sidecar container that **shares the
  service's network namespace** (compose `network_mode: "container:kokon-ee"`): its
  `127.0.0.1` is the service's, so the service's loopback token door is reachable and stays
  bound to loopback, and the only new door is this listener, on the docker network and
  nowhere else (see [docs/service-on-prod-box.md](../docs/service-on-prod-box.md)).
- **Default model** (`QLS_MCP_MODEL`): the model this server's jobs run on, named by its
  gateway id or its picker label (e.g. `kokon-qwen-image-2.1-full`). The service is shared
  with the browser UI, and its active model follows whoever switched it last, so before each
  job the server aligns the service to the configured value (the same switch the UI's picker
  sends — a config write; the engine loads on the job itself). The browser is free to wander;
  the next chat job re-aligns. Unset: follow whatever model the service has active.

In Open WebUI: add the sidecar as an MCP tool server at `http://kokon-ee:39153/mcp`
(`kokon-ee` is the service container the sidecar shares its network with) with the same
secret as its header, and assign it to the chat model.

## Tools

Prompts are used exactly as your assistant writes them: the service's own prompt writer
(Prompt+) runs only in the UI's modes, so the assistant writes the full description. The tool
descriptions tell it how.

| Tool | What it does |
| --- | --- |
| `generate_image` | Text to image, optionally with up to 10 reference images (a person, a product, a style) in a new scene. Size chosen automatically, or set width/height or aspect ratio. |
| `edit_image` | Edit an image with an instruction. Keeps its size; never overwrites the input. |
| `edit_images` | Edit image 1 with the help of up to 9 more images ("image 1", "image 2"...). |
| `inpaint_image` | Change only a masked area: edit, remove, replace or add something. |
| `outpaint_image` | Extend an image beyond its borders. |
| `upscale_image` | Enlarge an image (default 2x, up to 8192 px). |
| `get_image_info` | Size, type and, for Kokon images, the prompt, seed and settings. |
| `get_generator_status` | Engine state, GPU, VRAM and job queue. |

## Behavior and safety

- Every result returns the absolute path of the saved file, its size, seed and metadata.
- Reads and writes are limited to the project folder (`CLAUDE_PROJECT_DIR` or the working
  directory), the service's output folder and folders listed in `QLS_MCP_ALLOWED_DIRS`
  (separated by semicolons). Existing files are never overwritten.
- Images are not sent back to the model by default. Pass `include_preview: true` for a small JPEG.
- Progress is reported through MCP progress notifications.
- In serve mode the HTTP door needs the Bearer secret, and sessions idle for 30 minutes
close (the server process keeps one service lease no matter how many sessions it holds).

## License

AGPL-3.0-only (a derivative of Kokon's `kokon-mcp`; copyright in the
[NOTICE](https://github.com/iamskeole/kokon-ee/blob/main/NOTICE)). The Qwen-Image 2.1 model
weights are under the Qwen Research License (non-commercial). Source:
[github.com/iamskeole/kokon-ee](https://github.com/iamskeole/kokon-ee/tree/main/mcp).
