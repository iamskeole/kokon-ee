# The service on the stable box (engines stay on the accelerator)

The split deployment: **the stable box** runs the kokon service and the
browser UI — no GPU, no models, and (in the atlas deployment) **no
published ports at all**. All inference runs on the **accelerator** box
through its one-door gateway (`:8081`, keyed). The browser reaches the
service through the stable box's reverse proxy (and, on atlas, through
Cloudflare in front of it). This is the setup that lets the accelerator
go down for OS/CUDA/driver work while the UI keeps serving.

Executed and verified 2026-11-23 (task-027): **atlas** (the stable box)
serves **`https://imagine.unveilx.com`** — a real login and a real
text-to-image job completed through the whole chain (internet →
Cloudflare → atlas nginx → the service → apollo's gateway → the engine),
with the lab 100% apollo-side and vLLM resident throughout.

See also [fresh-box.md](fresh-box.md) (the whole stack on one machine).

## The boxes (as deployed)

| | atlas (the stable box) | apollo (the accelerator) |
|---|---|---|
| role | service + UI, behind nginx + Cloudflare | the gateway + every engine, pure muscle |
| holds | `~/infra/kokon/` (compose, config, data, gallery) | the model home, the gateway (`*:8081`), the engines, the `kokon-vision` door |
| no | no GPU, no models, **no published ports** | no user data, no UI, no MCP |

Conventions that shaped the atlas deployment (all pre-existing on that
box): every compose project lives in `~/infra/<name>/` with its compose
beside it; `~/infra` is **not** a git repo (plain files; secrets sit
mode-600 beside them); nginx is a container whose `conf.d/` is
bind-mounted and auto-reloads on a 6-hour loop; a wildcard cert for
`unveilx.com` is renewed by a `certbot/dns-cloudflare` sidecar every
12 h; the firewall (ufw) is active with **default-deny INPUT** — which
is why the service is a plain bridge-network container (below), not a
host-networked one.

## The prompt to hand over on a stable box

Copy from the next fence, fill in the angle-bracket values, and paste it
to the agent on that box:

```text
Set up the Kokon image service on this box. The full reference is in
docs/service-on-prod-box.md of the repo (step 3) — read it; the values
below override anything ambiguous in it.

Owner-supplied values:
- This box: <name> (<IP>); the accelerator: <name> (<IP>), gateway on
  :8081 — its only model door, every request needs the key.
- Gateway key: a file at <path on this box>, mode 600. Read it for the
  config. Never print it — not in your reply, logs, diffs, or commits.
- What people type for the UI: <https://name | name>, and whether a CDN
  (Cloudflare) sits in front of the box's nginx.
- Data location: <parent dir> — the folder that will hold the compose
  and the data dirs beside it (fresh start; the dev box's contents are
  fungible, nothing migrates).
- Default image model: the accelerator's current one (read its config).

Do, in order:
1. Preflight: docker + compose plugin; NO nvidia runtime needed (and
   none must be reserved — this service uses no GPU in this mode).
2. Create <parent>/kokon/ with data/, config/, gallery/ (chown
   1000:1000). Fetch the repo's docker/compose.yaml into it (the CI
   publishes the image on every push to main: tags multiuser-<sha> and
   latest — the box pulls, no registry credentials anywhere).
3. Clone the repo to a scratch dir (or fetch its files over https) and
   read docs/service-on-prod-box.md in full.
4. Write config/config.json: copy the accelerator's current one, change
   every 127.0.0.1:8081 address to the accelerator's name-or-IP, set
   multiUser to the owner's choice (one shared password: false), chmod
   600. If the box's containers cannot resolve the accelerator's
   .local name (most cannot — no mDNS in the image), pin it in the
   compose's extra_hosts.
5. docker compose pull && docker compose up -d; then
   docker exec -it kokon-ee kokon-service --set-password (password from
   a mode-600 file beside the compose; the running service picks it up).
6. If this box has a shared docker network for fronting services (atlas:
   nginx_default, the open-webui model) join the service to it and point
   the proxy at the container name. Publish no ports. Otherwise the box
   needs a firewall path from the proxy to the service (a host-networked
   backend behind a default-deny firewall is black-holed — see the doc).
7. Install the nginx server block from the doc (the exact values are
   load-bearing: SSE buffering off, 660 s timeouts, 64 m bodies, the
   X-Forwarded-For APPEND when a CDN is in front, X-Forwarded-Proto
   https), validate and reload nginx.
8. If the box's domain has a wildcard cert, use it; otherwise arrange
   the box's own TLS. Add the DNS record (proxied through the CDN, or
   the box's IP plain) — on atlas this used the box's own
   dns-cloudflare token (IPv4 egress only).
9. Verify with the doc's checklist — from OUTSIDE the network (the
   CDN's front), including one real generated image (this needs the
   accelerator up; ask the owner if it is), and confirm the accelerator's
   resident models were not disturbed.
10. Report back: what you did, where the service token file is (do not
    print the token), any deviation, and the one thing the owner should
    open from a second device to believe it.

Guardrails:
- Never print the gateway key or the service token (first 4 chars max,
  masked).
- This box stores no models and installs no engine; the only traffic you
  initiate toward the accelerator is through the gateway door on :8081
  with the key.
- If the box's reality conflicts with this brief (network idiom,
  firewall, TLS), stop and ask before adapting anything.
```

## Layout (as deployed on atlas)

```
/home/p/infra/kokon/
  compose.yaml            # the service container (fetched from the repo's main)
  .ui-password            # mode 600 — fed to --set-password once
  config/
    config.json           # mode 600 — the accelerator's config, addresses remapped
  data/                   # → /data    gallery + projects DB, service token, sessions, logs
  gallery/                # → /gallery saved images, dated folders

/home/p/infra/kokon-mcp/  # added 11-25 — the MCP sidecar (the family chat → Kokon)
  compose.yaml            # the sidecar container (the reference shape below)
  .env                    # mode 600 — the MCP's Bearer secret (QLS_MCP_HTTP_SECRET)
```

The repo is **not** cloned on the box (this box is a consumer, not a
source; the compose comes from the repo's `main`, the image from CI).
The three data dirs sit beside the compose on purpose: the owner sees
and backs them up.

### compose.yaml (the deployed shape)

```yaml
services:
  kokon-ee:
    image: ghcr.io/iamskeole/kokon-ee:latest   # CI publishes on every push
    container_name: kokon-ee
    restart: unless-stopped
    # Plain container on the shared nginx bridge network - the same shape
    # as open-webui: nginx talks to it by container name (docker handles
    # that forwarding; no ufw rule needed). Nothing is published: the box
    # is exposed to the internet, so nginx (via Cloudflare) is the only
    # door; the service's own listeners are reachable only from the bridge.
    networks:
      - nginx
    # The accelerator's name, pinned for the container: the image has no
    # mDNS (the host resolves .local via avahi; containers cannot).
    # Update here if apollo's LAN IP ever changes.
    extra_hosts:
      - "apollo.local:192.168.0.202"
    volumes:
      - ./data:/data
      - ./config:/config
      - ./gallery:/gallery

networks:
  nginx:
    name: nginx_default   # the shared bridge, owned by the nginx project
    external: true
```

**Why not `network_mode: host`** (the 11-23 finding): atlas's ufw is
default-deny on INPUT. A host-networked service is reached by nginx
through the bridge *gateway IP*, i.e. packets terminating on the host —
ufw black-holes them (nginx 499/504, nothing logged on the service).
The box's own idiom (open-webui, searxng, pihole) is the other way:
containers on a shared bridge network, docker doing the forwarding,
which ufw's FORWARD handling lets through — which is also why none of
those services has a ufw rule. Nothing published means the internet
front is the only front; the LAN cannot reach the service directly
(decision: wanted, the box is internet-exposed).

**Why `extra_hosts`**: the host resolves `apollo.local` through avahi
(`mdns4_minimal` in nsswitch); containers cannot (Docker writes their
`/etc/resolv.conf` without any mDNS path). The name stays in the config
(self-heals against IP changes the way mDNS would); the pin is the one
line that has to move if the accelerator's LAN IP ever changes.

The container's default command is `kokon-service --persistent
--remote`: it stays up when idle and opens the browser door (39152,
password + session cookies). `--set-password` (one-off, same command
form) sets the password without a restart; it is stored as an Argon2id
hash in `data/remote-auth.json` (sessions: `data/remote-sessions.json`).
The token API (39151) binds the container's loopback — with no
published ports it is reachable only from inside the container.

### The MCP sidecar (added 11-25)

The family chat (Open WebUI, `chat.unveilx.com`) drives Kokon through a
small sidecar container. The Kokon MCP speaks the Model Context Protocol
over HTTP natively (`kokon-ee-mcp serve`, the spec's Streamable HTTP
transport); the sidecar is the MCP's published image
(`ghcr.io/iamskeole/kokon-ee-mcp` — the repo's CI publishes it in the
same run as the service image): a Node process, no models, no engine, no
GPU.

```yaml
# ~/infra/kokon-mcp/compose.yaml (the deployed shape)
services:
  kokon-mcp:
    image: ghcr.io/iamskeole/kokon-ee-mcp:latest
    container_name: kokon-mcp
    restart: unless-stopped
    # Shares the service's network namespace: its 127.0.0.1 IS the
    # service's, so it reaches the service's loopback-only token door
    # without any change to the service. (The opposite of the host
    # finding above: the door is reached from inside the shared netns,
    # and nothing is published — the one new listener, 39153, is
    # visible only on the shared bridge.)
    network_mode: "container:kokon-ee"
    # The service's data volume, read-only: the sidecar reads
    # service.json (the token door's port + token) from it.
    volumes:
      - ../kokon/data:/data:ro
    environment:
      QLS_DATA_DIR: /data
      QLS_MCP_HTTP_PORT: "39153"
      QLS_MCP_HTTP_HOST: "0.0.0.0"
      QLS_MCP_HTTP_SECRET: ${QLS_MCP_HTTP_SECRET}   # from .env (mode 600)
      QLS_MCP_MODEL: kokon-qwen-image-2.1-full      # the owner's pick, 11-25
```

- **One new listener, on the bridge only**: port 39153 on the shared
  netns = the service's bridge IP, port 39153. Reachable from co-located
  containers (open-webui, nginx) and nowhere else — verified from the
  LAN and from the host's loopback (refused), and the sidecar itself
  publishes no ports. The service's own doors are unchanged.
- **The door**: every request must present `QLS_MCP_HTTP_SECRET` as a
  Bearer token; the same secret lives in Open WebUI's tool-server row
  (below). Generate it with `openssl rand -hex 32`.
- **The model** (`QLS_MCP_MODEL`, the owner's pick `kokon-qwen-image-2.1-full`):
  the service is shared with the browser, whose active model follows the
  last switch, so before each job the sidecar reads the service config
  and, on a mismatch, posts the matching picker entry (`POST
  /models/switch` — the same shape the UI's picker sends; no new service
  endpoint). The browser is free to wander; the next chat job
  re-aligns.
- **Open WebUI holds the connection** in its config store — the
  `tool_server.connections` row in `webui.db` (type `mcp`, url
  `http://kokon-ee:39153/mcp` — the service's container name on the
  shared bridge, so it survives IP changes — auth `bearer`, the same
  secret, an all-users read grant). The family's chat model can then
  call Kokon's tools in an ordinary conversation; a job from the chat
  lands in the same gallery as the browser's, and the service log shows
  the MCP as a separate client (its heartbeat) from the web clients.
- **Rollback**: `docker compose down` in `~/infra/kokon-mcp/` and delete
  the OWUI row. The service, the browser and the engines are untouched.

### config/config.json (how it was seeded)

Copied from the accelerator's current config (the source of truth for
model ids, gguf names, samples, prompt bundles and the two vlm doors),
with exactly two changes:

- every `http://127.0.0.1:8081…` address → `http://apollo.local:8081…`
  (the owner's decision: the name, not the IP — and `extra_hosts` makes
  the name resolvable inside the container);
- `multiUser: false` — the family box runs one shared password, no
  named accounts.

The file on disk may carry an older `schemaVersion` (16 when seeded);
the service migrates on load and re-saves in the current schema (18) on
the first config change. The gateway key is the same 48-char key the
accelerator's other clients carry (it lives mode-600 on both boxes; it
never appears in this doc).

### nginx (the deployed block, `~/infra/nginx/conf.d/imagine.unveilx.com.conf`)

```nginx
# Kokon (imagine.unveilx.com) - the family photo editor; the one service on
# the stable box. Mirrors chat.unveilx.com: Cloudflare in front, TLS here,
# one backend behind.
#
# The service is a plain container on this same bridge network, so the
# proxy target is its container name - exactly the open-webui model
# (docker forwards it; no ufw rule, nothing published on the host).
# Port 39152 is the service's password door; this block is its only
# front. That door is reverse-proxy aware: nginx is a peer on a private
# range, so it is treated as a reverse proxy - it reads X-Forwarded-For
# for rate limiting (lockouts key on the real client, not nginx) and
# X-Forwarded-Proto for Secure cookies.
#
# Cloudflare is in front, so X-Forwarded-For must APPEND to any incoming
# value, not replace it ($proxy_add_x_forwarded_for): a client behind its
# own proxy keeps its real address at the head of the chain, and the door
# takes the first entry.

server {
    listen 80;
    server_name imagine.unveilx.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl;
    http2 on;
    server_name imagine.unveilx.com;

    # The *.unveilx.com wildcard cert, renewed by the certbot sidecar
    # (dns-cloudflare, every 12h) - see this project's readme.
    ssl_certificate     /etc/letsencrypt/live/unveilx.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/unveilx.com/privkey.pem;

    location / {
        proxy_pass http://kokon-ee:39152;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $http_connection;

        # The door serves long-lived SSE job streams and image uploads.
        proxy_buffering off;
        client_max_body_size 64m;
        proxy_connect_timeout 60s;
        proxy_send_timeout 660s;
        proxy_read_timeout 660s;
        proxy_request_buffering off;
    }
}
```

The load-bearing values, and why:

- **`proxy_buffering off`** — `/api/v1/events` is a long-lived SSE
  stream; buffered, progress appears only at job end.
- **660 s read/send** — job waits outlive nginx's defaults (the service's
  own job timeout is 600 s).
- **64 m bodies** — edit references travel base64-in-JSON; the default
  1 m rejects real photos with 413. (The box's 200 m OWUI value is a
  habit for non-image attachments; photos need 64 m.)
- **`X-Forwarded-For` APPENDS** (`$proxy_add_x_forwarded_for`, not
  `$remote_addr`) — with Cloudflare in front, replacing would bury the
  real client behind the CDN edge; the door takes the first entry.
  Verified 11-23: the login log records the real client's WAN address,
  and the session cookie carries `Secure` (from `X-Forwarded-Proto
  https`). The door believes these headers **only from private peers**,
  so a direct client's claims are still ignored — no trust hole.
- **proxy target = the container name** — nginx resolves it at config
  load; when the service container is recreated it gets a new bridge IP,
  and nginx picks it up on its next reload (the box's 6-hour loop, or a
  manual one). The open-webui block has the same property.

### DNS / Cloudflare — **grey cloud, by the owner's choice**

`imagine.unveilx.com` and `chat.unveilx.com` both resolve **directly to
the box** (A → 84.232.239.80, **not proxied**): browsers terminate TLS
on this box's nginx with the local Let's Encrypt wildcard cert, and
Cloudflare is out of the data path entirely (verified 11-23: both names
present the Let's Encrypt `unveilx.com` cert, 200s, login and a full
job re-proven through the direct path). The reason is the threat model:
a proxied (orange-cloud) record means Cloudflare terminates the
client's TLS and therefore **sees the plaintext** of both sites — the
login password, session cookies, chat prompts and streamed answers,
prompts, generated images. The owner wants neither to pass through a
third party readable, so both records were unproxied (`PATCH .../
dns_records/<id>` with `proxied:false`) while every *other* record in
the zone stays proxied as before. The box's public IP is now visible on
DNS (ufw still limits exposure to 22/80/443/51820/8080/9090); CF's
DDoS/WAF no longer fronts these two names. Re-proxying either is one API
call if that ever changes. All CF API work from the box uses its own
token (`~/infra/nginx/secrets/cloudflare.ini`, single-line
`dns_cloudflare_api_token`), zone `unveilx.com`
(`c9014d7006ae387e8fcb64e3ce58724f`), **forced IPv4** (`curl -4`) —
the token is IP-restricted to the box's IPv4 (CF error 9109 on IPv6
egress); it lists zones and edits records.

### Verification checklist (all four states proven 11-23)

1. **From the internet** (any device outside the house):
   `https://imagine.unveilx.com` → 200, the Kokon UI, in well under a
   second, with the **box's own** certificate (Let's Encrypt
   `unveilx.com`) — if the browser shows a certificate error, check
   SNI/cert on the box's 443 (no CDN is in the path anymore); a 504/
   499 with fast local responses = the proxy-to-service leg, check the
   firewall, see the compose note.
2. **Login** with the password → `{"signed_in":true}`, cookie
   `kokon_session` with `Secure`. The service log then shows `browser
   signed in from <the real client address>` — nginx appends the real
   client IP to `X-Forwarded-For` (`$proxy_add_x_forwarded_for`, the
   append form so an earlier CDN hop would never be clobbered), and the
   door believes it only from private peers.
3. **The lab, from a signed-in browser over the internet**:
   `GET /api/v1/segment` and `GET /api/v1/matte` →
   `{"door": true, "doorConfigured": true, "installed": false}`. That is
   the split-box ladder's top rung: the models run 100% on the
   accelerator through the `kokon-vision` door; this box holds no lab
   bytes. (Check from a signed-in browser, not with the API token — the
   listener keeps a browser allowlist, and a token check passes where a
   browser call is refused; the 11-22 lesson.)
4. **One real job, from the internet**: a text-to-image job through the
   whole chain (internet → nginx → service → gateway → engine → back)
   completes and the image downloads from the gallery route; the file
   sits in `gallery/<date>/` on the stable box. On 11-23: 58 s,
   1024×1024, qwen-full. While it runs, the UI shows live progress (the
   SSE through nginx — if it only appears at the end, buffering is on
   somewhere).
5. **The accelerator's resident models**: vLLM (and the `kokon-vision`
   door) still up afterwards. The vision door is a permanent co-tenant
   of every matrix set, so it even survives a vLLM request (11-23: it
   did, while qwen was evicted — see the hazard below).

### Day-to-day

- **Update the service**: `cd ~/infra/kokon && docker compose pull &&
  docker compose up -d` (CI publishes `:latest` and `multiuser-<sha>` on
  every push to main; the sha tag is the pin for a known-good build).
  The data dirs are untouched. If you care about the new container IP
  being live in nginx before the 6-hour reload, `docker exec nginx
  nginx -s reload` after.
- **Password change**: `docker exec -it kokon-ee kokon-service
  --set-password` — no restart; every browser signs in again.
- **Accelerator down** (OS/CUDA/driver work): the UI and logins keep
  working; job requests fail fast into the visible "engine
  unreachable" state; when the accelerator is back, the next job
  re-adopts the gateway. In-flight jobs during the outage are lost
  (re-submitting works); the first job after recovery pays one cold
  model load.
- **Roll back / remove**: `docker compose down` in `~/infra/kokon`,
  delete the folder and the vhost (optionally the DNS record). The box
  started from zero, so there is nothing else to unwind.
- **The watermark**: a fresh box has no watermark model yet, so its
  early images carry a plain note in the record ("the invisible
  watermark could not be added… the file's metadata still marks it as
  made with AI"). The job succeeds; the model installs on first real
  use like any lab model.
- **An operational hazard (11-23, observed):** any client that talks to
  **vLLM through the gateway** (the box's agent runtimes, Open WebUI)
  makes the matrix evict a running image job mid-job — the sets are
  exclusive, and the job then fails with the honest "the engine stopped
  answering mid-job" (the next job retries the load). During a long
  generation, keep agent sessions on the accelerator's direct vLLM port
  (qwen-direct), per the autonomy protocol — never through the
  gateway.
- **MCP (installed 11-25)**: the family chat drives Kokon through the
  sidecar above (`~/infra/kokon-mcp/`): one more small container on the
  box, sharing the service's network namespace, one Bearer-guarded
  listener on the shared bridge, and an Open WebUI tool-server row with
  the same secret. The sidecar section has the shape, the model pin and
  the rollback.

### Dev on the accelerator, prod on the stable box — the workflow

- **Code flows one way only:** dev box (the accelerator, where the repo
  lives) → the project on GitHub → the stable box (`compose pull` +
  recreate). The stable box is a **consumer** of the project, never a
  source.
- **Data lives on the stable box, and stays there.** The gallery,
  config and token beside the compose are the family's actual state.
  Copies to the dev box, when wanted (backup, analysis), are explicit
  directory copies; the dev box's own service data is disposable (the
  owner's 11-23 ruling: fungible).
- **A live dev instance runs alongside** (it does, on the accelerator):
  the gateway is a shared, multi-client door by design. A dev
  generation while the family is using prod plays the ordinary eviction
  dance — no new failure mode.
- **Real-model validation happens on the dev box** (the GPU windows per
  the autonomy protocol). What gets deployed is a commit that already
  ran against the real models.
