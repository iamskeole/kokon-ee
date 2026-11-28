# task-047: GPU ESRGAN upscaling on the current doors — the DGL revival of #2404's casualty

**Parked (owner trigger, separate session; owner's ask: the 17-second GPU upscale).** Set up
2026-11-27, very late, from the 1b investigation during task-045.

## background

The service's ESRGAN enlarge calls `POST /sdcpp/v1/upscale` (synchronous, one-shot) — added
upstream in PR #2026 (commit `4dfe8f5`) and **deleted** in PR #2404 (commit `5b80e1f`,
"one context with on-demand model loading"; the sdcpp routes file lost ~427 lines in that
commit). All four doors on this box run the post-#2404 build (52491bd): capabilities still list
the upscaler models (`model: true`), but the `image_upscale` flag the service gates on
(`has_upscaler(name, true)`) is gone — so every enlargement falls back to Lanczos with the
honest note ("The engine box doesn't have that upscaler…").

The ESRGAN itself is alive in the build we run: it is a **DGL node**
(`src/dgl/models/image_upscale.{h,cpp}` — `ImageUpscaleModel` with ESRGAN + MoSR runners,
`upscale_image_tensor(model, tensor, w, h)`), and the current server exposes
**`POST /sdcpp/v1/dgl/jobs`** (graph source string → 202 job id + `poll_url`) with a
`--dgl-dir` and a file-access policy for model files. The makers ship a template for exactly
this operation: `src/api/templates/upscale.dgl` — `load_upscaler(model_path, tile_size)` →
`upscale(model, image)` → `{image}`. (Both paths under
`/home/p/ml/backends/stable-diffusion.cpp/` on the box.)

## design sketch (verify in the session)

- **`sdcpp.rs`:** a DGL client — POST the graph source to `/sdcpp/v1/dgl/jobs`, poll the job,
  fetch the result image (verify how the job result carries pixels: base64 vs file path vs
  media; `describe_dgl_result` in `async_jobs.cpp` is the reader).
- **`engine.rs::enlarge()`:** when the upscaler model is in caps **and** the door advertises
  dgl/jobs → run the DGL graph (model path = the upscaler file; `tile_size` per the template);
  keep the Lanczos fallback for doors/builds without DGL. The gate becomes "a `model: true`
  entry exists and the door advertises DGL" — current caps shape, not the dead `image_upscale`
  flag.
- **Notes:** restore the ESRGAN "enlarged with <name>" note on the success path; keep the
  Lanczos note on fallback; task-038's contract (engine's capabilities as arbiter) stays.
- **Measure in a GPU window:** the service's real enlargement factors (1.2–1.8× from the
  clamped-edit enlargements, and the explicit user upscale jobs up to 4×) — task-038 measured
  ~17.8 s for 4× on the older build; confirm the number on 52491bd.

## decisions

- **D-047-1 (owner):** CPU ESRGAN is not acceptable ("17 seconds vs minutes is unacceptable");
  the path must stay on the GPU and on the current stock build. DGL is that path. The
  290148d upscaler-door variant and an upstream PR (re-adding the one-shot endpoint) remain
  the fallbacks if DGL turns out infeasible — owner's call at that point.

## acceptance criteria

- [ ] A DGL ESRGAN job runs on the current doors on this box (measured in a window;
  17-second-class at the service's real factors).
- [ ] The service's enlarge path uses it when the door supports it; notes honest on both
    paths; Lanczos fallback intact for doors without DGL; the `has_upscaler` gate corrected
    to the current caps shape.
- [ ] Tests (mocked dgl/jobs round-trip), suites green, docs updated (engineering-notes
    upscaler section, fresh-box upscaler pin).

## blockers

None. Open questions for the session: how dgl/jobs delivers the result image; whether the
door's DGL file-access policy reaches the `--hires-upscalers-dir` files (the `upscale.dgl`
`model_path` must resolve for the door process); whether the template's `upscale` node takes
an explicit output size (the service enlarges at non-integer factors, not only 4×). All four
doors run 52491bd, so all four should work if one does.
