# Task 041 — wire MiniMax H3-Image as a new selectable model (service plumbing + the makers' PE PUP door)

**Status: done (created 2026-11-26, owner brief; closed 2026-11-26 after the
round-2 validation window and the qwen-PUP A/B window).**

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
  **Resolved 11-26: the Q4_K_M gguf carries no MTP module** (`llama-gguf`: no
  `n_layer_nextn`, no nextn tensors); architecture `qwen35` (Gated DeltaNet),
  natively supported by the pinned build — the boot script runs without
  `--spec-type draft-mtp`, and the CUDA SSM kernels in the build
  (ggml-cuda/ssm-scan.cu, ssm-conv.cuh) keep the hybrid's recurrence on GPU.
- **B2 (window measurement, P4)**: 16 MP on 24 GB with the Q4 DiT. The 13:31
  window peaked at **18.2 GB at 1024²** (4k tokens); 16 MP is 16k tokens of
  extra activations. If it OOMs, the quality preset drops to 12 MP
  (D-041-3's pre-decided fallback) — the card's own `min_generation_area`
  machinery means no model-side change.
  **Resolved 11-26 (round-2 window, direct door, the card's 6-step recipe):**
  2048×2048 completed, peak **19180 MiB**; 4096×4096 (16.8 MP) **completed**,
  peak **22152 MiB** — 2.4 GB under the card, no OOM, no fallback. The 16 MP
  quality preset **stands**; the pre-decided 12 MP fallback was not needed.
- **B3 (ops)**: the window evicts vllm (the agent's own runtime) for its
  duration — the owner's sessions stay on qwen-direct; the window runs under
  the autonomy protocol (blocking wait as the last step of the launch call).
  **Resolved 11-26:** both windows ran under the protocol and ended with the
  daemon-managed vLLM revived (23520 MiB, gateway probe 200) and no stray
  engines. One mid-run incident (an orphan vLLM from a manual exec of the 033
  script, which unconditionally `exec`s — no attach logic) desynced the
  daemon's matrix and killed the first H3 DiT boot; the standing rule is
  D-041-10/14 — vLLM revival is always a keyed gateway request through the
  daemon, never a script exec.

## Decisions (continued, from the validation windows)

- **D-041-21 (11-26, arm B is unreachable as a picker entry; the A/B verdict)** —
  the service keys the pipeline's PUP on the *running DiT's model id*
  (`models.iter().find(|m| m.model == model)`), so two picker entries sharing
  the H3 DiT id always resolve to the first (arm A, the PE) — the round-2
  window proved it: arm B's jobs carried the PE's exact captions (identical
  enhanced_chars) and both arms' gallery images are **pixel-identical**
  (same caption, same seed 42; only the PNG metadata differed). A picker
  entry cannot therefore A/B the PUP for one DiT id, and the arm-B entry was
  removed from the deployment config (dead, misleading config). The A/B was
  completed instead by asking the 9B qwen doors the same questions directly
  (window `models-041-qwenpup`, the service's own request shape): t2i
  15 s / 1228 gen tokens → 2844 thinking chars + a 3219-char caption in the
  **Qwen-2.1 contract** (`{"rewritten_prompt": …}`); i2i 24 s / 1307 tokens →
  5178 thinking + a 693-char caption, same contract. The PE (27B): 33–38 s,
  the H3 envelope (caption + 10-value ratio / W:H target). **Verdict: for H3
  the makers' PE is the only PUP that speaks the H3 caption contract the DiT
  is conditioned on; the 9B qwen doors speak the Qwen-Image-2.1 contract**
  (fast, but their captions are not what the H3 DiT expects — through the
  service's H3 path `decode_h3` would fall back to raw text after retries).
  The 9B doors keep their native role in the Qwen 2.1 pipeline.
- **D-041-22 (11-26, where Prompt+ is actually switchable from the API)** —
  the thin `/generate`/`/edit`/… endpoints take `SimpleOp`, which has no
  `enhance` field (`..Default::default()` → false): an `"enhance": true` in
  those bodies is silently dropped and the job runs raw (the round-1 window's
  `enhanced_chars: 0` — HTTP 200, job done, no PUP). The production path is
  the one the browser uses: `POST /api/v1/jobs` with the full
  `GenerationRequest` (`enhance`, `enhance_only`, `aspect_ratio`, …), then
  long-poll `GET /api/v1/jobs/{id}/wait?timeout=≤600`. Validation and any
  script/MCP consumer that wants Prompt+ must use `/jobs`, not the SimpleOp
  endpoints. (Behaviour, not a bug: the SimpleOp endpoints predate the
  enhance flag and stay deliberately minimal.)
- **D-041-23 (11-26, the 16 MP tier stands on its measured peak)** — the
  Qwen-calibrated token budget (`token_budget` + the large-GPU bonus) clamps
  a 24 GB card to ~6.4 MP, which would have left H3's 16 MP quality tier
  unreachable even though the measurement shows it fits (22152 MiB peak,
  B2). `RecommendedSettings` gained `measured_peak_mib` (the largest tier's
  measured full-run peak + 1400 MiB headroom; set only on the H3 row:
  `23_552`): a card that holds it keeps every **single-image** H3 tier
  (4/8/16 MP); reference-bearing jobs and unknown-VRAM boxes keep the shared
  budget (the measured run was single-image; multi-reference 16 MP is not
  measured and stays clamped). Verified live: `/plan` on this box now
  resolves quality to 4065×4065 (16.5 MP, under the 4096 side cap) with no
  reduction note.

## Verification (closed 2026-11-26)

- **PE door (P2)** — Q4_K_M 16.5 GB + mmproj-f16 0.93 GB in
  `models/gguf/MiniMax/MiniMax-H3-Image-PE/`; boot script
  `serve-kokon-pup-h3-image.sh` (ctx 32768, `--reasoning on`, temp 1.0 /
  top_p 0.95 / top_k 20, jinja, `-ngl 99`, **no MTP** — B1); gateway id
  `kokon-pup-h3-image` with its own bare set `h3p: ph & s & kv`. Cold boot
  ~17–25 s through the matrix; warm door ~26 tok/s generation (llama-server
  timings: prompt 1021 tok/s, gen 26.2 tok/s, graphs reused, 0 reallocs),
  ~19.4 GB resident.
- **Contract (P4, round-2 window)** — makers' T2I turn: 33.2 s, 774 tokens
  (1941 thinking + 1537 reply), envelope ok, aspect `3:2`, 1490-char caption.
  Makers' EDIT turn (one user message, no system — the corrected contract):
  37.7 s, 777 tokens, envelope ok, `target_aspect_ratio 1:1`.
- **Dance through the service (P4)** — every Prompt+ job in the round-2
  window shows in the gateway journal: the PE request evicts the previous
  tenant (vLLM) and boots the PE; the DiT request evicts the PE and boots the
  H3 DiT; no OOM, no double-boot. Service t2i (arm A, `enhance: true` via
  `/jobs`): done in 102 s, the **1487-char enhanced caption became the DiT
  prompt**, the PE ratio `3:2` drove the size (2496×1664, note "The
  upsampler's 3:2 choice sizes this image (your aspect is auto)") — D-041-4
  proven live. Service edit: done in 141 s, 1424-char enhanced caption,
  suggestion `1:1`, canvas kept (512²).
- **403 origin gate (the 52491bd break)** — the build's server added an
  origin/Host gate (examples/server/server_access.cpp); the gateway's
  reverse proxy preserves the client `Host` (`apollo.local:8081`), which the
  upstreams rejected with `Request origin is not allowed`. All four DiT boot
  scripts now pass `--cors-origins http://apollo.local:8081,https://…`;
  capabilities through the named-host route return 200 in ~5 s.
- **A/B (D-041-21)** — picker arm B unreachable (pixel-identical proof);
  direct qwen-door calls answered (numbers above). Verdict recorded; the
  deployment picker keeps one H3 entry (the PE) — the 9B doors stay in the
  Qwen 2.1 pipeline's entry.
- **B2** — 16 MP stands (above); the planner encodes it as D-041-23.
- **Tests** — `cargo test -p qls-core` 396 passed / 0 failed / 2 ignored;
  `npm test` 245 passed; `mcp` 49 passed / 3 skipped. New coverage: bundle
  family + hashes, request builder (t2i splice, edit parts, no-system edit),
  decode (both envelopes, think-strip, tolerant retry, out-of-contract note),
  H3 planner sizes (tier areas, 4096 side cap, the measured-peak waiver),
  engine hooks (whole-caption DiT prompt, auto-ratio re-resolution, the
  900 s PE clock).

## Verification

(filled in as the phases land)
- **D-041-17 (11-26, the 400 on H3 edits)** — the 52491bd build removed
  per-request reference overrides from the sd-cpp API: `image_preprocess`,
  `ref_image_args` and `increase_ref_index` in the img_gen JSON now throw
  ("...requires a DGL script" / "reference overrides require a DGL script",
  examples/common/common.cpp:1998-2004) → 400 "invalid generation
  parameters". Reference handling is definition-owned per model
  (generation_image.cpp: the H3 definition resizes with lanczos + augment +
  reference-noise blocks; qwen with bilinear; `init_image`/`ref_images`/
  `mask_image` are still request fields). The service therefore stops
  sending those fields (the 395/245/49 tests pass without them); qwen/flux
  edit/inpaint lose the `resize_before_vae=false` override — the definition's
  resize applies (stock-build behaviour, not overridable from the API).
- **D-041-18 (11-26, why every H3 job was 1760²)** — the service container
  has no nvidia-smi, so the planner's GPU detection is blind and falls back
  to the unknown-VRAM budget (12288 tokens × 256 px = 3 MP → 1760²): fast
  (4 MP) and quality (16 MP) were both clamped to the same 1760². The
  deployment fixes it with the documented escape hatch for split boxes:
  `vramOverrideMib: 24576` in config.json (the accelerator's total; the
  override path skips detection entirely). On 24576 the budget is 22528
  tokens ≈ 5.8 MP, so balanced and quality converge there until the
  measurement window decides the quality tier (B2).
- **D-041-19 (11-26, the mixed active state)** — `absorb_secrets` rule (4)
  inherits the stored `vlm`/`vlmVision` whenever an incoming config names
  none (stock 0.12 UIs carry no such fields; an explicit `null` is
  indistinguishable from absent through `Option<T>`), so switching from
  arm B back to arm A stranded the 9B qwen i2i door as arm A's `vlmVision`
  and H3 edits would have routed to the wrong PUP. Fix is deployment-side,
  matching the rule's own invariant (a named door is explicit state): arm
  A's picker entry now names the makers' PE door for **both** `vlm` and
  `vlmVision` — the PE serves both modes (the request shape is mode-driven,
  not door-driven), so the vision door falls back to nothing it needs.
- **D-041-20 (11-26)** — H3 `max_side` 6272 → 4096: the card's 21:9 examples
  reach 6272, but the 52491bd build caps the door at 4096 per side
  (capabilities' limits), and the planner must not hand the door a size it
  rejects (the explicit width/height validation uses the same bound).
