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

None technical. The DNS record needs the owner's CF dashboard (or the
zone id); everything else is executable from apollo via ssh.
