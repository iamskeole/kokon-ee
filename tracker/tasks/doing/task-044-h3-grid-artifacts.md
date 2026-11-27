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
    - **decision rule** (D-044-5): apply the `vae=on` + largest-fitting-tile
      door config only if a stage-B 4 K run completes AND its grid score is
      ≥ 2× better than the 256-tile baseline; otherwise restore the
      original script and report
(c) If a fix lands: the door is a script (no rebuild) — the next H3 load
    picks it up; update the door header comment, the ml provenance note,
    and `docs/engineering-notes.md` with the measured result.

## Acceptance

- [ ] The Paint button is hidden for H3 (and any no-inpaint entry) — done
- [ ] Window digest: per-run wall / VRAM peak / grid score, stage A vs B
- [ ] A root-cause statement backed by the measurements (tile size ↔ grid
      score, FA off → no full-tile fit, 1 MP control clean)
- [ ] A door config chosen by the decision rule (or the honest
      "no fix fits this card" report)
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
- **D-044-5** The window never applies a door change it cannot stand
  behind: apply only on fit + ≥ 2× grid-score improvement; otherwise
  restore the original script and report (the owner's images are the
  arbiter for anything in between).
