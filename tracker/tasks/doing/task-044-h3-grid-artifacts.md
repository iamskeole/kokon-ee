# task-044: H3 16 MP output artifacts — the VAE decode tiling (and the FA/sampling audit)

Opened 2026-11-27 from the owner's on-box review of H3 outputs (attached images):
4K generate and 4K edit show a visible square **grid** (strongest on
high-frequency detail like fur), the edit additionally shows **colour
dodge/burn/bleed**; 1 MP (generate and edit) is clean, on par with qwen.
Owner's stance: willing to cap the resolution, but only after a thorough
investigation — "16 MP native is THE maker's selling point … i doubt
they're marketing something broken". The owner also asked (same review):
flash attention appears off in the H3 boot script — is that wrong, slow,
or an artifact source? Audit every sampling param vs the maker's
recommendation.

## Symptoms (owner's on-box, 2026-11-27 ~12:11, all H3)

1. The action-row **Paint** button is still active when editing an image
   with H3 (042 gated the composer's brush; this is a second entry point
   in `PhoneStage.tsx`).
2. 4K (3264×4096) **generate**: small square grid, composition fine.
3. 4K **edit**: grid more visible than generate, plus colour dodge/burn.
4. 1 MP (1024²) generate **and** edit: no grid (edit has mild colour only).

## Investigation (code level, 52491bd tree + model card) — done 2026-11-27

- Our H3 VAE file (`minimax_h3_image_vae_fp16.safetensors`) is the
  **original** variant (`decoder.transformer_blocks.*`): a 24-layer 2D-ViT
  decoder, 16× spatial (4096² → 65,536 latent patches + 4 register tokens).
- 52491bd **force-enables** spatial tiling for the original H3 VAE
  (`resolve_original_tiling_params` in `minimax_h3_vae.hpp`: even with no
  flag, `enabled=true`, 256×256 px tiles; with `--vae-tiling` as our door
  passes, the CLI default `target_overlap 0.5` applies — a 128 px
  smootherstep crossfade, so no hard seams).
- Each 256 px tile is 256 tokens + 4 registers decoded through 24 attention
  layers **with no cross-tile context**: a 16 MP image is 256 such tiles,
  each seeing 1/256 of the decoder's context. The blend hides the hard
  seam; the per-tile texture/colour "mood" drift is the visible grid.
  1 MP is 16 tiles (each a quarter of the image) — too coarse and soft to
  read as a grid. The card's sdcpp examples are all 1024², so the tiling
  was never exercised at 16 MP by the doc's authors.
- Flash attention: the door's `--flash-attn off,diffusion=on` is exactly
  the new-tree form of the doc's `--diffusion-fa` (mapping verified in
  `generation_program.cpp`: the deprecated flag resolves to
  diffusion=on, everything else off). So **DiT runs FA-on** (O(N) — the
  ~40 s 16 MP DiT pass is the FA regime; no speed problem there) while
  **the VAE runs FA-off** → naive O(N²) attention. A single full-image
  4096² tile would need a 65,540×65,540 score tensor (~17 GB) — infeasible
  next to the 18.2 GB of weights on the 24 GB card. The forced 256-px
  tiling is therefore the *only* decode path with FA off, and it is the
  grid's cause. The grid is not an FA numeric artifact; it is the tiling
  that FA-off forces. Enabling FA on the VAE (`vae=on`) makes the
  full-image, full-context (makers') decode memory-feasible (O(N) KV,
  streamed through the graph cuts).
- Sampling audit vs `docs/models/minimax_h3_image.md` @52491bd (the tree
  our door runs): cfg 1.0 ✓ · euler ✓ · 6 steps ✓ ·
  `--sigmas minimax-h3-image` (the 6-step schedule; the default discrete
  scheduler yields the same at 6 steps) ✓ · `--rng cpu` ✓ ·
  `--offload-to-cpu` ✓ · P-H3-Image INT8 ConvRot text encoder (the doc's
  recommended small encoder, ~5 GB, works for t2i and reference editing)
  ✓ · the release fp16 VAE ✓ · 1024² fallback ✓ · `--vae-tiling` ✓ (as
  doc) · `--flash-attn off,diffusion=on` ≡ the doc's `--diffusion-fa`
  ✓. Box-only additions: `--eager-load`, `--max-vram 24`,
  `--backend/--params-backend` cuda0 pins, `--cors-origins`, `--seed -1`
  — all previously measured and justified (041/039). **Nothing deviates
  from the makers' recipe.**
- The makers' own 16 MP pipeline (card's Full Workflow) = bf16 DiT +
  full-attention VAE on big GPUs via SGLang — no tiling. The 16 MP spec
  is sound; the artifact is our 24 GB tiling regime.

## Fix hypothesis (the window confirms or kills it)

Boot the door with `--flash-attn off,diffusion=on,vae=on` and
`--vae-tile-size` as large as fits (4096 = a single full-image tile on
any size we serve — the VAE clamps the tile to the image, so 1 MP/2 K
become single tiles too): the makers' decode at every size. Fallback
ladder: the largest tile that completes 4 K without OOM (4096 → 2048 →
…); the build halves tiles automatically on an allocation failure.

## Pass 1 results (2026-11-27 11:18–11:55 UTC, the first window run)

- A1 (256) / A2 (512) / A3 (1024) 4 K t2i, seed 41, FA off: all completed in
  221–222 s, VRAM peak 22.3 GB (the wall is flat from 256 to 1024 — the
  attention work is still small in that regime).
- **A4 (2048, FA off) never finished: >25 min and still generating** (the
  naive 16k-token per-tile attention makes 2048-px tiles infeasible in this
  regime — the FA-off regime has a wall between 1024 and 2048). A4/A5 are
  dropped from stage A on purpose.
- Full-res crop comparison (the cat's chest, the high-frequency region the
  owner flagged): A1 (256) looks smooth in that crop; A2 (512) and A3 (1024)
  show a **fine pixel-grid mesh** (a few px period, over the white fur) —
  strongest at 1024; the owner's own 256-tile 3264×4096 image shows the same
  mesh **plus a hard tile-boundary step** (a straight vertical line at a
  colour edge). The 1 MP control shows no mesh.
- So the visible artifact is a **fine mesh in high-frequency detail** (not
  only the per-tile mood drift), and it is *not* monotonic in tile size —
  which points at the VAE decode itself (the fp16 305 M VAE's 16× upsample on
  high-frequency content, and/or the per-tile decode phase), not purely at
  the attention window. The decisive test is stage B: the same seed at full
  tile (the makers' full-context decode). If the mesh survives there, it is
  a property of this VAE build on this card (the makers' SGLang pipeline is
  bf16; the release VAE here is fp16), and the fix space changes (bf16 VAE
  variant / accept / cap); if it goes away, tiling is the cause and the
  largest-fitting-tile door is the fix.
- The window ended FAIL on the A4 poll budget (by construction) after
  restoring the runtime (sweep + canonical vLLM revival, 23520 MiB) — the
  protocol behaved; pass 2 is the restructure below.

## Pass 2 results (2026-11-27 12:12–12:43 UTC) and what killed the session

- A6 (2K t2i, 256 t, seed 42) completed in 51 s, peak 20,164 MiB, mottle
  0.6012; A7 (1 MP) re-ran in 10 s, 18,246 MiB, 0.8079 — both consistent
  with pass 1: small sizes stay clean.
- A8 (4K edit, 256 t, seed 44, the owner's 4K cat `100832` as reference,
  the owner's exact instruction) was submitted at 12:13:37 and never
  completed. Two independent events, both in the daemon journal:
  1. **The door process died at 12:13:51** — 14 s in, as the 3264×4096
     reference encode began (VRAM had climbed to 23,842 MiB on a 24,145 MiB
     card: a reference-encode OOM, the edit path's first heavy allocation).
     The daemon re-ran the boot script 6 s later (12:13:57, "booting
     sd-server :9112") — v269's on-demand revival of a dead registry
     model, proven again on the edit path. (The 12:13:49 `gdb` attach in
     the journal was the previous session investigating the death, not a
     cause.)
  2. **vLLM reappeared at 12:38** (17 min in) — the previous session
     itself was routed *through the gateway* (`127.0.0.1:8081/v1`, the
     journal's `pi/1.4.0` completions) with a ~142 K-token context: its
     compaction attempts each booted vLLM, and each boot's set conflict
     evicted the H3 door, orphaning A8. This is the D-027-10 failure mode
     (a long job's window must not share the gateway with a live session),
     which the autonomy protocol exists to prevent. The job's yield rule
     caught it (12:42, "a vLLM process is running (reclaim) — left
     untouched"); the protocol behaved, vLLM (23.8 GB) stayed up, marker
     last.
- The window's own bugs, recorded and fixed in the pass-3 rewrite: (a) the
  preflight's `caps=200` readiness check is a **phantom** — the door
  answers `/capabilities` before its weights are on the card (pass 2 logged
  "ready: caps=200 vram=4 MiB"); (b) `write_digest` was defined after its
  first use, so the FAIL path never wrote a digest; (c) the poll loop kept
  a 404'd job's stale `generating` state (the job's json is only refreshed
  on HTTP 200).
- The door script was never swapped (the yield ended the run before stage
  3); it is byte-identical to its pre-window state (verified:
  `--flash-attn off,diffusion=on`, `--vae-tiling --vae-tile-size 256`), and
  vLLM is the sole GPU tenant again.

## Pass 3 result (2026-11-27 15:09–15:16 UTC): a false FAIL on two job bugs

The window **loaded and ran the door for the whole 6-minute window** and
still ended FAIL — the box was never at fault:

- The first caps probe **did** trigger the on-demand load (journal 15:09:14:
  `matrix: model=kokon-h3-image set=h3 dsl="h3 & s & kv"
  evict=[qwen3.8-27b] target=[kokon-h3-image kokon-vision sidekick] cost=1`;`
  sd-server` booted :9112; the daemon's health check —
  `GET /sdcpp/v1/capabilities` — passed at 15:09:17, 2 s after boot).
- But the job polled `/upstream/kokon-h3-image/capabilities` — the pass-3
  rewrite dropped the `/sdcpp/v1/` prefix that pass 2 had. That is **not a
  door route**: the door answered its own 404 (empty body, ~400 µs) for
  7.5 min, the job read it as "not ready", and the FAIL-path sweep unloaded
  the healthy door at 15:15:18.
- The `vram > 15 GB` readiness gate was also wrong for this door: it runs
  `--offload-to-cpu`, so an idle-loaded door sits at **~4 MiB VRAM**
  (weights on CPU, moved on demand) — pass 3 read `vram=4` for the entire
  6-minute load window. This reinterprets pass 2's "phantom ready
  (caps=200 vram=4)": that was the normal loaded state, and that door served
  A6/A7 fine. The daemon's own health check is a weak 200 (it passes 2 s
  after boot, possibly before the model is fully resident); the correct
  gate is the capabilities body reporting the H3 DiT by name
  (`model-Q4_K_M` — the service's own model-match logic).
- The FAIL path then behaved as written: sweep → canonical vLLM revival
  (926808, 1-token probe 200 at 15:16:31; a transient CUDACachingAllocator
  warning during vLLM's boot while the door's memory freed is harmless)
  → marker last. vLLM has served the box continuously since; the door
  script is byte-identical to its pre-window state (pass 3 failed before
  the swap). No stage-B data was produced.

## Pass 4 results (2026-11-27 15:56–16:33 UTC) — the decisive stage B, third attempt

The window ran to a **YIELD-DONE on its own terms** (the runtime was left as
found: vLLM up — the owner's own message to the assistant at 16:33:12 booted
it through the gateway, which evicted the *idle* H3 door; the job's yield
rule ended the run cleanly, restored the door script byte-identical, and
wrote the marker. The gateway did exactly what it should; owner confirmed —
not a failure mode, no chasing).

Same window shape as the D-044-6 ruling (blocking wait, no neutralisation,
yield on any vLLM reappearing — the watcher's 3 h cap writes TIMEOUT and
leaves a live job running; it never kills mid-generation). Built on the
proven pass-2 job (the stage-A outputs it compares against live in
`models-044-h3artifacts`), with the pass-3 lessons:

- **Readiness gate**: `GET /upstream/kokon-h3-image/sdcpp/v1/capabilities`
  = 200 **and** the body names the H3 DiT (`model-Q4_K_M`); no VRAM gate
  (offload-to-cpu). The first request is the load trigger (the 15:09:14
  precedent). B6 (1 MP, ~10–30 s) is the acceptance proof with one retry
  after a 30 s fresh-boot settle.
- **The A8-class OOM risk is managed, not hoped away**: B4 (4K edit, 4K ref)
  and B8 (3264×4096 edit, 1MP ref) are tolerant — an OOM or a slow run
  fails their 30-min budget and the window continues with the t2i grid (the
  arbiter). The mid-run poll loop also breaks early if vLLM appears (yield
  after the run, never a kill-veto).
- **Door-crash resilience** as in pass 3: if the door dies mid-run while
  vLLM is out, wait for the daemon's auto-reboot (proven 12:13:57) and
  resubmit (≤ 3 attempts); if a *different* GPU door is up instead —
  someone evicted us on purpose — YIELD.
- **Runs, same body.py prompts/references/seeds as stage A** (so A↔B pairs
  compare identical content): B6 1 MP full tile → B5 2 K full → B2 4K @
  256 t / B3 4K @ 512 t (same seed 41 as A1/A2 — isolates the FA numeric
  effect) → B1 4K @ **4096 t (the makers' full-context decode)** → B7
  3264×4096 @ 4096 t (the owner's exact shape) → B4 4K edit @ 4096 t /
  B8 3264×4096 edit @ 4096 t (1MP ref, the lighter encode).
- **Measurement-only ending (D-044-5)**: door script restored
  byte-identical (verified by `cmp` against the pre-window backup on every
  exit path); `decision.txt` + `script.proposed` (vae=on + the largest tile
  that completed a 4 K run) + a contact sheet of chest crops (owner 4K t2i /
  owner 1 MP / A1 / A3 / B1 / B2 / B4 / B7, whichever exist) for the owner
  as arbiter. The mottle metric is *not* a grid detector (1 MP is visually
  clean at 0.81 while 4K-256-t is 0.52 with the visible mesh) — the contact
  sheet is the arbiter, the metric set is for trend tracking.
- **Preflight** also asserts the door script is the expected baseline
  (FA-off, 256 tiles) before the window starts — if the box drifted, it
  fails fast instead of producing an uninterpretable A/B.

**What actually happened (the record):**

- **All six t2i runs completed** on the `vae=on` door (swapped at 15:56,
  the makers' flash-attention decode): B6 1 MP full tile 20 s / 18.4 GB,
  B5 2 K full 40 s / 20.2 GB, B2 4K @ 256 t **223 s** / 22.3 GB, B3 4K @
  512 t 213 s / 22.3 GB, **B1 4K @ 4096 full-tile 222 s / 24.0 GB**, **B7
  3264×4096 @ 4096 full-tile 162 s / 23.0 GB** (the owner's exact shape;
  the full-context decode is *not* slower than the tiled runs and peaks
  23.9–24.0 GB — it fits the card, with ~200 MiB to spare).
- **B4 (4K edit, 4K ref) died in 2 s** — submitted 16:11:44, the door
  process exited 16:11:46: the 4096² reference encode OOMed (the A8-class
  risk, exactly as predicted). The daemon auto-rebooted the door 8 s later
  (16:11:54, health passed 16:11:55) but B4's job id was gone with the
  process, so the poll loop got 404s for the rest of its 30-min budget
  (16:12–16:33) while the card sat idle (the rebooted door idles at ~4 MiB
  VRAM — offload-to-cpu). **B8 (1MP-ref edit) never ran** (yield first).
- **The edit colour question (dodge/burn/bleed) therefore remains
  unmeasured**; B8 (the lighter encode) is the one missing data point and
  would be a ~5–10 min single-run window if the owner wants it. B4 is not
  runnable on this card as configured — a 4K-ref encode does not fit a
  24 GB card next to the DiT.
- **Metrics (score.py; the mottle family is trend-only, not a grid
  detector):** B2 (256 t, FA-on) is *bit-identical* to A1 (256 t, FA-off)
  — mottle 0.5196 / aligned 0.953 / acf 0.3417 / fft-period 2.8 — and B3
  (512 t, FA-on) matches A2 (0.7291 / 0.911 / 1.7672). **Turning the VAE's
  flash-attention on changes nothing measurable in a tiled decode** (same
  seeds, same content). A3 (1024 t, FA-off) 0.7117. Full-tile runs (B1,
  B5, B6, B7) have no tile to comb against — the metrics are null by
  construction; the eyes are the arbiter.
- **The evidence for the owner's call** (chest/fur crops, one shared crop
  box, all 8 panels): the contact sheet `165500-task044-h3-vae-decode-contact-sheet.png`
  is in the 11-27 gallery (browser), with the full-size originals in this
  job's `outs/` (B1/B7 4K + the owner's 4K/1MP) and stage A's `outs/`
  (A1/A2/A3). Row 1 = tiled decode (256/512/1024 FA-off, 512 FA-on);
  row 2 = the makers' full decode (B1 4K, B7 the owner's shape) beside
  the owner's reported 4K and the clean 1 MP control.
- **Agent's own read (for the owner, not a verdict):** in the 640-px crops
  the full-tile decodes do **not** look dramatically cleaner than the
  tiled ones or the owner's 4K — the fine fur texture is visible in all
  panels; the 1 MP control reads smooth but was upscaled from 327 px. If
  the owner's eyes at full resolution agree, the grid is a property of the
  fp16 24-layer 2D-ViT decoder itself (16× upsample on high-frequency
  content) rather than of tiling or of FA — which would make it a model/
  engine property, outside this window's measurement-only mandate.
- **The score.py fix** (the `acf_comb` None-crash on full-tile images,
  `round(None)`) is in this job's copy; pass-2's copy keeps the old bug
  (harmless — A-side scoring always had tile < width).
- **Two job improvements recorded for the next window** (neither was needed
  for the data we have; both cut wall time): (1) a door crash mid-run loses
  the job id — on N consecutive job-404s the run should **resubmit** the
  same generation (the daemon reboots the door in ~8 s, proven here at
  16:11:54) instead of polling a dead id for 30 min (the 16:12–16:33 idle
  gap the owner caught); (2) the window's *own* poller is a gateway client
  — any concurrent client (the owner's assistant, a compaction) can boot
  vLLM and YIELD the run, which is correct behavior but burns a 37-min
  window's setup; a shorter edit run order (B8 first, before the long t2i
  runs) would make a future edit-only window ~10 min end-to-end.

## Plan

(a) [done, committed] Hide the action-row **Paint** button for no-inpaint
    picker entries — 042 follow-up; `entrySupportsInpaint` on
    `PhoneStage.tsx` (tsc + 251 UI tests green).
(b) Window `models-044-h3artifacts` (H3 DiT door only; the PE is not
    involved — the artifact is a DiT/VAE-side phenomenon, fixed prompts):
    - **stage A** (door as-is, VAE FA off): 4 K t2i at 256 / 512 / 1024 /
      2048 / 4096 tile (same seed 41; the 4096 run is expected to fail —
      the FA-off ceiling, recorded as data), 2 K and 1 MP controls, 4 K
      edit at 256 / 512 (same seed 44; the reference is the owner's own
      4 K cat `100832`, prompt their own "make the cat an orange tabby")
    - **door swap** to the `vae=on` variant (script backup → sed → `sh -n`
      → gateway unload → on-demand reload)
    - **stage B** (VAE FA on): 4 K t2i at 4096 (full) / 256 / 512 (same
      seeds as the stage-A counterparts — isolates the FA numeric effect
      from the tile size), 4 K edit at 4096, 2 K and 1 MP at full tile,
      plus the owner's exact 3264×4096 shape for t2i and edit at full tile
    - **objective grid score** on every output: block-mean mottle (per-tile
      colour/texture drift), grid-aligned boundary contrast (tile-aligned
      vs half-tile-offset), and a 2-D FFT dominant period when numpy is
      available
    - **recommendation, not application (D-044-5, revised)**: `decision.txt`
      + `script.proposed` (vae=on + the largest tile that completed a 4 K
      run) + a full-res contact sheet of the chest crops (owner 4 K, owner
      1 MP, A1, A3, B1, B2, B4, B7) for the owner's eyes; the live door
      script is restored from its pre-window backup in every case
(c) If a fix lands: the door is a script (no rebuild) — the next H3 load
    picks it up; update the door header comment, the ml provenance note,
    and `docs/engineering-notes.md` with the measured result.

## Acceptance

- [x] The Paint button is hidden for H3 (and any no-inpaint entry) —
      committed `6f7868f`; lands on the box with the image rebuild this
      window runs first
- [x] Service image rebuilt from HEAD and re-deployed on the dev box, a
      1 MP H3 generate through the deployed service clean (2026-11-27,
      `kokon-ee:dev-h3` `c9e35bb5321a`; job `job_d5a7faea2f60`, ~48 s, HTTP
      200 — the smoke test of the pass-3 window turn)
- [ ] Window digest: per-run wall / VRAM peak / metric set, stage A vs B,
      door-crash / OOM journal evidence
- [ ] The pass-4 window completes (or yields) with the runtime restored
      before its marker
- [ ] A root-cause statement backed by the measurements (tile-bound vs
      VAE-decode-bound, per the B1 full-tile result)
- [ ] A decision (D-044-5): `decision.txt` + `script.proposed` + contact
      sheet for the owner's call — apply / cap / accept
- [ ] Docs / status updated; service repo and ml repo committed (unpushed)

## blockers

(none — the 24 GB card is shared with vLLM; the gateway's bare-set
conflict evicts vLLM when the door loads, and the window YIELDS if vLLM
reappears on its own mid-window)

## decisions

- **D-044-1** The grid is per-tile VAE-decode context fragmentation on the
  original H3 VAE (256-token attention windows in a 24-layer ViT), not a
  sampling-parameter error; the makers' recipe is followed
  flag-for-flag (the audit above).
- **D-044-2** The experiment uses the sdcpp API's per-request
  `vae_tiling_params` override (a feature 52491bd advertises for img_gen)
  so the door boots once per stage; only the A→B swap touches the script.
- **D-044-3** Every tiling override pins `target_overlap 0.5` (the CLI
  default the door already runs) so only the tile size varies between
  runs; baseline runs send no tiling field at all.
- **D-044-4** The edit reference is the owner's own 4 K cat
  (gallery 2026-11-27 `100832`) with their exact instruction
  ("make the cat an orange tabby") — a direct reproduction of their
  Image 3; fixed seeds per size across stages.
- **D-044-5 (revised after pass 1)** The window is **measurement-only**:
  it never applies a door change. The artifact is visible, and the
  objective metrics (tile-lag autocorrelation comb, residual mottle) do not
  separate the classes reliably enough to stand behind a config change —
  the owner's eyes on the contact sheet are the arbiter. The window
  produces the recommendation (`decision.txt` + `script.proposed`: the
  vae=on variant with the largest tile that completed a 4 K run) and always
  restores the live door script from its pre-window backup.
- **D-044-7 (measured 2026-11-27, pass-3 post-mortem)** the H3 door's
  readiness is **not** readable from VRAM: `--offload-to-cpu` keeps an
  idle-loaded door at ~4 MiB (weights on CPU, moved per request — pass 3
  read 4 MiB across a whole 6-minute load, and the pass-2 "phantom ready"
  was the normal state). The gate is the capabilities body naming the DiT
  (`/sdcpp/v1/capabilities`, the daemon's checkEndpoint — note the
  `/sdcpp/v1/` prefix; `/capabilities` without it is a door 404). A bare
  200 is weak (the daemon's health check passes 2 s after boot); a real
  cheap run (B6) is the acceptance proof.
- **D-044-6 (owner, 2026-11-27, on the pass-3 window mechanics)** the
  blocking-wait form is the only option — the gateway's eviction dance
  *is* the design: the launching session suspends in the wait and makes
  no gateway calls, vLLM is evicted by the first H3 request and revived
  by the job at the end (or by the next session turn through the
  gateway). No registry neutralisation, no config swap, no direct-port
  re-routing. Rationale: the previous session's death came from a *live*
  large-context session making gateway requests, not from the daemon's
  revival machinery — a suspended session removes the failure vector
  without surgery on the routing matrix (which carries the owner's
  uncommitted vision-slot WIP that a swap would have to preserve).
