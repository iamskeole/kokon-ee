# Engineering notes

How the Kokon box works inside, and the backend facts its design depends on. For using it,
see the [README](../README.md); for the whole stack on a machine, [fresh-box.md](fresh-box.md).

## Architecture

```
browser UI (React, embedded in the service)      kokon-mcp (MCP, stdio)      scripts
        │  HTTP + SSE, token / password                 │                        │
        └──────────────────────────────┬───────────────┴────────────────────────┘
                                       ▼
   kokon-service (crates/qls-service + qls-core)   127.0.0.1:39151 (token) · LAN :39152 (password)
   planner · pipeline · job queue · gallery · the lab (MobileSAM, BiRefNet)
                                       │  /sdcpp/v1/* through the box's gateway
                                       ▼
   the gateway (llama-swap) — one door, models loaded/evicted on demand
       ├─ stable-diffusion.cpp (stock): the image model, its VLM sidecars (Prompt+)
       └─ kokon-vision (stock vision.cpp + the server PR): MobileSAM, BiRefNet for the lab
```

| Crate / file | Responsibility |
| --- | --- |
| `crates/qls-core/src/geometry.rs` | Pure dimension math: 32-alignment, working canvas, outpaint/inpaint geometry |
| `imaging.rs` | EXIF orientation, alpha-safe resize, padding, masks, compositing, PNG metadata/ICC |
| `planner.rs` | Mode detection, resolution policy, upscale targets, memory budget |
| `pipeline.rs` | Builds engine requests, post-processes results (crop back, upscaler tiling, composite) |
| `sdcpp.rs` | Inference adapter — the only code that knows sd-server's schema (always the stock one) |
| `process.rs` | The engine door: probing, adopting, remembering the gateway's models; no process is ever owned |
| `engine.rs` | Async job queue, cancellation, OOM retry, saving |
| `speed.rs` | Per-machine speed calibration of time estimates |
| `diagnostics.rs` | Local GPU detection, box-health report (the engine door, the lab doors, upscalers, watermark) |
| `store.rs` | SQLite: the gallery + content-addressed inputs (and, task-032, the project list beside them) |
| `projects.rs` | The project list every device shares (the box's database): merged saves, tombstones, finished jobs |
| `paths.rs` | Path validation (no UNC/device/ADS/traversal, never overwrite) |
| `catalog.rs` | Pinned downloads the *service box* keeps: upscaler models, the watermark ONNX, the lab GGUFs; the engine build tag |
| `matting.rs`, `segment.rs` | The lab: in-process GGUF inference (MobileSAM, BiRefNet) and the `kokon-vision` door client |
| `onnx.rs` | ONNX Runtime for the lab and the watermark (CPU/Vulkan) |
| `config.rs` | The box settings: engine door, vision door, model entries, sample settings (schema 17) |
| `modelfile.rs` | What a `.gguf` / `.safetensors` file is, read from its header only (used by `/diagnostics`) |
| `crates/qls-service` | Headless service and stable HTTP API; single instance; lifecycle. `remote.rs`: the embedded browser UI, password sign-in and the LAN listener (rules in `qls_core::remote`) |
| `src/` | The browser UI (React + TypeScript), served by the service; logic in `src/lib/logic.ts` |
| `mcp/` | MCP server (TypeScript, official SDK) |

Internal names (`qls-core`, `qls-service`, `QLS_*` environment variables) come from the working
title used during development (Qwen Local Studio) and are kept on purpose.

## The engine door (the only way an image model runs)

The image model is a stock `sd-server` (stable-diffusion.cpp) on the box's gateway; the service
adopts it and never owns it (`config.json`'s `externalEngine` block):

```json
{
  "externalEngine": {
    "gateway": "http://127.0.0.1:8080/upstream",
    "model": "kokon-qwen-image-2.1-full",
    "modelName": "the-model.gguf",
    "sample": { "steps": 20, "cfg": 6.0 }
  },
  "models": [
    { "label": "Fast", "model": "model-a", "modelName": "the-model.gguf" }
  ]
}
```

- Every engine call goes to `{gateway}/{model}/sdcpp/v1/...`. The gateway loads the named model
  on demand (and evicts it when idle) and reports the loaded model through `/capabilities`.
- The service never starts, restarts, idle-unloads or stops the engine. The probe in
  `ensure_ready` adopts a running engine only when the model it reports matches `modelName`,
  and marks it as not owned. A different model, or no answer, is a visible error state;
  nothing is ever spawned by the service.
- A load in progress at the gateway shows as the "loading" server state: the probe waits out
  the load (a long timeout) instead of giving up after a few seconds, so a slow first image is
  visible and cancellable, not a failure.
- `models` is the list of entries the box's config names: the active entry's `model` is what
  the engine runs, and each entry may carry `sample` overrides (steps, cfg, sampler) that apply
  to the jobs it starts. Switching is `POST /api/v1/models/switch` with the chosen entry: only
  the config changes; the gateway is first touched when work needs the model, which evicts the
  current tenant and waits out the load, shown in the usual loading state.
- The engine's capabilities are only learned while it runs (asking the gateway about it would
  load it). Each adoption is remembered per model id (`external-caps.json` in the data folder)
  and restored when the service starts, so the UI can report what the model supports — e.g.
  which upscalers the engine has — before the engine has loaded; the model itself is still
  reported as not loaded, and the next job re-probes and replaces the remembered values.
- Each generation records the model name the engine reports, so the gallery and the image
  details can tell the models apart.
- Failed jobs surface through the engine's job API, not its log: a job that fails inside the
  engine usually comes back as a generic message. The actual error line is in the engine's own
  log — with a gateway, the gateway's — so diagnose failures there, not in the service's.
- A job that outlives the gateway (the engine is evicted or reloaded mid-job) fails plainly
  after a short grace: the service polls the job record, treats gateway errors as "engine
  down" after 30 s and a vanished job after 10 s.
- In this mode "no model loaded" is the resting state: opening the UI starts nothing; the
  model loads on the first job, through the gateway.

## The boxes

- **The service box** (this repo's image): the browser UI, the API, the job queue, the gallery
  and the lab. It is the only custom layer; everything it runs on its own CPU/GPU is stock
  inference (ONNX Runtime for the lab and the watermark, GGUF for the lab's in-process models).
- **The engine box**: stock engines behind the box's one-door gateway — stable-diffusion.cpp
  for the image model, llama.cpp for the prompt-enhancer sidecars, vision.cpp for the lab's
  `kokon-vision` door. It is not part of this repo: the gateway's config names it (see
  [fresh-box.md](fresh-box.md) and [service-on-prod-box.md](service-on-prod-box.md)).
- **Co-residency**: the gateway's tenants share the GPU under its own scheduling; the service
  never evicts anything. The vision door's budget is capped (its `ggml` arena) and it falls
  back to CPU, so it co-resides with whatever else the box runs.

## The lab (segment and matte, task-037)

The lab's two models — MobileSAM (click/box segmentation) and BiRefNet-lite (matte) — run on the
service box, never in the browser, in a capability ladder:

1. **The door**: `config.json`'s `visionEngine` names a `kokon-vision` entry behind the same
   gateway. A door answers `POST /sam`, `POST /birefnet`, `GET /health` (plain JSON, no
   Kokon naming); the client in `segment.rs`/`matting.rs` uses it first.
2. **In-process**: with no door, the service runs the models itself (GGUF through ONNX Runtime,
   `onnx.rs`), with an LRU embedding cache per image.
3. **Hard error**: neither available, the UI says so plainly. There is no browser-side model.

The `kokon-vision` door is stock vision.cpp with the upstream `vision-server` PR
([Acly/vision.cpp#25](https://github.com/Acly/vision.cpp/pull/25)): CPU + Vulkan only, all five
model families (MobileSAM, BiRefNet-lite, Depth-Anything-V2, MIGAN, RealESRGAN), the model files
pinned in the ml repo's model home. The door is a permanent sidekick of the gateway's matrix
sets (`& kv`), on-demand boot, `ttl` resident.

## Verified backend facts (stock stable-diffusion.cpp)

The image engine is the stock upstream build; the facts below were measured on the builds the
box pins (see the ml repo's engine pins) and hold for any compatible release.

- Convolutions default to `auto`: implicit-GEMM direct convolutions on CUDA (RTX 20xx+) and
  Vulkan, VAE included. `--fa` and `--diffusion-conv-direct` are deprecated aliases (still
  accepted).
- Qwen-Image 2.1 conditioning does not truncate the prompt; the DiT/VAE weights stream through
  the gateway's tenant's files, which the gateway's box keeps.
- The text encoder lives in RAM (`--params-backend te=cpu`); below the budget, layers run
  split CPU/GPU.
- Time estimates: the time model was measured on an RTX 5070 Ti. `speed.rs` compares every
  finished job with the uncalibrated prediction and keeps a smoothed ratio per engine, GPU,
  memory mode **and model** (`<data>/speed.json`; since 043 the key carries the model id —
  models on one box calibrate themselves separately instead of sharing, or poisoning, one
  factor). Two honesty rules: the plan applies the external engine's sample block, so the
  estimate is the job that actually runs (H3: the door's 6 steps and built-in schedule, no
  sigmas of its own — before 043 the UI estimated Turbo's 8-step qwen curve, ~9 min for a
  ~2-minute 16 MP image), and on a split box whose local GPU the service cannot see, the
  uncalibrated estimate of an external model is hidden (the UI says "time known after the
  first image") until that model's own first measured job.

## Resolution and upscaling policy

Every job runs a single diffusion pass at the working size (Auto: ~1 MP for Fast, ~1.5K for
Balanced, native 2K for Quality; or the 1K/1.5K/2K/Source/Custom choice), capped by the VRAM
token budget that the target shares with its reference images. The earlier two-pass mode
(≈1 MP render → ESRGAN → low-denoise refine at 2K) was removed on 2026-10-06: in A/B tests
the refine made edits look worse than a single pass.

Whenever a result is larger than what the model rendered (edits restored to the source size,
outpaint, sizes above the VRAM budget) it is enlarged with the upscaler model (×4) and then
resized down to the exact size; Lanczos is only used for shrinking or factors below 1.1×.

### Upscalers (2026-10-12)

Default: Superscale-SP (ESRGAN, WTFPL), 128 px tiles with direct convolutions (13.2 s / +455 MB
for 1472² on an RTX 5070 Ti, vs 24.7 s / +747 MB without). Also offered: MoSR (experimental),
UltraSharp (non-commercial), Lanczos.

**Where they live (task-038):** the upscaler models live on the **engine box**, in the engine's
own models folder; the engine's `capabilities.upscalers` (fetched while the engine runs) is the
arbiter of what a named upscaler means. On a split box the service's `/upscalers` reports the
engine's list (its `installed` is `null` until the engine has been queried once), and
`POST /upscalers/{name}/download` is refused — there is no service-box copy to download. On a
single box (service and engine on the same machine) the service keeps and downloads them under
its own sdcpp folder and offers them the same way. `upscaleBlend` mixes the result with
Lanczos; `upscalePrescale` shrinks the input so x4 lands ~1.41x the target.

MoSR by Philip Hofmann (CC BY 4.0), 17 MB: ported into stable-diffusion.cpp and merged
upstream (PR #2128), so every stock engine since `master-964` carries it. Tile size per
request (`planner::upscale_tile`): 768 px with ≥ 14 GiB of planning memory, 256 px below;
`upscaleTileSize` overrides it. Why MoSR is not the default: on clean full-size renders
enlarged < ~2.8x it invents a fine crosshatch that the final downscale turns into a visible
grid. Superscale shows the same on JPEG blocks and dark areas; UltraSharp over-smooths skin
and invents lines. A/B crops: the in-app comparison uses the recorded crops.

The **Upscale** action runs the upscaler model straight on the image. The Detailed method
(a low-denoise img2img pass at about 2K first) was removed on 2026-10-09: Turbo's schedule
leaves only 1–2 steps at strength 0.3. Requests with `"method": "detailed"` run as the
upscaler model only.

### Turbo (the default image model since 2026-10-09)

Every job runs Qwen's official 8-step distilled model
([Qwen/Qwen-Image-2.1-Turbo](https://huggingface.co/Qwen/Qwen-Image-2.1-Turbo), released
2026-10-09, Qwen Research License like the base), as
[unsloth/Qwen-Image-2.1-Turbo-GGUF](https://huggingface.co/unsloth/Qwen-Image-2.1-Turbo-GGUF)
Q4_K_M (4.2 GB) on the engine box; the ml repo pins the file. The box's config names the
engine model and its sample settings; the picker (the `models` list) also offers Qwen
Image 2.1 (full), Flux Image 3 Klein and MiniMax H3 Image — switching is a config change,
never a service-owned process. The **default** (first boot, and what a new box starts on)
stays Turbo; H3 is a picker entry, not a default change.

- Schedule: `planner::TURBO_SIGMAS` = `sample_sigmas` of the checkpoint's model_index.json
  ([1, .978453, .95418, .926626, .89508, .845148, .704534, .414568, 0], fixed at every
  resolution), sent as `sample_params.custom_sigmas` (used verbatim, steps = len − 1).
  CFG is always 1. Steps, CFG and scheduler are not settable (Qwen evaluated no other
  schedule); the sampler is.
- Turbo goes no further than native 2K: Qwen trained and published 2K buckets only, and
  2048² is the measured VRAM limit on 16 GB. Larger outputs come from the upscaler model.
- **Same-seed A/B, RTX 5070 Ti, 1024², 3 t2i + 3 edits** (recorded in the original product's
  bench): Fast (20 steps) 19.1 s t2i / 23.8 s edit; Viggle v0.3 LoRA (6 steps) 10.1 / 13.6 s;
  official Turbo (8 steps) 9.5 / 13.1 s. ~4.5 s per image are fixed (encode, 2.7 s decode).
  Quality equal by eye.
- Candidates rejected or not adopted: Viggle turbo v0.3 LoRA (works, slower per step, third
  party); Turbo8 and Pruna 8-step LoRAs (unofficial); Sarvam Tez 2-step (t2i only); Alibaba
  Fun-Acc 4-step (a PDD bundle, not a plain LoRA); LightX2V Lightning for 2.1 (not released).

### MiniMax H3 Image (picker entry, 2026-11-26)

[MiniMax-H3-Image](https://huggingface.co/MiniMaxAI/MiniMax-H3-Image) (community
license): a 33B DiT distilled to the card's recipe — **6 steps, euler, CFG 1, no custom
sigmas** (the `minimax-h3-image` schedule in the boot script; a request carrying a custom
schedule replaces it, so the service's per-model sample row sets
`{steps: 6, cfg: 1, sampler: "euler", clear_sigmas: true}` and sends no sigmas), 16× VAE
with 2×2 patchify (32 px per token), up to 16 reference images, unified t2i and
reference-edit in one checkpoint. Works ~4–20 MP, best at 16; the 52491bd build caps the
door at **4096 per side** (capabilities' limits), so the planner's H3 row keeps that
bound: area presets 4/8/16 MP, align 32, max references 16, no bucket table.

- **Prompt+ is the makers' PE fine-tune** (`MiniMax-H3-Image-PE`, a Qwen3.8-27B
  fine-tune; [mradermacher's Q4_K_M GGUF](https://huggingface.co/mradermacher/MiniMax-H3-Image-PE-GGUF),
  16.5 GB + the f16 mmproj). One llama-server door serves both workflows — t2i (system
  message + task with the instruction spliced into the trailing `{}` placeholder) and edit
  (**one user message, no system** — the makers' edit task prompt carries its own role
  statement) — because the PE is a native VLM. Output is an H3 envelope
  (t2i: `expanded_caption` + a 10-value `aspect_ratio` enum; edit: `expanded_caption` +
  `target_aspect_ratio` W:H), and since 042 the reply is **grammar-constrained per
  request**: the pinned llama.cpp takes the makers' strict schema as per-request
  `json_schema` (converted to GBNF, output-format scoped — the thinking stream stays
  free) and the card's thinking trio (`enable_thinking`, `preserve_thinking`,
  `reasoning_effort: xhigh`) as `chat_template_kwargs`. The params files live in the
  hash-verified bundle (`t2i/` and `edit/request_params.json`); the values keep their
  **native JSON types** — the door 400s a stringified `"true"` where it wants a boolean.
  The service decodes the reply, strips the thinking, retries once out-of-contract, and
  on exhaustion falls back to the raw text with a job note (never triggered in the
  window). The PE carries **no MTP module** — not a quant gap: the makers' own bf16
  weight map (2387 tensors) ships no nextn/MTP head, and the pinned build's qwen35 MTP
  path *hard-asserts* `n_layer_nextn > 0`, so `--spec-type draft-mtp` on this file
  aborts the door at model init rather than degrading (parked: an MTP-carrying PE
  re-opens it). The door runs at `-c 65536` with `--image-min-tokens 1024`: a 16-reference
  edit (the card's maximum) consumed 41.3k prompt tokens + the thinking budget and still
  finished (117 s; per-image cost ≈ 2.0–2.4k tokens). Warm door: ~26 tok/s, ~19.5 GB
  resident, cold boot ~17–25 s through the matrix; the service gives the PE call a 900 s
  clock.
- **The eviction dance.** The PE (17.4 GB) and the H3 DiT (~21 GB bare) **cannot
  co-reside** on a 24 GB card, unlike the qwen/klein doors that fit beside their PUPs.
  Each is its own bare matrix set (`h3`, `h3p`), so a Prompt+ job sequences two gateway
  requests and the matrix does both swaps: PE boots (evicting whatever held the card),
  answers, the DiT boots (evicting the PE), the 6 steps run. No router machinery; the
  journal of one job shows both evictions. A plain (no Prompt+) job never touches the PE.
- **The 52491bd origin gate.** That build's server rejects requests whose `Host`/origin
  it does not know (`server_access.cpp` → `Request origin is not allowed`). The gateway's
  reverse proxy preserves the client `Host` (`apollo.local:8081`), so every DiT boot
  script passes `--cors-origins http://apollo.local:8081,https://apollo.local:8081` —
  without it the capability probe (and every job) 403s at the door. The gateway itself
  stays key-gated; the upstreams stay keyless loopback services.
- **16 MP stands (measured 2026-11-26, this 24 GB card):** a full 4096×4096 job
  (6-step schedule + VAE decode) peaked at **22.2 GB** — 2.4 GB under the card; 2048²
  at 19.2 GB. The Qwen-calibrated token budget would have clamped quality to ~6.4 MP, so
  the H3 row carries a measured peak (`measured_peak_mib: 23_552`): a card that holds it
  keeps every single-image tier; reference-bearing jobs and unknown-VRAM boxes keep the
  shared budget (multi-reference 16 MP is not measured). Since 042 the picker also
  offers the native tier explicitly: the entry's `resolutions` list
  (`auto/1k/2k/4k`) gains the new `4k` mode (4096²), labelled *native size* in the UI;
  a 3:2 request at that tier sizes to 4096×2720 under the side cap.
- **VAE decode: 256 px tiles with FA off (measured 2026-11-27, this 24 GB card;
  task-044).** The original H3 VAE (the fp16 305 M one: 24-layer 2D-ViT decoder, 16×
  spatial upsample) decoded cleanly **only in 256 px chunks with flash-attention off**,
  on this build: at the same seed, a fine pixel-grid mesh appears over high-frequency
  detail (fur) from 512 px tiles up, with VAE FA on it appears even in 256 px chunks
  and in the single full-image 4096² decode (the makers' SGLang pipeline is bf16
  full-context — the only known clean 16 MP path, unreachable on a 24 GB card), while
  1–2 MP stays clean in every configuration (too coarse to read). The owner picked the
  256/FA-off look from a contact sheet of all eight variants. So the door keeps
  `--vae-tiling --vae-tile-size 256` with `--flash-attn off,diffusion=on` (no
  `vae=on`), and the engine's `vaeTileSize: 256` in the box config tells the planner
  the door's tiling is authoritative: no per-job shaped tile (the large-GPU memory
  heuristic used to silently replace the 256 with ~896 px tiles — the visible grid in
  the 2026-11-27 10:08 image) and no memory-derived untile.
- **No brush, by design (042).** H3 has no mask/inpaint path — its editing is text plus
  up to 16 reference images. The picker entry says so (`supportsInpaint: false`,
  deployment data, config schema 19); the UI hides the mask tool for that entry while
  qwen keeps it (boxes stay Klein-only), and the engine drops a `mask`/`marks` sent
  through the raw API for such an entry with a job note, completing the job as a
  whole-image edit.
- **The UI reads H3's own data (043).** The /config reply carries the active model's
  derived row (`activeModel`: the service plans H3 with its own 16-ref / 4·8·16 MP
  row, so that is what the UI shows): the refs cap in the UI is the card's 16, not the
  stale qwen profile's 10, and the quality presets say what H3's tiers are (~4 MP / ~8 MP /
  native 16 MP · slowest) instead of the qwen strings (~1 MP / ~1.5K / native 2K). The
  qwen-area triple keeps its historical labels; anything else is derived (and marked
  *native* only when the tier is the model's top resolution, the size menu's rule). The
  H3 time estimate hides until H3's first measured job on the box (the split-box rule
  above), then self-calibrates under H3's own key.
- **The PUP A/B (D-041-21).** The 9B Qwen-Image-2.1 PE doors (the owner's first
  candidate) were asked the same questions directly: they answer fast (15–24 s) but in
  the **Qwen-2.1 contract** (`{"rewritten_prompt": …}`), not the H3 envelope the DiT is
  conditioned on — through the H3 path that is an out-of-contract raw-text fallback. The
  makers' PE is the only PUP that speaks H3, so the H3 picker entry names it for both
  VLM doors (the picker keys the PUP on the DiT's model id, so a second entry sharing the
  id could never have routed to the 9B doors anyway); the 9B doors stay with the Qwen 2.1
  pipeline.

## Prompt+ (the prompt enhancer)

Optional, off by default. A Qwen3-VL sidecar behind the same gateway as the image model
rewrites the user's short instruction into a full English prompt (looking at the input image
for edits) before the image is made.

- **The sidecar, not the image engine.** Each pipeline entry in the config names its VLM
  endpoint (`vlm` for text-to-image, `vlmVision` for image-grounded jobs — routing is the
  guard, an image part never reaches a text-only door) and its prompt bundle. The sidecar
  answers a whole turn; the answer replaces the user's prompt for the image job, and a down
  sidecar hard-fails the job (no silent raw-prompt fallback). **Name the doors explicitly
  in every picker entry:** `absorb_secrets` inherits a stored `vlm`/`vlmVision` whenever an
  incoming config names none (an explicit `null` is indistinguishable from absent), so a
  switch to an entry that leaves a door unnamed keeps the previous arm's door — for H3
  that would route its edits to the wrong PUP.
- **Prompt bundles** are data in the data dir (the BFL-bundle pattern: a `manifest.json`,
  per-mode `system_prompt.txt` files, a `LICENSE`), swappable per deployment, with the
  constants in `enhancer.rs` as the fallback when a deployment has no bundle. They are
  our own texts, chosen in blind A/Bs against Qwen's official PE prompts. The one
  exception is a **contract model's bundle** (the makers' H3-PE prompts, family
  `minimax-h3`): the texts are the model's own training contract, so the loader verifies
  the makers' published sha256s at load and a broken or hash-mismatched bundle fails the
  job cleanly (and the prompt text itself never enters the repo, logs or chat). For our
  own texts the rules kept: copy the user's fixed content exactly (quoted text, counts,
  colours), name the medium early, always state the light, no quality tags; for edits lead
  with the operation, describe only what changes and end with one "keep everything else
  unchanged" clause. Output is always English. Lesson: **any example colour or object in a
  system prompt leaks** into the outputs.
- **Length:** the prompts ask for a length that fits the request (t2i ~80-150 words, up to
  ~250 for crowded scenes; edits 20-60, up to ~150 for several changes; compose 100-200, up
  to ~300).
- **Review first:** `enhance_only` runs a job that stops after the prompt (no image, nothing
  saved); the app streams it into the prompt box. The next job sends the text with
  `enhanced_from` (the user's words), so the record keeps both and nothing is rewritten twice.
- **A painted area and Expand:** two more built-in prompts. For a brushed area the enhancer
  sees the region as it is with the area highlighted and writes what goes in the area, or
  names what is removed, after the action's words; for Expand it sees the canvas with the new
  borders empty. With nothing typed for a removal or an expansion, the app has Prompt+ write
  the prompt into the box first (`enhanced_from: ""`), to approve or change.

### New image from references (compose)

`mode: compose` is a new image whose size follows text-to-image (chosen size, ratio, a ratio
in the prompt, else square, at the quality preset), never image 1's: an edit's output takes
image 1's shape and, in Auto, at most its area, so a small reference gave a small result.
The references share the token budget with the output as in edits (`cap_for_sources`), each
at most the reference cap (Standard 1 MP) and never larger than the output. In the UI,
**New image** with images added sends compose; **Edit image** keeps today's edit/combine.
Compose runs in two steps: each reference is labelled alone, then the scene is written from
the request and the labels.

## The browser UI on phones

Measured on an iPhone 15 (iOS 27, Safari and the home-screen app) and in Playwright at
390x844, dpr 3:

- **Previews.** The stage, full screen, comparisons and the paint editor show
  `/gallery|inputs/{id}/preview` (at most 1280 px, JPEG) while it is as sharp as the full
  image there (`logic.ts` `stageFitsPreview`, `viewerWantsFull`); the full file loads once
  the zoom needs more pixels. A 30-version project of 2048 px PNGs opened with 0.84 MB
  instead of 15.1 MB.
- **No image-sized layers.** WebKit backs a composited layer at its untransformed size x the
  device scale, so a 4096 px image laid out at its own size and shrunk by a 3D transform
  cost ~150 MB per layer at dpr 3, and an edit's full screen (four such layers while the
  image developed in) crashed Safari. `ZoomPan` lays the content out at its drawn size with
  a 2D translate (`flat`).
- **Status bar.** iOS 26+ ignores `theme-color`: the bar takes the colour of an opaque fixed
  element at the top of the page, read at load. `StatusTint.tsx` is a 6 px strip at the top
  of the body, created anew at every colour change, which makes iOS read it again; black
  while full screen is open.
- **Take Photo.** The file input stays in the page until it answers: iOS drops a detached one
  while the Camera is open. Safari names camera shots `image.jpg`, which is how
  `PhotoReview.tsx` tells them apart.

## HDR

The source's ISO 21496-1 gain map is reused and faded out where the edit changed brightness by
more than ~¼ stop (gone at 1 stop). Reading the gain map needs `ffmpeg` 7.1+ on the service
box's PATH; without it, or when the result's framing differs from the source (outpaint, other
aspect ratios), the result is saved in SDR and the job's notes say why.

## Downloads

Sources, sizes and SHA-256 sums are pinned in `crates/qls-core/src/catalog.rs`. What the
service box downloads: the lab's two GGUFs (MobileSAM, BiRefNet-lite), the watermark's ONNX
models, and (on a single box) the upscalers. Files ≥ 16 MB download over 8 parallel
connections in 8 MB pieces (HTTP Range, positional writes into a preallocated `.part`,
finished pieces recorded in `.part.segments` so pause or a crash can resume). When nothing is
left to start, idle connections also fetch the piece with the most left; the first to finish
wins. A server without range support falls back to one connection. A file that fails twice,
or a link that moved, is reported with its download page and target path so it can be
fetched by hand.

## Settings not shown in the UI

In `config.json` (schema 17): `vramOverrideMib` (plan sizes as if the GPU had this much VRAM;
0 = detect; for testing), `upscaleTileSize` (0 = automatic), `prefixCacheType`,
`eagerLoad`, `sageAttention`, and the dormant fields of earlier schemas (`hardwarePreset`,
`macEngine`, `serverPort`, `keepServerRunning`, `idleUnloadMinutes`), which no longer act.

## Testing smaller cards on a bigger one

`vramOverrideMib` in the config makes the planner size images for a smaller card. The
recorded numbers below come from the original product's diagnostics (real cards, same
planner), and are the reference for what a box of that card should do:

- Real RTX 5070 12 GB (12 GB planning, Turbo Q4_K_M, text encoder on the CPU, text to
  image only): 1376×768 at 1.24 s/step (10 s sampling, 1.9 s decode); ~3.2 MP
  (2400×1344, 1216×2624) at 4.3 s/step (35 s sampling, 7.2 s tiled decode). No OOM retries;
  ~5.8 GB VRAM free with the model loaded. About 1.5× the 5070 Ti's step time, so
  `planner::step_seconds` (calibrated on the Ti) underestimates on this card.
- RTX 5070 Ti 16 GB (16 GB planning, Turbo Q4_K_M, text encoder on the CPU): text to image
  2048² at 3.67 s/step (29.8 s sampling, 5.9 s tiled decode), 36 s total; edit at 1472² with
  one reference at 2.0 s/step, 24 s total; ESRGAN 4× of 1448² to 5792² in 22 s.
