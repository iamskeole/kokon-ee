# Kokon in Docker

The box service, without a build step: pull the published image and run it.
Inference does **not** run inside the container — the engines (the image
model, the lab's models, the assistant) live on the engine box behind its
one-door gateway; this box holds the service, the browser UI and the MCP
server, and its one config file points at that gateway. The whole stack is
[fresh-box.md](fresh-box.md); the split layout with this box as the
family-sized one is [service-on-prod-box.md](service-on-prod-box.md).

## What you need

- Linux with Docker and the Compose plugin. No GPU on this box: the
  in-process lab models (single-box mode) run on CPU, and the GPU work
  happens on the engine box.
- The engine box's gateway reachable from this one (its address and key
  go into `config/config.json`).

## Start it

Each push to the repo's `main` publishes the image as
`ghcr.io/iamskeole/kokon-ee` (tags `multiuser-<short sha>` and `latest`),
so you need the compose file — no clone for the image and no build:

```bash
mkdir kokon && cd kokon
curl -fsSLO https://raw.githubusercontent.com/iamskeole/kokon-ee/main/docker/compose.yaml
docker compose up -d
docker exec -it kokon-ee kokon-service --set-password   # once; at least 8 characters
```

(Or `git clone https://github.com/iamskeole/kokon-ee kokon-ee && cd kokon-ee/docker`
and use the compose file that is in the repo. To run a build you made
yourself instead of the published image, `docker build
-f docker/Dockerfile -t ghcr.io/iamskeole/kokon-ee:latest ..` from that
folder first — the verifiable route.)

Open `http://<this box's address>:39152` in a browser and sign in. A fresh
`config/config.json` must name the gateway (`externalEngine`, and the
`visionEngine` door for the lab) — the reference host's file is the
template.

## Where things are

| Folder (beside `compose.yaml`) | Inside the container | What |
|---|---|---|
| `data/` | `/data` | the gallery + projects database, the service token, the lab's in-process models (single-box mode), logs |
| `config/` | `/config` | settings (`config.json`: the gateway address and key) |
| `gallery/` | `/gallery` | the images saved to the gallery |

The service runs as uid 1000, so the three folders must be writable by it.

## Everyday use

```bash
docker compose pull && docker compose up -d   # update to the published image
docker logs -f kokon-ee                       # what the service is doing
docker exec -it kokon-ee kokon-service --set-password   # change the UI password
```

For access from outside your home use Tailscale or another VPN
([remote-access.md](remote-access.md)), never port forwarding on your router.
