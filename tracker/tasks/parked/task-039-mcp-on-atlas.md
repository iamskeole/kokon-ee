# Task-039 (PARKED — owner-triggered) — MCP on atlas: the family's OWUI drives Kokon through the advertised MCP, zero custom code

**Created:** 2026-11-23 — the owner asked how to wire Kokon as an MCP
source into Open WebUI on the family box now that both boxes run the
service. **Do not start until the owner triggers it** — one owner
prerequisite is outstanding (npm account / publish decision, D-039-3).
When it starts: move to `todo/`.

## Purpose

The MCP server is Kokon's *advertised* way for agents to drive the
service (README, AGENTS.md, `mcp/`). On the dev box (apollo) it already
works as-is — proven live 2026-11-23: `kokon-ee-mcp` v0.12.1 on the host,
`QLS_DATA_DIR` → the service's data bind, discovered the running service
through `service.json`, dialed it on **127.0.0.1**, initialized
(`kokon 0.12.1`), and listed all 8 tools (`generate_image`,
`edit_image`, `edit_images`, `outpaint_image`, `inpaint_image`,
`upscale_image`, `get_image_info`, `get_generator_status`).

On atlas it does **not** work as-is, for one physical reason: the
advertised contract dials the service on **127.0.0.1** — the loopback
of the machine the MCP process runs on. Apollo's service is
host-networked, so host loopback *is* the service's loopback. Atlas's
service is a **bridge container** (D-027-3c: no published ports): its
127.0.0.1 is a container-private loopback that no process outside the
container's network namespace can reach, and its 39152 (the only
interface-visible door) is session-only — it rejects Bearer tokens. So
the advertised token path is unreachable from the atlas host or from
OWUI's container.

The fix is not new code but **placement**: run the MCP process in the
service's own network namespace. Then its 127.0.0.1 *is* the service's
127.0.0.1, the advertised discovery works verbatim, and the service
token never leaves the netns.

## The one gap between the two advertised pieces

- **Kokon** advertises an MCP server that speaks **stdio**
  (`mcp/`, `kokon-ee-mcp`).
- **Open WebUI 0.13** (the image verified on atlas) consumes MCP as a
  **client** over **streamable HTTP** (a tool-server connection in its
  Config: URL + headers/auth, or the Tether device relay). It has no
  stdio path of its own.

The ecosystem's standard bridge for exactly this is **supergateway**
(published npm: wraps a stdio server as a streamable-HTTP/SSE
endpoint). A command line, not code.

## Design (pre-logged)

- **D-039-1 (sidecar placement).** A small sidecar container on atlas
  (`~/infra/kokon-mcp/`): `network_mode: "container:kokon-ee"` — it
  *shares the service container's network namespace*, so
  `QLS_DATA_DIR=/data` (the existing `~/infra/kokon/data` bind, mounted
  read-only here) + 127.0.0.1 discovery is the advertised contract
  verbatim. It runs `supergateway --stdio kokon-ee-mcp` listening on an
  internal port in that netns (must not collide with 39150/39151/
  39152). **No published ports** — the listener exists only in the
  container netns, unreachable from the LAN/internet.
- **D-039-2 (shared network for OWUI reachability).** A new external
  docker network (e.g. `kokon_ee`) added to the kokon compose and the
  openwebui compose (one line each). The sidecar inherits the service's
  interfaces, so OWUI addresses it as `http://kokon-ee:<port>/mcp`.
  OWUI's existing published door (127.0.0.1:3000) and nginx vhost are
  untouched.
- **D-039-3 (the npm decision — owner).** Publish `kokon-ee-mcp` to npm
  (owner's npm account; AGPL is npm-legal) so the advertised
  `npx -y kokon-ee-mcp` in the README is actually true — or
  **D-039-3b**: skip npm and build the `.cjs` from the repo into the
  sidecar image at build time (self-contained, no public artifact; the
  README's npx line stays aspirational). Owner picks; the sidecar
  design is identical either way.
- **D-039-4 (OWUI registration — UI only).** OWUI admin UI: add the MCP
  tool-server connection (`http://kokon-ee:<port>/mcp`, optional shared
  header), assign it to the family model (the tool-calling one behind
  the gateway). No OWUI code.
- **D-039-5 (security boundaries).** The web password is **never**
  involved (the MCP uses the service token, per its advertised
  contract); the service token stays inside the shared netns (OWUI
  never sees it); the supergateway listener is docker-internal only
  (family trust domain, like OWUI already talking to the gateway on the
  LAN); the sidecar mounts the data dir **read-only** and
  `QLS_MCP_ALLOWED_DIRS` is scoped to the data/gallery mounts (the
  MCP README's file-scope discipline).
- **D-039-6 (no Kokon code changes).** That is the point of the task:
  the advertised MCP server, the published bridge, and the advertised
  OWUI client. If anything in the `mcp/` package needs a change to make
  this work, stop and report it — the contract should hold as
  advertised (it holds on apollo).

## Acceptance

1. From a chat in OWUI (chat.unveilx.com, the family model via the
   apollo gateway) asking for a picture, a Kokon tool is invoked and
   the image appears in atlas's gallery (and the reply references it).
2. `mcp/` suite green (35 + 3 skipped) on the current tree.
3. The supergateway listener is unreachable from outside the docker
   network (no published ports; a LAN curl to atlas on that port
   fails).
4. The browser product path (imagine.unveilx.com) is unaffected; the
   service log shows the MCP heartbeat as a separate client.
5. Docs: `service-on-prod-box.md` gains the MCP section (sidecar
   placement, the OWUI connection, rollback = down the sidecar + delete
   the OWUI connection); `atlas.md` updated; the `mcp/` README notes
   the box sidecar variant.
6. No GPU window needed (pure CPU/network work) — but if the box
   happens to be mid-window, do nothing that touches the engines.

## blockers

None, other than the owner prerequisite: **npm account + the
D-039-3/3b decision.** (D-039-3b removes the npm prerequisite entirely
— the task can start without it if the owner prefers.)
