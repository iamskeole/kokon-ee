# Local HTTP API

Kokon's service listens on `127.0.0.1:39151` (the token door). The MCP server and your own
scripts use it with the token. When access from other devices is on, a second address
(port `39152` by default, every network interface) serves the browser UI to them, signed in
with the password; the token does not work there, and browsers must send the `x-kokon-csrf`
header on every change ([remote-access.md](remote-access.md)). The routes below are the ones
registered in [`crates/qls-service/src/api.rs`](../crates/qls-service/src/api.rs); request
bodies are JSON unless a row says otherwise.

## Base URL and token

The service writes its port and token to `service.json` in its config folder (in the image:
`/config`, or `$QLS_CONFIG_DIR`) when it starts:

| Setup | Path |
| --- | --- |
| Box image | `/config/service.json` |
| Bare binary | `~/.local/share/kokon-ee/service.json` (or `$QLS_CONFIG_DIR`) |

Base URL: `http://127.0.0.1:<port>/api/v1` (the port is usually `39151`). Every call except
`/ping` needs `Authorization: Bearer <token>`. Errors come back as `{"error": {...}}`.

**Bash:**

```bash
INFO="/config/service.json"   # or $HOME/.local/share/kokon-ee/service.json
TOKEN=$(sed -n 's/.*"token" *: *"\([^"]*\)".*/\1/p' "$INFO")
PORT=$(sed -n 's/.*"port" *: *\([0-9]*\).*/\1/p' "$INFO")

curl -s -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:$PORT/api/v1/status"

curl -s -X POST "http://127.0.0.1:$PORT/api/v1/generate" \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"prompt":"a lighthouse at dusk, 16:9","quality":"fast"}'
```

The service has to be running for `service.json` to exist and be current.

## Status and settings

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/ping` | Liveness check (no token needed) |
| GET | `/status` | Engine state (through the gateway), model, GPU and VRAM, active job, queue |
| GET | `/jobs` | List the jobs the service knows about |
| GET, PUT | `/config` | Read or replace the box settings (GET adds `activeModel`: the active model's derived row — refs cap and preset tiers — that the UI renders, 043); the reply to PUT says whether the engine door changed |
| GET | `/diagnostics` | Box health: the engine door, its capabilities, the lab models, the watermark — as JSON |
| GET | `/logs?lines=200` | Tail of the service log (default 200 lines, at most 4000) |
| GET | `/enhancer/defaults` | The built-in system prompts for Prompt+ |

## Generating

| Method | Path | Purpose |
| --- | --- | --- |
| POST | `/generate`, `/edit`, `/inpaint`, `/outpaint`, `/upscale` | Simple operations; wait for the result unless `?wait=false` |
| POST | `/jobs` | Full request (see `GenerationRequest` in `planner.rs`) |
| GET | `/jobs/{id}` | Job status |
| GET | `/jobs/{id}/wait?timeout=60` | Long-poll until the job finishes |
| POST | `/jobs/{id}/cancel` | Cancel a job |
| POST | `/plan` | Dry run: resolved mode, working and output size, notes |
| GET | `/events` | Server-sent events (jobs, server, gallery) |

Simple-operation fields: `prompt`, `image` / `images[]`, `mask`, `action`
(`edit|remove|replace|add`, inpainting), `width`, `height`, `aspect_ratio`, `quality`
(`fast|balanced|quality`), `seed`, `output_path`, `preserve_dimensions`, `private`,
`left/right/top/bottom` + `units` (`pixels|percent`) for outpainting, `scale` for upscaling.
`/generate` with `images[]` makes a new image that uses them as references (people, products,
a style), sized like any new image; `/edit` with several images edits image 1 with the
others. The count is capped by the active model's profile (`recommendedSettings.max_references`,
default 10 — the makers' card max): a job over the cap is a 400 (`Use at most N reference
images.`) at submit, and the browser stops adding references at the same number. Full requests (`/jobs`) also accept `"mode": "upscale"` with
`"upscale": {"scale" | "width"/"height"}`, `"mode": "compose"` (the references case of
`/generate`), and with `"enhance": true` (Prompt+) `"enhance_only": true`: the job only
writes the prompt (`enhanced_prompt` in the job, no image), so it can be read and changed;
send it back as `prompt` with `"enhanced_from"` set to your original words. Prompt+ works on
every mode but upscales; for a painted area (`mask`) and an expansion it writes only what
goes in the area (for a removal: what is removed) or in the new borders, and the prompt may
be empty for a removal and an expansion. `"enhance_detail": true` asks Prompt+ for a longer
prompt with more context about what is in the image or what changes.

The engine itself is a stock `sd-server` (stable-diffusion.cpp) on the box's gateway; the
service never owns its process. A job that outlives the gateway (the engine is evicted or
reloaded) fails plainly after a short grace.

## The lab (segment and matte)

The lab's models run on the box (in-process, or through the `kokon-vision` door behind the
gateway — `config.json`'s `visionEngine`), never in the browser:

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/segment` | The segment model's state (the in-process copy, or the door's) |
| POST | `/segment/download` | Download MobileSAM (progress in `/downloads`) |
| POST | `/segment/encode` | Encode an image's embeddings: `{"image": "<b64 PNG>"}` |
| POST | `/segment/predict` | One click or one box: `{"image", "point": {"x","y"} | "box": {...}}`; answers four masks |
| GET | `/matte` | The matte model's state (BiRefNet) |
| POST | `/matte/download` | Download BiRefNet-lite (progress in `/downloads`) |
| POST | `/matte` | A matte for an image: `{"image": "<b64 PNG>"}`; answers the matte as a PNG |
| POST | `/matte/cancel` | Stop a running matte |

## Downloads

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/downloads` | The download that is running (lab models, upscalers, watermark models), if any |
| POST | `/downloads/cancel` | Cancel it |
| POST | `/upscalers/{name}/download` | Download one upscaler to the box (refused on a split box: the engine box keeps its own) |

## Input images

| Method | Path | Purpose |
| --- | --- | --- |
| POST | `/inputs` | Upload an image: raw bytes as the body, optional percent-encoded `X-Filename` header, `?private=true` for private mode. Up to 210 MB |
| POST | `/inputs/import` | Import an image from a path on this box: `{"path": "..."}`; `?private=true` is allowed |
| GET | `/inputs/{id}` | The stored input's record |
| GET | `/inputs/{id}/image`, `/inputs/{id}/thumb` | The image, or its thumbnail |
| GET | `/inputs/{id}/preview` | A screen-sized copy (at most 1280 px on the long side, JPEG; PNG if it has transparency), made on first request |
| POST | `/provenance/check` | What an image's AI markings say: raw image bytes as the body (up to 210 MB), nothing stored. Answers `ai`, `metadata` (`digital_source_type`, `creator_tool`, `c2pa_manifest`), `watermark` (`checked`, `found`, `kokon`, `text`), `sha256`, `checked_at`; see [ai-marking.md](ai-marking.md) |
| POST | `/image-info` | Size, aspect, type and embedded generation metadata of an image |
| POST | `/private/clear` | End private mode: delete every private input and result |

## Gallery

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/gallery` | List results, see the query parameters below. Replies `{"items": [...], "total": n}` |
| GET | `/gallery/{id}` | One result with its recipe |
| PATCH | `/gallery/{id}` | Change a result; today only `{"favorite": true|false}` |
| DELETE | `/gallery/{id}` | Delete a result and its files |
| GET | `/gallery/{id}/image` | The result image |
| GET | `/gallery/{id}/thumb` | Its thumbnail |
| GET | `/gallery/{id}/preview` | A screen-sized copy, as for inputs |
| GET | `/gallery/{id}/original` | The result before the upscaler enlarged it, when it was kept |
| POST | `/gallery/{id}/export` | Save a copy to a path: `{"path": "...", "overwrite": false, "original": false}`; the format follows the file extension |

`GET /gallery` query parameters (all optional):

| Parameter | Meaning |
| --- | --- |
| `q` | Text to search for |
| `favorites` | `true` to list only favorites |
| `limit`, `offset` | Paging |
| `sort` | `recent` (default) or `oldest` |

## Projects

The project list every device shares (the box's database, beside the gallery's own records — task-032; during this one release it is also mirrored as a read-only `projects.json` in the data folder, so a rollback to the previous image reads it). A version
is a result (`"kind": "gen"`, `genId`) or an imported image (`"kind": "input"`, `inputId`).

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/projects` | Every project, most recently changed first: `{"projects": [...]}`, each as `/projects/{id}` gives it |
| GET | `/projects/{id}` | One project: its `meta`, its versions (`items`), the `selected` one |
| GET | `/projects/{id}/records` | The records of all its versions in one answer: `{"generations": {...}, "inputs": {...}}` by id; versions deleted from the gallery are left out |
| PUT | `/projects/{id}` | Save a device's copy: `{"project": {...}, "known": [keys]}` (`known`: the versions that device had seen, so a version added elsewhere meanwhile is kept). Answers with the merged project |
| DELETE | `/projects/{id}` | Remove the project from the list (its images stay in the gallery) |
| POST | `/projects/import` | Merge a list of projects in, once |

## Models, LoRAs and LUTs

| Method | Path | Purpose |
| --- | --- | --- |
| POST | `/models/switch` | Switch the active model: the full external-engine body of the picker entry — `{"gateway", "apiKey", "model", "modelName", "sample", "vlm"?, "vlmVision"?, "promptBundle"}` (a bare `{"model": …}` is rejected, 042) |
| GET | `/upscalers` | The upscalers the engine offers and which one is active |
| GET, POST | `/loras` | List LoRAs; add one from a path on this box: `{"path": "..."}` (copied into the LoRA folder) |
| PATCH, DELETE | `/loras/{id}` | Update or remove a LoRA |
| GET | `/luts` | List the color LUTs |
| POST | `/luts/import` | Add a `.cube` file from a path: `{"path": "..."}` |
| DELETE | `/luts/{name}` | Remove a LUT |
| GET | `/luts/{name}/preview?source=gen:<id>&max=<px>` | The LUT at full strength on a result (`gen:<id>`) or imported image (`input:<id>`), reduced to `max` pixels (256 to 4096), for a live preview |
| POST | `/luts/{name}/apply` | Save the image with the LUT applied as a new version, keeping the source: `{"source": "gen:<id>", "intensity": 1.0, "private": false}` |
| POST | `/examples` | Add the bundled example projects and list them |

## Service routes

| Method | Path | Purpose |
| --- | --- | --- |
| POST | `/session/heartbeat`, `/session/release` | A client registers or drops its lease on the service |
| POST | `/shutdown` | Stop the service |
| GET, POST | `/remote` | Access from other devices: its state and addresses; turn it on or off, `{"enabled": true, "port": 39152}` (needs a password) |
| POST, DELETE | `/remote/password` | Set or change the password (`{"password": "..."}`, every browser signs in again); remove it, which also turns access off |
| GET | `/auth/session` | A browser: whether it is signed in and whether a password exists |
| POST | `/auth/login`, `/auth/logout` | A browser signs in with the password (a session cookie) or out |

## Named accounts (multi-user)

With `multiUser` on in the config, browsers sign in with a name and a password of their own
and see and save only their own gallery; the shared password stays the operator.

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/users` | List accounts (operator only) |
| POST | `/users` | Create one: `{"name", "password", "enabled"}` |
| POST | `/users/enable` | Enable or disable an account |
| POST | `/users/{id}/password` | Change the account's password |
| DELETE | `/users/{id}` | Remove an account (its gallery stays, owned) |
