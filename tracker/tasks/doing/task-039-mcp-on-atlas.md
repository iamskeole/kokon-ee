# Task-039 (DOING) — MCP on atlas: the family's OWUI drives Kokon through its MCP (design settled 11-24 late: a native streamable-HTTP mode; un-parked 11-25)

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
  container netns, unreachable from the LAN/internet. **11-24 late:**
  the sidecar runs `kokon-ee-mcp serve` (the native mode, port **39153**
  — recon confirmed 39151/39152 are the only service listeners in the
  netns; the earlier "39150" note is not used) instead of supergateway.
- **D-039-2 (shared network for OWUI reachability).** A new external
  docker network (e.g. `kokon_ee`) added to the kokon compose and the
  openwebui compose (one line each). The sidecar inherits the service's
  interfaces, so OWUI addresses it as `http://kokon-ee:<port>/mcp`.
  OWUI's existing published door (127.0.0.1:3000) and nginx vhost are
  untouched. **11-24 late, superseded by recon:** the `openwebui`
  (172.18.0.3) and `kokon-ee` (172.18.0.2) containers are **already on
  the same internal docker network** (`nginx_default`) — no new network
  needed; OWUI addresses the listener as `http://kokon-ee:39153/mcp`
  by container name.
- **D-039-3 (the npm decision — owner).** Publish `kokon-ee-mcp` to npm
  (owner's npm account; AGPL is npm-legal) so the advertised
  `npx -y kokon-ee-mcp` in the README is actually true — or
  **D-039-3b**: skip npm and build the `.cjs` from the repo into the
  sidecar image at build time (self-contained, no public artifact; the
  README's npx line stays aspirational). Owner picks; the sidecar
  design is identical either way. **11-24 late:** no longer on the
  critical path — the sidecar pulls a **CI-published image** (a second
  artifact of the workflow that already pushes the main image on every
  `main` push; atlas, like today, is a pull-only consumer), so the
  npm decision now only governs the README's host-side `npx`
  line for coding agents on a regular computer.
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
  advertised (it holds on apollo). **11-24 late, lifted by the
  owner:** the `mcp/` diff is the serve mode (the dial stays
  127.0.0.1 — placement (a) means the service is untouched).

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

The **npm decision is no longer a blocker for this task** (11-24 late):
the sidecar is a container that pulls a small CI-published image, so
the atlas path needs no npm account at all — publishing only decides
whether the README's host-side `npx -y kokon-ee-mcp` line (coding
agents on a regular computer, e.g. apollo) becomes true. The task is
otherwise **ready to start on the owner's word** (design settled
11-24 late: native HTTP mode + placement (a)).

## Design settled + atlas recon (2026-11-24, late)

**Settled with the owner:** (i) the native streamable-HTTP mode in
`kokon-ee-mcp` — **approved** (the supergateway bridge is out of the
design); (ii) placement **(a) the netns sidecar** — **approved**
("ok with your option A"); (iii) npm — **decoupled** as above. The
owner's use case, in his words: an agent in Open WebUI drives Kokon
via the MCP **iteratively, in a conversation, until the user is
satisfied**, while the Kokon browser UI keeps being served on the same
box. The loop works by construction: every tool call returns the
saved file path, so the next turn edits that file; with a vision
family model the optional preview JPEG lets the agent judge its own
result and self-correct. The one wrinkle noted for the wiring: the
tools read input files from the sidecar's filesystem (the read-only
data/gallery mounts), so chat can directly edit **anything in the
gallery** (what Kokon made, plus what the family uploaded through the
UI); a brand-new file arriving via OWUI's own upload path would need
a small bridge step — only if wanted.

**Atlas recon (read-only, via the owner's `ssh p@atlas.local`)** —
every design assumption checked against the live box:

- `openwebui` and `kokon-ee` are **already co-tenants of
  `nginx_default`** (172.18.0.3 / 172.18.0.2) → D-039-2 satisfied
  as-is, no new network (see the D-039-2 note).
- `~/infra/kokon/data/service.json` is present (133 bytes) — the
  sidecar mounts `~/infra/kokon/data` **read-only**; the service
  image carries no `kokon-service` binary in the sidecar's image, so
  the MCP's start-the-service path is inert there by construction
  (a down service is reported, never spawned).
- Listeners inside the shared netns: **39151** (127.0.0.1 only — the
  token door, exactly as designed) and **39152** (all interfaces — the
  browser door); 127.0.0.11 is docker's own DNS. **39153 is free** →
  the sidecar's listener.
- The service container publishes **no ports** (the `39152/tcp` in
  `docker ps` is EXPOSE metadata only); OWUI's published door
  (127.0.0.1:3000) and the nginx vhost stay untouched.
- OWUI runs `ghcr.io/open-webui/open-webui:main`, the 2026-11-17
  build — the 0.13 generation verified on 11-23. Its exact
tool-server client flavor (sessioned vs stateless streamable-HTTP)
is checked against the running admin UI during wiring; the serve
mode covers both (one setting flip between them).
- The sidecar's home on atlas: `~/infra/kokon-mcp/` (a compose file
  beside the existing `~/infra/kokon/` project — same convention as
  every other box project).

**D-039-7 (the MCP's default model — owner 11-24 late).** The owner:
the MCP is for a "just works", chat-based, user-friendly solution —
with three image models on the box and one MCP serving every chat, it
needs **a config value for the MCP's default model**. Design (settled
by code inspection this same night; **zero new service endpoints** —
`GET /config` and `POST /models/switch` already exist and carry
everything):

- The value lives **on the MCP client** (an env var in the same family
  as `QLS_DATA_DIR` / `QLS_MCP_ALLOWED_DIRS`; set once in the
  sidecar's compose). **Unset = today's behavior** (follow whatever
  model the service has active) — the apollo host install stays
  byte-identical.
- **Pin:** before a job the MCP reads the service's existing
  `GET /config` (it already returns the picker's `models` list — each
  entry a complete pipeline spec: model, gateway, VLM, VLM-vision,
  prompt bundle — the same list the UI's picker is fed from) and, if
  the active `externalEngine` differs from the configured value, posts
  that entry through the existing `POST /models/switch` (the exact
  shape the UI sends). The switch is a config write — the actual
  engine load happens on the first job anyway — so aligning is free.
- **Announce:** `get_generator_status` already reports the model; the
  tool descriptions stop hardcoding "Qwen-Image 2.1" (wrong the moment
  klein is the default) and name the box's default instead.
- **Semantics (the shared-service wrinkle, stated plainly):** the
  service is shared with the browser UI and the switch is global — so
  the contract is: **the chat is pinned to the configured model; the
  browser is free to wander; the next chat job quietly re-aligns.**
  That *is* the "just works" contract. A config value, not a user
  setting — the doctrine holds (we pick, the user picks nothing; the
  expert picker stays in Settings, off by default).
- **Atlas value: `qwen-turbo`** (the standing choice since 11-17) —
  the owner confirms it when he triggers.

**D-039-8 (the OWUI chat brain = the PUP door — owner 11-24 late).**
The owner: the model the family talks to in order to refine its
generations should be **the actual VLM/PUP** — "so we make efficient
use of VRAM". He settled the eviction story himself, and it is exactly
what the measured matrix says: when the user starts a chat on the PUP
door, the gateway loads the PUP plus — on the first image request —
the DiT, and **keeps both resident** (the qwen sets are precisely that
co-resident pair: `qwen-t = qt & (p | pt | pi) & s & kv`), evicting
vLLM **until someone asks for it again** (the daemon revives it on
demand; warm 60–90 s). Two measured nuances on record:

- The set carries **one PUP door per qwen state** (the mode-exclusive
  group — two 9B pups and the DiT don't fit). A refine loop
  (image-grounded) stays on the i2i door throughout; a pure
  text→image job mid-loop swaps only the 9B door (i2i→t2i) — the DiT
  and vLLM are untouched.
- **The chat pin is i2i, and it covers both chat situations** (owner
  clarification, 11-24 late): the t2i door is **text-in only — no
  mmproj** (that is what Qwen's t2i/i2i split means), so it cannot see;
  the i2i door sees *and* carries plain text chat fine (the vision
  path simply goes unused) — one door for the whole conversation,
  **no chat-side switching at all**. The only remaining switch is the
  *job-side* one above. **Zero-swap variant (b):** point the pipeline's
  t2i VLM call at the i2i door as well (a service-config change) —
  then the i2i door is the *sole* qwen PUP: chat and every job, one
  door, never a swap (the owner's "efficient use of vram" taken to the
  limit). **Gated on a quick A/B** (a few t2i jobs per door, compare
  the rewrites): the maker's t2i fine-tune would be bypassed, and the
  11-13 battery measured each door only in its own mode. Klein's BFL
  PUP is also a VLM, but a different fine-tune in a different set
  (`k & p & s & kv`) — a non-issue while the standing model is
  qwen-turbo.
- "Resident" is bounded by the box's existing **600 s idle TTL**: after
  ~10 min of silence the door unloads, and the next message re-boots it
  (a 9B Q4_K_M is a fast boot).

Facts measured this same night (apollo is the engine box; read-only):
**vLLM (qwen3.8-27b, 23.7/24.6 GB) was the sole GPU tenant** — and it
is today's OWUI default (`ui.default_models = qwen3.8-27b`), so every
family chat session holds the whole card and every image job evicts it.
That is the tax this decision removes. The i2i door's boot script is a
**plain llama-server** (Q4_K_M + mmproj-bf16, thinking on, **no forced
system prompt** — the PE system prompt goes in per request from the
service), so nothing in the door's code blocks chat use; the open
question is purely the **fine-tune's behavior** as a chat brain:
tool-calling reliability, system-prompt obedience, and no
JSON-envelope leakage (it was trained to rewrite prompts into a JSON
envelope with a fixed system prompt). **The OWUI groundwork already
exists:** all three "Kokon — PUP sidecar" entries are registered in the
OWUI model table (currently `off`; `openai.api_base_urls` already
points at the gateway), `g4-e2b-cuda` (the purpose-built vision-chat
door, ~6–7 GB) is ON as the fallback, and the big text doors (26–31B)
remain in the picker as the big-brain option. **Remaining:** (1) a
**short probe window** — evict vLLM, boot the i2i door, run the battery
(plain chat, a tool-calling request, image input, system-prompt
obedience, envelope leakage), revive vLLM canonically (the standard
small-window procedure); (2) if it passes, the i2i PUP entry becomes
the family chat's default (picker on / default for the account); if
not, E2B-cuda takes the seat. D-039-7 and D-039-8 compose: the **hand**
(the DiT, via the MCP's default model) and the **brain** (the PUP, via
the OWUI model) both run on the qwen family in the same set — the
refine conversation is with the very model that rewrites the prompt
for the DiT.

**Probe window (2026-11-25, apollo — `i2i-probe-039`, DONE in 11 min).
Verdicts:** the window booted the i2i door in 7 s (the router evicted
vLLM; the journal proves the choreography), ran the battery, ran the
full-stack A/B, and revived vLLM in ~65 s (16-token smoke clean).

- **Plain chat: passes** (obeyed the "keep answers short" system
  prompt). **Vision: passes** (a 64×64 red square, sent as a data-URL
  image part, answered "a solid, uniform bright red color"; 1,058
  image tokens in the prompt). **Envelope leakage: none** (asked to
  repeat its system instructions, it repeated exactly the trivial one
  it was given and nothing else).
- **Tool calling: fails — and this is the disqualifier.** The door
  given a standard OpenAI `tools` array and "make me a picture of a
  lighthouse at dusk" never returned a tool call: it hit the 1,024
  token cap (`finish: length`, empty content) *writing a prompt
  rewrite* ("this is a change-scope (A) case … I'll freely construct
  a new scene") — the PE fine-tune's role dominates over tool
  calling, whether or not its llama.cpp template carries a tool
  format. A door that cannot call a tool cannot drive the MCP from a
  chat, which is this task's whole purpose. **The i2i PUP is
  rejected as the OWUI chat brain.**
- **Zero-swap variant (b) — declined, for two reasons.** (1) The MCP
  flow never calls the PUP at all: `runJob` sends `enhance: false`
  (the calling model writes the final prompt and Kokon uses it as
  is); the PUP doors only run on the UI's Prompt+ path. (2) The
  A/B-as-planned turned out to be a no-op on the job itself (both
  arms generated from the raw prompt — 274 s vs 262 s, no
  difference), and the maker built separate t2i/i2i fine-tunes for a
  reason (the 11-13 battery measured each door in its own mode only).
  **The maker's split stays:** UI Prompt+ t2i rewrites go to the t2i
  door, image-grounded rewrites to the i2i door. (What the arms did
  prove: a fresh service + the MCP's serve mode against a live
  service — initialize, sessions, a long `tools/call` answered over
  SSE, the image saved, the lease released, clean shutdown.)
- **D-039-8 resolved: the family chat's brain stays `g4-e2b-cuda`**
  (the registered, ON "Kokon — vision door": `tools: true`, ~6–7 GB,
  fast boot). The VRAM story the owner wanted still holds via the
  matrix — chat on the E2B door and image jobs on the DiT alternate
  (the 600 s idle TTL keeps whichever just ran warm), and vLLM is
  evicted by either; the brain decision is about capability
  (tool-calling), not VRAM.
- **D-039-7 value (owner, 11-25): `kokon-qwen-image-2.1-full`** —
  "default to qwen full". The sidecar's `QLS_MCP_MODEL` gets that id
  on atlas (apollo host installs stay unset = follow the active
  model).

## Owner re-visit reminder (2026-11-24 — fired the same night; settled below)

The owner asked to be reminded, once the surrounding work (PUP ctx,
reference cap) was done, to **re-visit the MCP loopback concern: he
feels something is off in the apollo-vs-atlas loopback story and the
fix might be "exposing a port or changing the bind to 0.0.0.0" rather
than the netns-sidecar design above.** The reminder fired at 23:19 the
same night (task-040 done); the owner's re-visit came with a new axis
of his own — **the design re-visit below supersedes this section's
"two readings", with its factual error corrected**:

## Design re-visit (2026-11-24, late — owner: a native HTTP transport in `kokon-ee-mcp`)

The owner's proposal, verbatim in spirit: *now that we manage our own
fork, wouldn't it be more elegant to build an HTTP transport into the
MCP (alongside stdio), instead of the stdio-over-docker bridge?* The
assessment below is recorded with the proposal; the task stays parked
on his trigger, and the calls at the end are his.

**The transport gap is small to close — the SDK already ships the
other half.** `mcp/` bundles `@modelcontextprotocol/sdk` 1.32.1 into
its single `.cjs` (devDependency today, bundled in the published file),
and that SDK ships:

- `WebStandardStreamableHTTPServerTransport` — the spec-first-class
  transport (2025-03-26 *streamable* HTTP, exactly what OWUI 0.13
  consumes), running on plain `node:http` (Node ≥ 18: global
  `Request`/`Response`), **zero new dependencies**;
- the SDK's own reference server for it (two variants in
  `examples/server/`): a `Map` of sessions, `Mcp-Session-Id` routing,
  one `McpServer` per session (~100 lines; a stateless variant exists
  for clients that do no session handling), plus `sessionIdGenerator` /
  `onsessioninitialized` / `onsessionclosed` hooks and an
  `allowedHosts` option (DNS-rebinding protection — something
  supergateway cannot be made to have);
- `StreamableHTTPClientTransport` on the client side, for the test
twin of `test/protocol.test.ts` (spawn the bundle in serve mode, speak
streamable-HTTP back at it with the SDK's own client).

The existing server is already transport-agnostic (`McpServer.connect(transport)`;
the stdio entry is one call in `src/index.ts`). A serve mode is: argv
dispatch (`kokon-ee-mcp serve [--port] [--secret]` — same single-file
bundle, no `package.json` change), the `registerTool` loop factored
into a per-session server factory, a session registry, and one shared
`ServiceClient` (its lease/heartbeat is per-process: the sidecar holds
one `mcp` lease for its lifetime — the sidecar *is* the client, the
correct semantics). D-039-5's "optional shared header" becomes a
first-party `QLS_MCP_HTTP_SECRET` check on the listener. **Verdict on
the proposal: yes — a native mode is more elegant than supergateway
(a third-party proxy process wrapping ours, in the family stack), and
D-039-6's "no Kokon code changes" was the task's stance, not a
load-bearing wall; the owner is lifting it.** The cost to name:
`mcp/` is upstream-derived (present on upstream master), so the serve
mode is a fork-only divergence to keep through every rebase (the
identity rebrand already is one); if the shape proves generic it is a
future upstream-PR candidate (the vision.cpp #25 pattern) —
**decoupled from this task** either way. The npm decision (D-039-3/3b)
is orthogonal: the serve mode still needs the `.cjs` on atlas
(published, or built into the sidecar image).

**Two axes, don't conflate them.** The "stdio hackery on atlas"
is really *two* separate gaps, and the native HTTP mode closes only
one of them:

1. **Transport** — OWUI speaks streamable-HTTP, the advertised MCP
   speaks stdio. **Closed by the native mode**; supergateway drops
   out of the design entirely (D-039-1 as written is amended, not
   deleted: the sidecar stays, it runs `kokon-ee-mcp serve` instead of
   `supergateway --stdio kokon-ee-mcp`).
2. **Placement** — the advertised contract dials **127.0.0.1** and the
   service's token door is hard-loopback, so on the bridge-container
   atlas the MCP must still *reach* the door. The HTTP mode needs this
   settled on its own. The two readings, corrected:

   - **(a) Keep the netns sidecar (D-039-1 as written).** The sidecar
     runs the serve mode *inside the service's network namespace*; its
     127.0.0.1 *is* the service's 127.0.0.1; the advertised contract
     holds verbatim, **zero service code changes**, and the token door
     never binds a non-loopback interface — the tightest isolation
     possible on an internet-exposed box. What remains is one compose
     line (`network_mode: "container:kokon-ee"`): a standard Docker
     idiom for a tightly-coupled sidecar (the same family as the
     shared-bridge idiom the box already runs; `docs/service-on-prod-box.md`
     is full of it), not a code hack. Rollback = the sidecar down.
   - **(b) The 11-24 "expose a port / bind 0.0.0.0" intuition — with
     the mechanics corrected.** The reminder above is wrong as
     written: on atlas the service does **not** bind 0.0.0.0 in its
     netns for the door the MCP uses. `crates/qls-service/src/main.rs`
     hard-binds the *token* door to `127.0.0.1` in three places (the
     listener, the single-instance probe, and the MCP's own dial in
     `mcp/src/service.ts`); the door that does bind `0.0.0.0`
     (`remote.rs`) is the 39152 *session* door, which rejects Bearer
     tokens. So (b) is a **service-side change**, not just placement:
     make the token door's bind a deploy-time choice (env, e.g.
     `0.0.0.0` inside the container → reachable only from the docker
     bridge, never published — the invariant "the token stays out of
     every published/LAN-visible place" still holds), have
     `service.json` carry the reachable address (the service already
     knows its own bridge IP — `remote.rs::lan_address()`, the
     UDP-connect-to-8.8.8.8 trick), and make the MCP dial that address
     (its hardcoded `127.0.0.1` becomes "service.json's address,
     default 127.0.0.1" — apollo's contract byte-identical when the
     field is absent). Then the sidecar is a plain container on the
     shared bridge, and there is no netns line at all. The trade:
     the token door becomes reachable from *any* container on the
     family bridge (in practice not a regression — OWUI already
     carries the gateway key on that same bridge, and nothing is
     published — but it is weaker than loopback-only) plus a service
     diff and tests on top of the mcp/ one.

**Lean (the owner confirms or overrides): native HTTP mode +
placement (a).** It kills the hack that actually is one (a
third-party stdio→HTTP proxy in the family stack), keeps the
advertised 127.0.0.1 contract true on both boxes, keeps the token
door hard-loopback on an internet-exposed box, and costs zero service
code — the only remaining cleverness is one reversible compose line.
(b) is the fallback if the netns line still doesn't sit right: it is
the bigger change (service + mcp + tests on both sides) for weaker
isolation, and it is the owner's own starting intuition, so it stays
on the table.

**Owner's calls:** (i) native mode — **approved** ("ok with your
option A", 11-24 late); (ii) placement (a) — **approved** (same
words); (iii) npm — decoupled (the CI image, above); (iv) **MCP
default-model config — approved as a design** (D-039-7 below; the
atlas value `qwen-turbo` awaits his confirmation); (v) **OWUI chat
brain = the PUP door — direction approved** (D-039-8 below; the
behavior probe is the remaining open item). When the task starts,
D-039-1..6 get restructured to the settled shape (sidecar runs
`kokon-ee-mcp serve` on 39153 in the shared netns; no new network;
CI-published sidecar image; OWUI registration by URL + optional
shared header).

## The two readings the reminder left (superseded — kept for the record)

1. **Keep the advertised contract, fix placement (D-039-1 as written):**
   the sidecar shares the service's network namespace, so 127.0.0.1
   stays true and nothing new is exposed. → Now reading (a) above.
2. **Change the contract's address source:** the MCP discovers the
   service through `service.json` in the data dir (which the sidecar
   already mounts read-only); if that file also carried the service's
   *reachable* address (it binds 0.0.0.0 in its netns on atlas; the
   container's network IP is addressable from any container on a
   shared network), the same client code would dial 127.0.0.1 on
   apollo and the container IP on atlas — no shared netns, no exposed
   port (the token door would never leave docker networks). That would
   bend D-039-6 (it touches `mcp/` and what the service writes) — so
   it is the owner's call, with his port/bind intuition as the
   starting point for the conversation. → Now reading (b) above,
   **with the parenthetical corrected** (the token door is
   hard-loopback today; (b) needs the service change described
   above).

Whichever is chosen, the standing invariant holds: the service token
and its door stay out of every published/LAN-visible place; the web
password is never part of the MCP path.

