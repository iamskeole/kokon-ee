# task-046: 16 GB-card-era calibrations audit — no canned limits on a box that manages its own backend

**Parked (low priority; owner trigger, separate session).** Set up 2026-11-27, very late, from
the owner's follow-up to task-045.

## trigger

The owner's overarching principle, stated in this session: "by unbundling the frontend / service
from the backend, we trust the backend operator to know what they're doing and customize their
box — no canned limits on some old measurement, historical baggage." Task-045 removed the
biggest one (edit budgets fell back to a Qwen-16 GB-era token budget); the owner believes more
exist ("sounds like we got most of it but I'd like to be sure") and wants a full audit of the
service code base.

## background

The service was born as a bundled product (service + engine on one 16 GB-class card); many
defaults and calibrations date from that era and live in code:

- **`planner.rs`:** `token_budget()` (16384 tokens = "a 16 GB card runs 2048²"; the
  `<15 GiB` and `<=20 GiB` branches), `DISCRETE_LARGE_GPU_BONUS` (16 GB native-2K-edit
  measurement), `LARGE_GPU_VAE_TILE_AREA`, `UNTILED_VAE_MIN_VRAM_MIB` (23 GiB),
  `UNTILED_VAE_MAX_AREA`, `VAE_DECODE_RESIDENT_MIB` (8 GB = Qwen DiT 3.9 + VAE 0.65),
  `VAE_DECODE_BYTES_PER_PIXEL` (5090 measurement), `VAE_DECODE_OTHER_APPS_MIB`
  (8 GB "desktop and other apps"), `STANDARD_REFERENCE_AREA` (1 MP), `cap_for_refs`
  (1/(1+n) share), `CanvasLimits` defaults (`min_side`, `max_upscale`).
- **`speed.rs`:** the time model (qwen-9B-on-5070Ti calibrated; per-model factors landed in
  task-043, but the base curve and step-count assumptions are qwen-era).
- **`engine.rs`:** the enlarge path (`ESRGAN_MIN_FACTOR` 1.1, the upscaler gate — see
  task-047), VAE-decode tile heuristics for unpinned models.
- **`catalog.rs` / `config.rs`:** bundled-era downloads and defaults (watermark, upscalers).
- **UI:** remaining 16 GB-era strings/labels (task-043 fixed the preset sublabels), the
  estimate display, reference-detail defaults.

The measured-row bypass (tasks 044/045) only covers models carrying measured rows; everything
else still plans on the 16 GB-era formulas. That is *right* for an unknown 16 GB box, but the
owner wants each assumption either **justified as a safe default for unknown boxes** or
**driven by the box's own data** (config rows, door capabilities — `max_width/height`,
`measured_peak_mib`, `vramOverrideMib`) — not a silent constant.

## decisions

- **D-046-1 (owner principle, this session):** on a split box the service does not impose
  canned limits from historical measurements; a box owner's backends are sized from the box's
  own data. Bundled-box defaults may remain for boxes without data — safe, honest, documented.
  The audit must keep that distinction explicit per item.
- **D-046-2:** inventory-first: every constant, formula, and user-visible string gets a row
  (location, what it assumes, where measured, when it applies), then a disposition per item
  (keep as documented default / make it config-overridable / derive from capabilities). No
  behavior change without the owner's per-item verdicts.
- **D-046-3:** the deliverable is the inventory (engineering-notes section or a table in this
  file) + the approved fixes, one small commit per item, tests updated. D-044-12 discipline
  throughout: no hardcoded model exceptions — per-model data lives in config rows.

## acceptance criteria

- [ ] Full inventory: every 16 GB-era constant, formula, and user-visible string in
  `crates/qls-core` (and the UI where visible), each with its source/measurement and when it
  applies.
- [ ] Per-item disposition settled with the owner (keep / override / derive); approved fixes
  implemented with no hardcoded model exceptions; tests updated.
- [ ] Suites green (qls-core / UI / mcp); docs updated; behavior on boxes *without* measured
  data unchanged except where the owner explicitly approves.

## blockers

None. Low priority — a separate session on the owner's trigger.
