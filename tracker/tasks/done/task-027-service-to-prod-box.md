# Task-027 (PARKED — owner-triggered) — move the kokon service (+ MCP + its data) to the stable prod box, behind nginx; the accelerator keeps only the gateway + engines

**Created:** 2026-11-11 — the owner moved Open WebUI to the more stable
"prod" box (nginx front, talks to the accelerator's gateway on the
one-door `:8081` with an OpenAI-compatible API) so that the accelerator
can be taken down for OS/CUDA/driver work. He asked whether moving the
kokon frontend/service the same way has footguns.

## Status (2026-11-12)

**Dev-box half DONE (this box).** The deployment at
`/home/p/ml/frontends/kokon/` moved from named volumes to the
bind-mounted shape (compose + `data/ config/ gallery/` beside it, the
clone staying at its dev home `/home/p/code/iamskeole/kokon`).
Carried over: the service token (`service-token`; `service.json` is
re-created on boot), the login password + sessions (`remote-auth.json`,
`remote-sessions.json`), the capability cache, the lab models
(257 MB) and the config with its backups. Fresh: the gallery (the
owner: the old images were test assets, not worth keeping). Verified:
old token accepted, door 404/401, both lab models installed, clean
boot, no GPU reservation needed. The old named volumes
(`kokon_kokon-*`) are kept as rollback until the owner says delete.

**Prod-box half PENDING — needs access to the stable box.** The
accelerator has no ssh path to it (no config, unidentifiable
known_hosts), so the agent here cannot run that half. The owner
supplies either (a) an agent session on the stable box to receive the
paste-able prompt in `docs/service-on-prod-box.md`, or (b) ssh access
(the accelerator's `~/.ssh/id_ed25519.pub` in the box's
`authorized_keys`) so this agent can do it end-to-end; plus the box's
name/IP, nginx layout and TLS-or-not. The fresh start there needs no
data migration (the owner dropped the test assets).

## Trigger (when this stops being parked)

The owner says go, and names the prod box (host/IP, its nginx, and
whether it terminates TLS — self-signed or real).

**The setup is already written up:** `docs/service-on-prod-box.md`
(handed over 2026-11-11 with the owner's request for a handover
prompt) — the paste-able brief, the bind-mounted compose (data dirs
next to the yaml, chown 1000:1000), the config seed (gateway + vlm
entrypoint → the accelerator's IP), the nginx values that are
load-bearing (SSE buffering off, ≥660 s timeouts, 64 m bodies, the
X-Forwarded headers), the verification checklist, and the receiving
side of the data migration. Executing the move = this task: pack the
three volumes on the accelerator (service stopped), ship them, run
the brief on the prod box, verify, retire the accelerator's volumes
only when the owner is happy.

## Why the move is architecturally clean (not a hack)

- The service container already runs `kokon-service --persistent
  --remote`: it never self-terminates on idle, and the remote port
  (39152, password + session cookies) is the browser door. The token
  port (39151) stays 127.0.0.1-only by design.
- **0.12 is explicitly reverse-proxy-aware** (`remote.rs`): a private
  peer's `X-Forwarded-For` is believed for the per-IP attempt limiter,
  and `X-Forwarded-Proto: https` from a private peer lets the session
  cookie carry `Secure`. nginx in front is the intended usage.
- The only cross-box traffic becomes service→gateway
  (`http://<accelerator>:8081/upstream/…`) — the same one-door,
  single-key pattern the prod box already carries for owui's
  OpenAI traffic. After the move the accelerator holds **no user
  data, no UI, no MCP** — pure muscle: gateway, engines, models.
  OS/CUDA/driver reinstalls then have nothing personal to break.
- The gateway does **not** health-check the service (verified in the
  live yaml: no per-model healthCheck entries; the kokon service is a
  client of the gateway, not a registry member) — nothing on the
  accelerator depends on the service being there.

## Footguns (the checklist for the actual move)

1. **nginx must be configured for this API, not just pointed at it:**
   - `/events` is a long-lived **SSE** stream: `proxy_buffering off`
     and a long `proxy_read_timeout` on the kokon location.
   - Job waits run for minutes (the service's own client timeout is
     600 s): `proxy_read_timeout >= 600s`, else nginx 502s mid-job.
   - Edit references are base64-in-JSON: nginx's default
     `client_max_body_size` is **1 MB** → 413 on any real photo.
     Set 32–64 m.
   - No WebSockets involved (SSE + fetch only) — no Upgrade handling.
2. **The token port must not go LAN.** nginx proxies to the *remote*
   port (39152) only. **The MCP server moves to the prod box with the
   service** (it speaks the token port over loopback; it cannot reach
   another box's loopback, and the token port should never be LAN-
   exposed). Whatever agent sessions rely on that MCP must move too.
3. **Gateway URL in the service config:** `127.0.0.1:8081` → the
   accelerator's **IP** (not a hostname: OS reinstalls can change
   names; the IP survives). Same 48-char box key (already
   multi-homed: owui, pi clients, the service).
4. **Data move:** three docker volumes (`kokon_kokon-{data,config,
   gallery}` on the accelerator; the service's real data — `gallery.db`
   + `inputs`/`originals`, `config.json` + backups, dated gallery
   folders). Safest: stop the service, tar/copy the volume contents,
   restore on the prod box, start, verify the DB opens and the
   **service token** is intact (the MCP depends on it; it lives in
   the data volume). The `~/Pictures/Kokon` folder on the accelerator
   is the *desktop app's* gallery (a second, separate Kokon on the
   box), not the service's — it stays with the desktop install.
5. **Prod compose differences:** drop the `nvidia` device reservation
   (the service needs no GPU in external-engine mode — every
   inference is at the gateway; without an nvidia runtime on the
   prod box the reservation would refuse to start). Image is 237 MB
   compressed. Ports 39151/39152 free on the prod box.
6. **Failure semantics (the point of the move):** accelerator down ⇒
   the prod box keeps serving the UI and login; job requests fail
   fast (3 s connect timeout) into the visible "engine unreachable"
   error state; when the accelerator returns, the next start
   re-adopts the gateway (this death/recovery path is the 16/16
   mock-gateway harness, sections 6–7). Cost: in-flight jobs during
   the outage are lost (re-submit works), and the first job after
   recovery pays one cold model load (same as the current 10-min idle
   unload — not new).
7. **Cosmetic/records:** the gateway yaml comment calls `:39152`
   "the only other fixed address on the box" — stale after the move;
   update it in the next config edit.
8. **Opportunity, not just a footgun:** if the prod nginx terminates
   TLS (self-signed is enough on a trusted LAN), browser clients
   gain a **secure context ⇒ WebGPU**, so the Layers lab's
   click-to-select actually runs on the client's GPU instead of the
   slow single-thread WASM fallback it silently degrades to today
   over plain-HTTP LAN. The service already honours
   `X-Forwarded-Proto` for the `Secure` cookie.

## Upstream relation

Stock 0.12 runs the service on the machine that owns the GPU (one
box, one person). A split service/engines deployment is fork territory
by nature (the external-engine mode *is* the fork) — nothing to
contribute, all additive on top of stock's own reverse-proxy support.

## Status (2026-11-23, night — the trigger fires; the prod box is atlas)

The owner named the prod box and the door: **atlas** (the stable box),
served at **`https://imagine.unveilx.com`** behind atlas's nginx, "per
the openwebui model at chat.unveilx.com". The 11-11 blocker (no ssh
path to the stable box) is gone — apollo's key auth works
(`p@atlas.local`). The task is back in `todo/` and is the current one.

**Superseded by what we now know** (from the 11-11/12 record above):
the **data migration is void** — the dev box's contents are fungible
(the owner's 11-23 ruling; it was wiped to a pristine install the same
evening), so atlas starts from zero and nothing ships. The **nvidia
reservation** item is already handled (the repo's compose has none —
the service needs no GPU). The image is the **CI-published**
`ghcr.io/iamskeole/kokon-ee` (task-033, closed the same night — the box
just pulls, no ghcr credentials anywhere). The image's default command
is still `kokon-service --persistent --remote`, and `remote.rs` is
still reverse-proxy-aware (private peer ⇒ XFF believed for the limiter,
`X-Forwarded-Proto: https` ⇒ `Secure` cookie) — both premises of the
11-12 record hold in the current tree. There is no origin check on the
remote door (a route allowlist only), so the `https://imagine.unveilx.com`
origin is fine, and the UI is same-origin through the proxy (CORS
irrelevant).

**Recon (read-only, from apollo, 11-23 night):**

- atlas = Ubuntu 24.04.5, 8 cores, 7.7 GB RAM, **no GPU**, 31 GB free on
  `/`, `~/` ≈ 3 GB. Docker 29.9.2 + Compose 5.6.1, systemd.
- Conventions: every compose project lives in `~/infra/<name>/` with its
  compose beside it (`nginx`, `openwebui`, `privacy-stack`, `searxng`,
  `registry`); `~/infra` is **not** a git repo (deployment = plain files;
  secrets sit mode-600 beside them, as the dev box's gateway key does).
- The nginx container (`~/infra/nginx`): conf.d bind-mounted with a
  **6-hour auto-reload loop** (new blocks land without a manual reload),
  `letsencrypt/` holds a **wildcard cert for `unveilx.com`** (renewed by
  the `certbot/dns-cloudflare` sidecar every 12 h) — so
  `imagine.unveilx.com` is already covered for TLS. The catch-all
  returns 444 for unknown hosts.
- `chat.unveilx.com.conf` is the template: 80→443 redirect, the wildcard
  cert, `client_max_body_size`, `proxy_pass` to the backend, `Host` /
  `X-Real-IP` / `X-Forwarded-For` / `X-Forwarded-Proto https`, 600 s
  read/send, `proxy_buffering off` (SSE). OWUI's container is reached
  by name over a shared docker network; the kokon service is
  **host-network**, so the proxy target is `127.0.0.1:39152`
  (loopback ⇒ the service's private-peer trust applies). Ports 39151/
  39152 are free on atlas (in use: 22, 53, 80, 443, 8080, 9090, 6767,
  loopback 3000/631/37945).
- **`imagine.unveilx.com` has no DNS record yet.** apex and chat are
  orange-cloud (CF-proxied: 104.21.11.11 / 172.67.147.130); atlas's
  public egress is 84.232.239.80 (the router forwards 80/443). The CF
  token on the box is zone-scoped and cannot even list zones, so the
  record (A/AAAA → the router, **proxied**) is a dashboard action, or
  the owner hands over the zone id / a broader token.
- **The one-door path is proven from atlas**: apollo's gateway is bound
  `*:8081`; from atlas, no key → 401, the shared 48-char key (the one
  OWUI already carries) → clean `model not found` for a bogus id
  (nothing booted). `apollo.local` mDNS resolves on atlas.

**Execution plan (discussed with the owner, then run):**

1. `~/infra/kokon/` on atlas: the repo's `docker/compose.yaml` (pull the
   published image), `data/ config/ gallery/` (uid 1000), `.ui-password`
   mode 600.
2. `config/config.json` seeded from the dev box's (the 11-23 clean-slate
   shape): `externalEngine` + `visionEngine` doors → **apollo's gateway**
   (decision: `apollo.local` vs the IP `192.168.0.202` — the 11-11
   footgun note says the IP survives reinstalls; `.local` survives IP
   moves), the same 48-char key. `docker compose pull && up -d`, then
   `--set-password` once.
3. Verify on atlas: ping/UI, the engine adopted **across the LAN**, both
   lab models report `doorConfigured/door` true (100% apollo-side),
   vllm on apollo untouched.
4. nginx: `conf.d/imagine.unveilx.com.conf` — the chat block adapted
   (`proxy_pass http://127.0.0.1:39152`, `client_max_body_size` 64 m
   per the handover doc, timeouts ≥ 660 s, buffering off, XFF/XFP;
   no WebSockets — SSE + fetch only).
5. DNS: `imagine.unveilx.com` A/AAAA → atlas's router, **proxied**
   (owner's dashboard, or via API if given the zone id).
6. **The final test, from the outside world**: `https://imagine.unveilx.com`
   from apollo (i.e. out over the internet and back through CF): UI 200,
   login, and a full t2i job through the whole chain (CF → atlas nginx
   → service → apollo gateway → engine), vllm verified resident after.
7. Records: `docs/service-on-prod-box.md` (the handover becomes the
   executed reality — fresh start, CI image, wildcard cert, the 127.0.0.1
   proxy target), the ml repo (the atlas deployment), this file → done.

**Open owner decisions:** (a) gateway address in the config —
`apollo.local` (proposed; mDNS, self-healing) vs `192.168.0.202` (the
11-11 note's IP-survives argument); (b) `client_max_body_size` 64 m
(handover doc) vs 200 m (the box's OWUI habit); (c) who adds the CF DNS
record (dashboard, or zone id/token for the API); (d) the LAN door
(39152, password) stays directly reachable on the family LAN alongside
the https door — assumed yes, that is the remote door's design; (e) MCP
on atlas (a clone + node, speaking the token port over loopback) —
optional, later.

## blockers

None — all cleared during execution (see below).

## Status (DONE, 2026-11-23 night) — atlas is live at https://imagine.unveilx.com

**Owner rulings on the open items:** (a) `apollo.local` in the config
(the name, not the IP — with a container-side pin, D-027-8); (b) 64 m
(200 m was the OWUI's habit for non-image attachments); (c) the agent
should dig into the box's CF token — the dig paid off: the token in
`~/infra/nginx/secrets/cloudflare.ini` works fine from the box (lists
zones, edits records); the earlier "zone-scoped, can't list" reading was
a failed extraction plus IPv6 egress (the token is IP-restricted to the
box's IPv4 — the nginx readme's own gotcha), so the record was added via
API from atlas; (d) **reversed by the owner**: the LAN door does *not*
stay directly reachable — the nginx front is the only door, which is
what keeps the internet-exposed box more secure; (e) MCP: later.

**What was executed (all from apollo over ssh, plus the DNS via atlas's
own token):**

- `~/infra/kokon/` on atlas: the repo's compose (fetched from `main`),
  `data/ config/ gallery/` (uid 1000), `.ui-password` (600), the CI
  image `:latest` pulled anonymously. The config is a copy of the
  accelerator's with all nine gateway addresses remapped to
  `apollo.local:8081` and `multiUser: false` (the family's shared
  password; no named accounts). Fresh start, per D-027-2.
- The service joined the box's shared `nginx_default` bridge network
  (the open-webui idiom) with **no published ports**; nginx proxies to
  the container name. The first attempt was `network_mode: host` with a
  `127.0.0.1:39152` proxy target — wrong twice over: the nginx
  container's loopback is not the host's, and behind atlas's
  default-deny ufw the bridge→host leg is black-holed (526, then 504/
  499 with fast local responses; no ufw rule exists and none was added —
  OWUI has none for the same reason: its traffic is FORWARD, handled by
  docker, not INPUT). The bridge shape needed zero firewall changes.
- `~/infra/nginx/conf.d/imagine.unveilx.com.conf`: the chat.unveilx.com
  model with the two corrections that mattered — the wildcard cert (the
  11-12 draft's self-signed is void) and **`X-Forwarded-For` as an
  append** (`$proxy_add_x_forwarded_for`, Cloudflare is in front).
- DNS: `imagine.unveilx.com` A → 84.232.239.80, proxied, via the box's
  token (zone `unveilx.com`, id `c9014d7006ae387e8fcb64e3ce58724f`,
  IPv4-forced egress).
- **The final test, from the outside world (apollo = a device outside
  the house, out over the internet and back through CF):** the UI 200
  in ~150 ms (three in a row); login → `{"signed_in":true}` with a
  `Secure` cookie, and the service log records `browser signed in from
  84.232.239.80` (the real client address — the XFF append survives the
  CDN); a full text-to-image job (job_0ff51e55d902, 58 s, 1024×1024,
  qwen-full Q8_0) through CDN → nginx → service → gateway → engine,
  image fetched through the gallery route, file in atlas's
  `gallery/2026-11-23/`. The lab over the internet: both models
  `door:true, doorConfigured:true, installed:false` — 100% apollo-side,
  zero bytes on atlas. vLLM resident before and after.
- **The one mid-window scare (operational, not a product defect):** the
  first job died with "the engine stopped answering mid-job" — the
  gateway journal showed it: `matrix: model=qwen3.8-27b set=vllm
  evict=[kokon-qwen-image-2.1-full]`, triggered by **this agent's own
  runtime**, which talks to vLLM through the gateway. The service
  reported the failure honestly ("the next job retries the load") and
  the retry succeeded; the `& kv` vision door survived the same vLLM
  request, as designed. Lesson recorded as D-027-10 and in the doc.
- `docs/service-on-prod-box.md` rewritten to the executed reality
  (D-027-7); the ml repo carries the atlas deployment record.

## decisions

- **D-027-1 (owner):** the box names — **atlas** = the stable box,
  **apollo** = the accelerator (this box). The 11-11 "unknown prod box"
  is resolved; ssh is `p@atlas.local` (the first attempt at
  `p@apollo.local` failed for the obvious reason).
- **D-027-2 (owner, restated):** atlas starts from zero; no data
  migration (the dev box's contents are fungible — the 11-12
  pack/ship/restore plan is void).
- **D-027-3 (owner, reversed the assumption):** no LAN door on the
  internet-exposed box. nginx (via Cloudflare) is the only front; the
  service publishes no ports. More secure than the remote door's
  "directly reachable" design, and sufficient: the family uses the
  browser.
- **D-027-4 (owner):** the config names the accelerator
  `apollo.local` (mDNS, self-healing) rather than the IP — with
  D-027-8 making the name resolvable inside the container.
- **D-027-5 (owner):** `client_max_body_size` 64 m on the vhost (the
  200 m is the OWUI's habit for non-image attachments; photos need 64).
- **D-027-6 (owner):** the CF DNS record is added with the box's own
  stored token, not the dashboard — the dig confirmed the token has
  zone edit rights from the box's IPv4 (the 11-23 morning
  "zone-scoped" reading was a red herring: empty extraction + IPv6
  egress, the readme's documented 9109 gotcha).
- **D-027-7:** the handover doc is rewritten to the executed reality
  (pull not build; `multiuser-<sha>`/`latest` tags; schema 18; wildcard
  cert; CF-aware XFF append; container-name proxy target; no migration;
  the update flow is `compose pull && up -d`).
- **D-027-8:** containers on this network cannot resolve `.local`
  (the host uses avahi via `mdns4_minimal`; Docker-written
  `resolv.conf` has no mDNS path). The compose pins
  `apollo.local:192.168.0.202` in `extra_hosts` (verified working with
  host- and bridge-networking). The config keeps the name; the pin is
  the one line that moves if the accelerator's IP ever changes.
- **D-027-9:** behind a CDN, the vhost must **append** to
  `X-Forwarded-For` (`$proxy_add_x_forwarded_for`), never replace it —
  the door takes the first entry for the per-IP limiter, and the
  replacement variant would key lockouts on the CDN edge. Proven live:
  the login log shows the real client's WAN address and the cookie
  carries `Secure`.
- **D-027-10 (operational, observed 11-23):** any client that hits
  **vLLM through the gateway** (agent runtimes, OWUI) evicts a running
  image job mid-job (exclusive sets; journal: `matrix …
  evict=[kokon-qwen-image-2.1-full]`). The job fails honestly and the
  next one retries the load; the `& kv` vision door survives either way.
  During long generations, agent sessions stay on the direct vLLM port
  (qwen-direct), per the autonomy protocol — never through the gateway.
- **D-027-11 (operational, minor):** this repo carries both `origin`
  (iamskeole/kokon-ee) and `upstream` (GabrieleLozupone/kokon); bare
  `gh` in this checkout can resolve to the **upstream** repo (it did on
  11-23 — the "run list" was upstream's runs). Always use
  `gh … -R iamskeole/kokon-ee` here.
- **D-027-12 (observed, minor):** a fresh box has no watermark model,
  so early jobs carry a plain note in their record ("the invisible AI
  watermark could not be added… the file's metadata still marks it as
  made with AI"). The job succeeds; the model installs on first real
  use. Not a chain defect.
- **D-027-13:** the acceptance test is *from the outside world* (the
  11-12 plan said "another device on the LAN"); the 11-23 execution
  used apollo as the outside client, over the CDN, and passed.
