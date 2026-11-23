# task-038: upscalers live on the engine box — state and gate come from the engine

## status: DONE 2026-11-23 (night) — owner's two-box question, same-night fix, proven on the box

## goal

Upscalers (ESRGAN/MoSR `.pth` models) are loaded and run **by the engine**
(`--hires-upscalers-dir`, resolved by file stem at request time). The service only
sends the name (`POST /sdcpp/v1/upscale`, hires second pass). In the split
deployment the files live on the **accelerator box** (the ml repo's
`models/sd-cpp/upscalers/`, pinned — see `docs/fresh-box.md`). But three places
still reason about upscalers from the **service box's** own data volume:

1. **The planner's gate** — `plan_inputs()` feeds `PlanInputs.upscaler` from
   `cfg.upscaler()` (a local file check on `/data/sdcpp/models/upscalers`).
   On a box without local copies (a fresh prod box, or this dev box this
   morning) the planner sets `plan.upscaler = None`, so the job silently
   enlarges with Lanczos and notes "The upscaler model is missing (install it
   in Setup)" — even when the engine **has** the model.
2. **`GET /upscalers`** (Settings list) — `installed` = local file present;
   the Download button stores a copy on the service box that the engine
   never sees.
3. **`/status` `upscaler.available`** — same local-file check.

**Proven live (2026-11-23, this dev box):** the owner's 10:03:03 upscale job
enlarged with Lanczos ("The upscaler model is missing (install it in
Setup)") although the engine's pinned directory had Superscale since
2026-10-27; the 10:05–10:06 UI downloads (84 MB, to the service box) only
flipped the service's local gate; the 10:06:46 job then ran Superscale on
the **engine's** copy (the fans). The downloaded copy served no inference
purpose.

## decisions

- **D-038-1**: In EE (external-engine) mode the upscaler's identity is the
  **configured name** (`upscaler_model`, "lanczos" excluded) handed to the
  planner; the existing fresh **capabilities** gate in the job path
  (`caps.has_upscaler(name, true)`) stays the arbiter of whether the engine
  can run it — the engine's own list *is* the engine box's file list.
  Bundled mode keeps the local file check (byte-identical behaviour).
- **D-038-2**: `GET /upscalers` in EE mode reports per item
  `installed: true|false|null` from the engine's last cached capabilities
  (`null` = engine not queried yet), and the response carries
  `external: true`. The UI hides the Download button in EE mode; a `false`
  item says the model is not on the engine box (pin it there — fresh-box
  doc), a `null` item says the engine state is pending.
- **D-038-3**: `POST /upscalers/{name}/download` in EE mode is refused with a
  plain error (the service box has no write path to the engine box).
- **D-038-4**: `/status.upscaler.available` in EE mode comes from the cached
  capabilities (`null` when unknown).
- **D-038-5**: The "cannot run the upscaler" job note gets split-box wording
  in EE mode (pin it on the engine box). The local "missing (install it in
  Setup)" note stays for bundled mode, where it is correct.
- **D-038-6**: No config schema change. No new downloads anywhere. The
  fresh-box doc names the upscalers as engine-box pins.
- **D-038-7**: The Setup wizard's two bundled-engine checks
  (`upscaler_check`, `engine_release_check`) get split-box short-circuits in
  the same spirit: on a split box the wizard would otherwise report a
  missing local upscaler and an updatable engine — both false. They now
  report what is true (the model is on the engine box; the engine is a
  pinned stock build). The wizard itself stays until task-031 cuts it.

## acceptance criteria

- [x] EE mode: an upscale job with a configured ESRGAN name sends it to the
  engine when the engine's capabilities list it (no local file needed);
  falls back to Lanczos with the engine-box note when the engine lacks it.
  **Proven live on the dev box with the service-side copies moved aside**
  (job `job_9f5ffb616b3a`: done, zero notes; engine journal: `load
  /home/p/ml/models/sd-cpp/upscalers/4x_NMKD-Superscale-SP_178000_G.pth`,
  ESRGAN 17.8 s).
- [x] Bundled mode: planner, list, download, notes all unchanged (tests).
- [x] `GET /upscalers` and `/status` report engine-sourced state in EE mode
  (true/false/null); download refused in EE mode (422, plain message).
- [x] UI: no download button in EE mode; honest states; bundle verified to
  carry them. (The component logic follows the file's existing inline style;
  the service-side behaviour is unit-tested in Rust.)
- [x] Docs: fresh-box (upscalers = engine-box pins; service box needs none in
  split mode) and prod-box (handover: the accelerator box's upscalers are
  pinned; the thin box's Settings reflects the engine and downloads nothing).
- [x] `cargo test -p qls-core` (427) / `-p qls-service` (20) / `npx vitest run`
  (255) / `tsc` clean.
- [x] Deployed as `multiuser-9f1f070` on the dev box (commits `85dafa4` code +
  `9f1f070` docs); lab statuses and vllm co-residency re-verified untouched.

## blockers

None. (The 84 MB of service-side copies on the dev box stay on disk,
harmless; the fix makes the UI stop offering them.)
