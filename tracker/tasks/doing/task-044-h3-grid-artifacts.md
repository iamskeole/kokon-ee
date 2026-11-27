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

## Pass 3 (running now) — the decisive stage B, rebuilt after pass 2

**Window mechanics (D-044-6, the owner's ruling)**: the blocking-wait form
only — no registry neutralisation, no config swap, no direct-port
re-routing. The launching session ends its turn in `wait-marker.sh` and is
*suspended* for the whole window: it makes no gateway requests while it is
out, so it cannot evict the door (pass 2's death came from a live
142 K-context session making gateway requests — a suspended session by
construction does not). vLLM is evicted by the first H3 request and stays
out for the window; any vLLM that reappears mid-window (a human reclaim —
a manual boot or another client's request) still YIELDs the window, and the
job revives vLLM canonically before its marker (the next session turn
auto-revives it through the gateway). The owner is asked to stay off the
gateway for the window's duration; a mid-window chat would boot vLLM and
end the run (the yield keeps the box safe, the data would be partial).

The job (`models-044b-h3artifacts`, stage B on the `vae=on` door):

- **Preflight hardened** (pass 2's three bugs): the readiness gate is
  `caps=200 AND vram > 15 GB` (a `caps=200` at 4 MiB is a load in
  progress, not ready); `write_digest` defined before use; the poll loop
  resets the job state on any non-200.
- **Door-crash resilience** (pass 2's A8): if the door's caps drop
  mid-run while vLLM is out, the job waits for the daemon's auto-reboot
  (proven 12:13:57) and resubmits the run (≤ 3 attempts); if a *different*
  GPU door is up instead (read from `GET /running`) — someone evicted us
  on purpose — the job YIELDs rather than fighting for the card.
- **Runs, same prompt/seeds as stage A, cheap first** (the t2i decision
  data lands before the big edits; a 3 h watcher cap bounds the tail):
  B6 1 MP / B5 2 K (whole-image tile) → B2 4K at 256 t / B3 4K at 512 t
  (each 20-min budget; same seeds as A1/A2 — isolates the FA numeric
  effect) → B1 4K at **4096 t (the makers' full-context decode)** / B7
  3264×4096 at 4096 t (the owner's exact shape) / B4 4K edit / B8
  3264×4096 edit (30-min budgets; the edits are tolerant — a repeated
  reference-encode OOM fails fast, a slow-but-alive generation gives up at
  the budget, either way the window continues).
- **Measurement-only ending** (D-044-5 as revised): the door script is
  restored byte-identical; the job writes `decision.txt`, `script.proposed`
  (vae=on + the largest tile that completed a 4K run) and a contact sheet
  of chest crops (owner 4K t2i / owner 4K edit / owner 1 MP / A1 / A3 / B1 /
  B2 / B7, whichever exist) for the owner as arbiter; the digest carries
  the daemon-journal evidence (tile halving / OOM lines). The mottle
  metric is *not* a grid detector (1 MP is visually clean at 0.81 while
  4K-256-t is 0.52 with the visible mesh) — the contact sheet is the
  arbiter, the metric set is for trend tracking.

Also before the window (this same turn): the service image is rebuilt from
the committed HEAD (lands the Paint-button fix, `6f7868f`, which the
running `dev-h3` image predates) and re-deployed on this box, with a 1 MP
H3 generate through the deployed service as the smoke test — item 1 of the
owner's original list.

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
- [ ] Service image rebuilt from HEAD and re-deployed on the dev box, a
      1 MP H3 generate through the deployed service clean
- [ ] Window digest: per-run wall / VRAM peak / metric set, stage A vs B,
      door-crash / OOM journal evidence
- [ ] The pass-3 window completes (or yields) with the runtime restored
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
