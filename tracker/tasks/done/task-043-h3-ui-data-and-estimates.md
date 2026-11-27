# Task 043 — H3's UI data and time estimates

Status: done (2026-11-27, no GPU window — the fix is plan/config/UI; verified live against the running service after the `kokon-ee:dev-h3` rebuild)

The 041/042 work made the *service* H3-correct (the row, the doors, the PE) but three
user-visible things still run on pre-H3 data. The owner asked (2026-11-27, with
screenshots): (1) does the UI enforce 16 refs for H3? (2) the quality-preset menu says
"native 2K · slowest" — is that wrong for H3? (3) where do the time estimates come from —
~2 m 9 s for 2 MP and ~9 min for 16 MP disagree with his measurements (DiT ~30–40 s at
16 MP).

## Findings (the mechanisms)

1. **Refs cap.** Service: yes — the submit cap is `effective_rec().max_references`
   (engine.rs), the H3 row = 16; the PE door's 64 k ctx is sized for it (042). UI: no —
   `maxRefs()` reads `profiles[activeProfile].recommendedSettings.max_references` = 10
   (the qwen-image-2.1 profile left in the config file), never the active external
   model. The UI therefore caps at 10 while the service would allow 16.

2. **Quality preset menu.** The Fast/Balanced/Quality sublabels are **hardcoded strings
   in Composer.tsx** ("~1MP · fastest", "~1.5K", "native 2K · slowest") written for the
   qwen Turbo; they reference no model data. The preset's *action* is already per-model
   (Quality → the row's area_quality → 16 MP for H3; the live /plan resolves auto to
   4096²). Only the labels are wrong.

3. **Time estimates.** `planner::estimate_seconds`: latent tokens = area/256, per-step
   cost 1.6·x^1.1 (sage) / 2.0·x^1.4 (else), x = tokens/4096, × guidance passes, ×
   reference factor, × steps, + encode (4 s + 3 s/4096 ref-tokens) + decode (1 s +
   1.25 s/MP), × the learned machine factor (data/speed.json, geometric moving average
   of measured/predicted, weight 0.5). For H3 on the live box that stack is wrong four
   ways:
   - the plan's steps are `TURBO_STEPS` = 8 (the row carries no steps field; the entry's
     `sample.steps = 6` is applied only to the job body, not the plan) — so the
     estimated work is 8/6 of reality;
   - the plan's `custom_sigmas` still carry the qwen 8-step schedule (the job body is
     cleared via the entry's `clear_sigmas` — the *display* lies);
   - the per-step curve is calibrated on the qwen 9B on one 5070 Ti;
   - the learned factor's key (`cuda|<gpu>|<placement>|sage=`) **carries no model**:
     H3's measurements recorded under the qwen-learned key, with predictions computed
     at 8 steps, cross-contaminating both models' calibration in both directions. On
     the split box the GPU name degrades to "unknown GPU", so the box can't even tell
     the curves apart.

## Acceptance criteria

1. **The plan is the job.** For an external engine the plan takes the entry's
   `sample` (steps / cfg / sampler / clear_sigmas): the planned steps are the door's
   (6 for H3), the plan's sigmas are empty when `clear_sigmas` (the door's built-in
   schedule runs — the display no longer shows the qwen schedule), and
   `estimated_seconds` is computed from the same values the job body receives. The
   speed *record* predicts with the same numbers. No model-id logic: the sample is
   deployment data on the entry.
2. **Per-model calibration.** The speed key gains the model id
   (`…|sage=…|model=<id>`): each door calibrates itself from its own measurements and
   models stop sharing (or poisoning) one factor. On a box whose local GPU is unknown
   (the split box) and an external engine is active, the *uncalibrated* estimate is
   hidden (0) until that model's first measured job — the UI already renders 0 as
   "time known after the first image".
3. **The UI reads the active model's data.** The /config response gains a derived
   `activeModel` (`model`, `maxReferences`, `presetAreas{fast,balanced,quality}`)
   computed by the service from the same row dispatch the planner uses
   (`effective_rec`: the H3 row for the makers'-PE family, the profile row otherwise).
   The UI's refs cap and the quality-preset labels come from it. Preset labels: the
   qwen-area triple (1 MP / 2.36 MP / 4 MP) keeps its current strings verbatim (no
   churn for existing users); any other row gets derived labels (MP notation; the
   Quality tier is marked "native …" when it is the model's top resolution tier, the
   same rule the size menu uses). H3 → "~4MP · fastest", "~8MP", "native 16MP ·
   slowest".
4. `cargo test -p qls-core` green (new: plan-with-sample steps/sigmas/estimate, key
   carries the model, activeModel for H3 and qwen, the split-box hide rule); `npm test`
   green (new: maxRefs from activeModel, preset-label derivation incl. the no-churn
   qwen case); `npx tsc --noEmit` clean; `mcp` unaffected or green.
5. Redeploy the dev image on the box; live /config shows `activeModel` (H3: 16 refs,
   4/8/16 MP); live /plan for H3 4096² shows the 6-step-derived estimate (or the
   hidden-until-first-measurement 0), no qwen sigmas; the UI caps refs at 16 and shows
   the H3 preset labels. No config-file change needed (schema 19 stands) — the
   activeModel is derived, so nothing to redeploy to `frontends/kokon-ee/config/`.
6. tracker/STATUS.md, docs/STATUS.md, docs/engineering-notes.md (the estimate model +
   per-model calibration + the derived activeModel), docs/http-api.md (/config's
   activeModel).

## blockers

(none)

## decisions

D-043-1: The plan applies the entry's sample to steps/cfg/sampler only for real
diffusion plans (steps > 0); Quick (steps 0) and non-diffusion paths are untouched.
The `effective_sample` helper moves to planner (pipeline re-exports it) so the plan
and the job body share one implementation — the display and the request cannot drift
again.

D-043-2: The speed key gains `model=<active model id>` for every engine (local
profiles included — profile switching changes the model, so the old shared key was
already cross-model). Pre-existing factor entries simply stop matching; they are
harmless orphans in speed.json.

D-043-3: `activeModel` is derived in the service (a `ConfigOut` wrapper that
flattens AppConfig), not a new config-file field: the row is Rust code for external
engines (h3_recommended) and the profile block for local ones — the deployment file
stays the source only where it already was. Schema 19 unchanged; old UIs ignore the
extra key.

D-043-4: Preset-label no-churn rule: the exact qwen-area triple keeps the historical
strings; everything else is derived (MP notation, "native …" when the quality tier is
the model's top resolution tier).

D-043-5: No new GPU window for this task: the changed surfaces are the plan (verified
live via /plan), the config reply (verified live via /config) and the UI (unit-tested).
The first H3 image after the deploy records the per-model factor and the estimate
self-corrects from the box's own measurements.

## verification (live, after the `kokon-ee:dev-h3` rebuild, 2026-11-27)

- `GET /config`: `activeModel = { model: "kokon-h3-image", maxReferences: 16,
  presetAreas: { fast: 4194304, balanced: 8388608, quality: 16777216 } }`,
  `schemaVersion: 19` unchanged — the deployment config file untouched.
- `POST /plan` (quality/auto): `output 4096×4096, steps 6, sampler euler, cfg 1.0,
  custom_sigmas []` — the door's built-in `minimax-h3-image` schedule, no qwen schedule
  in the display; `estimated_seconds 0.0` (split-box hide rule: the service sees no
  local GPU, so the uncalibrated number hides until H3's first measured job — the UI
  renders it as "time known after the first image").
- `POST /plan` (balanced/auto): `2912×2912 (8 MP), steps 6, est 0.0`.
- Suites: qls-core **401** (new: plan-is-the-job, per-model key, show-uncalibrated,
  active_model_info), UI **251** (new: maxRefs from activeModel, preset-label
  derivation incl. the no-churn qwen case), mcp 49 passed + 3 skipped, tsc clean.
- The dev box was redeployed (`kokon-ee:dev-h3` rebuilt + compose restart); the PE/DiT
  doors are gateway-owned and were never restarted; vLLM stayed up.
