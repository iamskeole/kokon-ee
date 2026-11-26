# Task 041 — wire MiniMax H3-Image as a new selectable model (service plumbing + the makers' PE PUP door)

**Status: doing (created 2026-11-26, owner brief; the gateway half of the door already
exists on the box from the 11-26 morning work).**

Owner brief, condensed: wire the new image model MiniMax H3. The image model is
already downloaded and configured on the gateway (11-26: `kokon-h3-image`, DiT
Q4_K_M + P-H3 INT8 ConvRot TE + fp16 VAE, bare matrix set `h3`, booted and
proven t2i + edit through the gateway in `models-041-window`). What remains is
the **engine plumbing**: the prompt-enhancer door (the makers' PE fine-tune as
a GGUF), the service-side wiring (model entry, prompt bundle, request/response
contract, sizes, sample params), the eviction dance the 24 GB card forces, and
the A/B the owner asked for. The model is a *picker entry*; the default model
does not change in this task.

## What exists on the box (11-26 morning, before this task)

- `models/sd-cpp/MiniMax/minimax-h3-image/` (DiT `model-Q4_K_M.gguf` 12.2 GB +
  `model-Q4_K_S.gguf` VRAM fallback + `p-h3-image_text_encoder_int8_convrot.safetensors`
  5.1 GB + `minimax_h3_image_vae_fp16.safetensors` 1.1 GB + provenance).
- Boot script `_scripts/serve-kokon-h3-image.sh` (sdcpp 52491bd): the card's
  recipe as launch defaults — `--sigmas minimax-h3-image`, euler, cfg 1.0,
  6 steps, `--seed -1`, `--rng cpu`, `--offload-to-cpu`, `--max-vram 24`,
  explicit `--backend/--params-backend …=cuda0` (the 11-26 CRUX fix), VRAM
  drain gate, 1024² boot default.
- Gateway entry `kokon-h3-image` (9112) + matrix set `h3: h3 & s & kv` (bare —
  ~21 GB co-resides with nothing; its vision part is the in-engine TE).
- `models-041-window` (13:31–13:35): cold boot ~16 s, **t2i 1024² 6 steps in
  19 s, VRAM peak 18.2 GB**; edit with one reference 200 OK (request capture
  `h3-edit.json`); matrix eviction of vllm verified; vllm revived. **16 MP is
  NOT yet measured** — the quality preset must be proven or reduced (blocker B2).

## The model cards (read 2026-11-26; summary, not the text)

**MiniMax-H3-Image** (hf `MiniMaxAI/MiniMax-H3-Image`, sha
`8ced60068623def5b74b4d10558117cb61d6482d`; community license):
- 33.1B DiT (50 layers), 16× VAE + 2×2 patchify → 32 px per token; the H3
  (Qwen3-VL) text encoder conditions the DiT (layer 50 hidden states).
- Distilled to **6 NFE, no CFG**; `num_inference_steps` only 6, `guidance_scale`
  only 1 (others → 400); `negative_prompt` rejected.
- Unified t2i + reference-image editing in one checkpoint; **up to 16
  reference images**; references in upload order are `<Picture 1>…` in the
  prompt.
- **Works about 4–20 MP, best around 16 MP**; common 16 MP sizes 4096×4096,
  4736×3552, 5024×3360, 5472×3072, 6272×2688 (+ flips);
  `min_generation_area` default 4 MP (smaller sizes are generated on a larger
  canvas at the same ratio and resized back).
- The maker's API turns a short prompt into a detailed English description via
  **H3-Image-Context-IR** (or the local PE) *before* generation, and uses the
  PE's **size recommendation**: "When your aspect-ratio selection is `auto`,
  use the `aspect_ratio` returned by the model for image generation. If you
  select an explicit ratio, use your selected ratio."

**MiniMax-H3-Image-PE** (hf `MiniMaxAI/MiniMax-H3-Image-PE`; part of the H3
family, same community license): prompt expander, **a fine-tune of
Qwen3.8-27B** (the family of the box's assistant runtime — the owner's "(you!)").
NVFP4 17.9 GB (the maker's deployment: SGLang, xgrammar-constrained JSON,
`--reasoning-parser qwen3`). Two workflows:
- **T2I PE (fine-tuned)**: a released system prompt + a task prompt (the user
  instruction spliced into a trailing `input_text: {}` placeholder, Python
  `.format` semantics — the template's `{{`/`}}` unescape). Reference request:
  temp 1.0, top_p 0.95, top_k 20, min_p 0, presence 0, repetition 1,
  `max_tokens 4096` **including thinking**, thinking on, reasoning effort
  xhigh. Output: thinking, then **exactly one JSON object**
  `{"expanded_caption": …, "aspect_ratio": …}` with the ratio a 10-value enum
  (1:1, 5:4, 4:3, 3:2, 16:9, 21:9 + flips).
- **Edit PE (prompt-only, "still under development")**: a 38 KB task prompt
  used as the first part of a single user message; then a reference count
  line, per image a "Reference Media" label part (the image's **exact lowest
  integer display ratio**, or "unavailable") immediately before the image
  part, and a final text part carrying the editing instruction (JSON string)
  and the user aspect-ratio selection (`adaptive` or a positive-integer
  `W:H`). Output: `{"expanded_caption": …, "target_aspect_ratio": "W:H"}` —
  an explicit user selection is copied exactly; adaptive resolves per the
  prompt's rules (main reference ratio by default).
- The PE is a **native VLM** (it has a vision tower + mmproj), so one model
  serves both workflows.

**Q4 GGUF of the PE** (hf `mradermacher/MiniMax-H3-Image-PE-GGUF`, static
quants of the makers' repo): `Q4_K_M` 16.5 GB (the pick — "fast,
recommended"; fits the card's 24 GB at moderate context with the 0.93 GB
f16 mmproj; Q8_0 28.7 GB cannot), `mmproj-f16.gguf`. No imatrix variants, no
separate MTP file. The Qwen3.8-27B base is a hybrid (16×(3 DeltaNet + 1 full
attention), 64 layers) whose KV is cheap; the pinned llama.cpp (52975342f)
carries the `qwen35` reader **with MTP (NextN) support** (`--spec-type
draft-mtp`, proven active on this build in the 13:33 window, E2B arm).

## The eviction dance (the 24 GB constraint)

PE Q4 (16.5 + 0.93) and the H3 DiT (~18.2–21) **cannot co-reside** on the card,
unlike every existing image door (the qwen/klein DiTs fit beside their PUPs).
So the service sequences one job across two gateway sets, and the matrix
performs the swaps — no new router machinery:

```
Prompt+ t2i/edit job:
  1. service → kokon-pup-h3-image (set h3p = ph & s & kv, bare)
       matrix evicts whatever holds the card (vllm / a DiT), boots the PE
  2. PE answers (caption + ratio)
  3. service → kokon-h3-image (set h3, bare)
       matrix evicts the PE, boots the DiT; job runs 6 steps
plain job (no Prompt+):
  1. service → kokon-h3-image directly
```

The PE door gets its **own bare set** (`h3p`), not a slot in the qwen/klein
sets: requesting the qwen PE doors would co-boot the qwen-turbo DiT (set
`qwen-t`), which is dead weight for an H3 job; and the PE's 17.4 GB does not
fit beside a qwen DiT anyway. One door, one identity (`kokon-pup-h3-image`,
gateway id = llama alias = service `model` field = boot-script name).

## Decisions (pre-logged)

- **D-041-1 (the PUP = the makers' PE fine-tune, Q4_K_M GGUF)** — owner
  ruling 2026-11-26 ("we need a gguf of the fine tuned PE"): not the base
  Qwen3.8-27B, not the 9B qwen PUPs. One door serves both t2i and edit (the
  PE is a VLM). MTP: enabled in the boot script **iff** the gguf carries the
  MTP module (inspect after download; the build supports it); otherwise off
  — the card's sampling row is the contract, MTP is a speed optimization.
- **D-041-2 (sample params respected at both layers)** — the DiT recipe
  (6/euler/cfg 1/sigmas `minimax-h3-image`) is already in the boot script
  (11-26) and is repeated in the service's per-model `sample` row
  (`{cfg:1, sampler:"euler", steps:6, clear_sigmas:true}` — the request must
  not carry a custom schedule, or the server's PDD schedule is replaced);
  the PE door's boot row is the card's T2I row (temp 1.0 / top_p 0.95 /
  top_k 20, thinking on, jinja), and the service additionally sends the
  per-mode output budget on the wire (t2i 4096 incl. thinking; edit 16384 —
  the makers released no edit row, the prompt-only status; overridable in
  the bundle manifest).
- **D-041-3 (sizes decided by code, per model)** — H3 gets its own
  `RecommendedSettings` row in the planner (code, not user config — the
  product rule): area presets **4 MP fast / 8 MP balanced / 16 MP quality**
  (the card's working range, best at 16), align 32 (VAE 16× × patch 2×2),
  max side 6272 (the card's 21:9 width), **max references 16** (the makers'
  max; the qwen default of 10 stays for the qwen models). The 16 MP quality
  preset stands or falls on the window's VRAM measurement (B2); a 12 MP
  fallback row is pre-decided if it OOMs.
- **D-041-4 (the PE's ratio drives the size — t2i only, auto selection only)**
  — per the PE card the returned `aspect_ratio` is *for image generation*:
  when the user left the aspect on auto, the decoded ratio re-resolves the
  planned size (same tier area, the card's aligned sizes where one matches);
  an explicit user selection always wins; edit and multi-reference jobs keep
  the planned size (the PE's `target_aspect_ratio` is recorded on the job
  view as the suggestion, same as qwen's `wh_ratio` today — changing canvas
  geometry after `prepare()` would break the reference mapping). A missing or
  unparseable ratio never fails the job (the planned size stands).
- **D-041-5 (the bundle is deployment data, not repo bytes)** — the three
  makers' prompt files (t2i system, t2i task, edit task) + the community
  LICENSE + a `manifest.json` (family `minimax-h3`, file paths, the makers'
  own sha256 manifest for hash verification at load, the per-mode sampling
  rows, source repo + sha) live in the deployment's data dir
  (`/data/assets/pup-h3/` on the dev box) — the `pup-qwen-native` precedent
  (third-party contract text is never committed; loader deals in keys and
  hashes only). A bundle without verified hashes or a LICENSE fails the job
  cleanly.
- **D-041-6 (A/B arms)** — the owner's "a/b test for quality" = the PUP
  choice, same H3 DiT, same prompt + seed: **arm A** the makers' PE door
  (the default once adopted), **arm B** the existing 9B qwen PE doors
  (the owner's first candidate; the doors and bundle already exist — the
  A/B is a config swap of the H3 entry's `vlm`/`vlmVision`). The window
  produces the images; the owner's visual verdict decides the default (the
  config makes either a one-field change).
- **D-041-7 (the dev box's missing qwen-native bundle)** — the 11-23
  clean-slate wiped `/data/assets/`, so **every** qwen Prompt+ job on the dev
  box is currently running the built-in fallback with a per-job note (the
  native decode never engages). Restore `pup-qwen-native` (rebuild from the
  PE model dirs' own `system_prompt.txt` files + LICENSE + manifest) before
  the A/B, or arm B is not a valid comparison.

## Plan

- **P1 (CPU, this repo)** — service plumbing: the `minimax-h3` bundle family
  (loader + hash verification + sampling rows), the H3 request builder
  (t2i: system + task with the format-placeholder splice; edit: the card's
  parts assembly incl. the per-image ratio metadata and the final part) and
  the decode (both envelopes, think-stripping, tolerant parse, one warm
  retry, out-of-contract note), the engine hooks (mode routing, post-enhance
  size re-resolution for auto t2i, ratio on the job view), the H3
  `RecommendedSettings` row, config validation, tests (loader, builder,
  decode, planner sizes, the mock-gateway harness entry).
- **P2 (box, ml repo)** — PE download (Q4_K_M + mmproj-f16, provenance.md
  with sources + sha256s); gguf inspection (arch, MTP presence); boot script
  (ctx 49152, q8_0 KV, image clamp 1024–4096, the card's row, MTP if
  present); gateway entry + `ph`/`h3p` matrix set (port map shifts;
  `.last-good` before the change).
- **P3 (dev box)** — the bundle in `/data/assets/pup-h3/` (+ the qwen-native
  restore, D-041-7); config.json picker entry; local image build + compose
  swap (ghcr push is still task-033's lapsed-credentials story).
- **P4 (GPU window, the autonomy protocol)** — job script (setsid + watcher +
  marker-last; vllm neutralized by the matrix itself — it is the evictee):
  PE boot + 1-token probe + a t2i PE call + an edit PE call (contract,
  timings); the **dance** end-to-end through the *service* (Prompt+ t2i:
  PE → evict → DiT → 16 MP or fallback; Prompt+ edit with references); the
  **A/B** (arm A vs arm B, same prompt + seed, t2i at the quality preset);
  vllm revived canonically before the marker. Digest + images in
  `_jobs/<window>/`.
- **P5** — docs (engineering-notes: the door, the dance, measured numbers;
  fresh-box: model list, matrix row, bundle acquisition), the two STATUS
  pages, close the task.

## Acceptance criteria

1. The picker lists **MiniMax H3 Image** (sub-line honest about the 16 MP /
   6-step character); selecting it sets the active pipeline (gateway model
   `kokon-h3-image`, PE door both VLM fields, bundle `pup-h3`) and a t2i job
   renders through it — proven in P4, not assumed from the config.
2. Sample params match the card at both layers: the DiT request carries
   6/euler/cfg 1 and **no custom sigmas** (the boot script's
   `minimax-h3-image` schedule runs — visible in the job's metadata and the
   gateway journal); the PE calls carry the card's row and the per-mode
   budgets.
3. The **dance** is proven through the service: a Prompt+ job shows PE
   evicting the previous GPU tenant, then the DiT evicting the PE, in the
   matrix journal; no OOM, no double-boot; a plain job never touches the PE
   door.
4. Prompt+ **edit** works end-to-end (references reach the PE with their
   ratio metadata, the DiT gets `expanded_caption` + the references) and the
   **t2i auto-ratio** behaviour of D-041-4 holds (PE ratio drives the size
   when auto; user selection wins; edit records the suggestion without
   moving the canvas).
5. **A/B images** (arm A makers' PE vs arm B 9B qwen PUP, same prompt +
   seed) in the window digest for the owner's verdict; the config makes the
   winning arm a one-field change.
6. Tests: `cargo test -p qls-core` + `npm test` green (new coverage: bundle
   family, request builder, decode, sizes, ratio application); the
   mock-gateway harness includes the new model.
7. The 16 MP quality preset is measured on the card (or the pre-decided
   fallback is in place and noted); fresh-box/STATUS updated with the new
   door, set and bundle.

## Blockers

- **B1 (resolved by inspection, P2)**: whether mradermacher's PE gguf carries
  the MTP module (no separate MTP file in his repo; unsloth keeps it apart).
  MTP on iff present — the build supports it; no code change either way.
- **B2 (window measurement, P4)**: 16 MP on 24 GB with the Q4 DiT. The 13:31
  window peaked at **18.2 GB at 1024²** (4k tokens); 16 MP is 16k tokens of
  extra activations. If it OOMs, the quality preset drops to 12 MP
  (D-041-3's pre-decided fallback) — the card's own `min_generation_area`
  machinery means no model-side change.
- **B3 (ops)**: the window evicts vllm (the agent's own runtime) for its
  duration — the owner's sessions stay on qwen-direct; the window runs under
  the autonomy protocol (blocking wait as the last step of the launch call).

## Verification

(filled in as the phases land)
