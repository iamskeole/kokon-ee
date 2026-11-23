# Task 040 — the reference-image cap: the makers' max, in one place

**Status: done (2026-11-24, one turn).** Owner question, raised with the PUP
ctx work: "the UI check for what happens if the user supplies 11 reference
images — do we need one?"

## What the investigation found

The cap was not missing — it existed in **three places, hardcoded 10 in
two of them**:

1. **The planner** (`planner.rs::plan`, first check): rejects
   `image_sizes.len() > rec.max_references` with
   `Use at most N reference images.` — `RecommendedSettings.max_references`
   (config `recommendedSettings.max_references`, default **10**, the makers'
   card max). Runs at job start and in the `/plan` dry run (the UI's
   preview already showed the error after an 11th reference was added).
2. **The UI** (`store.ts`): `addAsReference` and `addInputs` both stopped at
   **10** (literal) with the toast "Up to 10 reference images are
   supported." — so picker, browse, drop and paste all already refused the
   11th reference; the user never reached the service.
3. **The submit backstop** (`engine.rs::submit`): a generic
   `images.len() > 16` → "Too many images."

The gap: the UI's 10 was a constant while the service's was config-fed, so a
changed cap would silently disagree between the two, and a raw-API client
only learned the truth when its job *started* (after queueing).

## What changed

- `engine.rs::submit` now checks the profile's `max_references` at submit
  (before queueing) with the planner's exact message; the 16 stays as the
  protocol backstop for a misconfigured cap above it.
- `store.ts` state gained `maxRefs` (default 10), set from the config at
  connect (`App.tsx`); `addAsReference`/`addInputs` use it, so the composer
  and its toast follow whatever the service configures.
- `logic.ts`: `maxRefs(cfg)` — the active profile's
  `recommendedSettings.max_references` (wire key, snake_case), falling back
  to the default for a missing/broken value; `DEFAULT_MAX_REFS = 10`.
- `config.rs::validate` rejects `max_references < 1` (a zero cap would
  reject every edit).
- Docs: `docs/http-api.md` names the cap and the 400.
- Tests: planner (11 → the message, 10 → ok, a custom cap honoured), submit
  (11 images → the message, before queueing), config validation (0 →
  error), UI `maxRefs` helper (default / active profile / broken values).

## Decisions

- **D-040-1**: the cap lives where it always did — the profile's
  `recommendedSettings.max_references` (default 10 = the makers' max, the
  number now recorded in the models' provenance). No new config key; the UI
  reads the existing one.
- **D-040-2**: the UI never invents its own number — one source, the
  service's config; the default only stands in before first connect.

## Blockers

None.

## Verification

`cargo test -p qls-core` 386 green; `npm test` 245 green (13 files); mcp
35 + 3 skipped; build green (new bundle). Both-box rollout with the next
image build.
