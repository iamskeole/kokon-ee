# task-031 — the chaff cut: the product is the box service

With independence, the "standalone app" machinery the product no longer
ships goes away: the bundled-engine path, the installer, the updater, the
Mac engine, the per-OS release logistics, upstream's marketing. What
remains: **the service (docker image + bare binary) + the browser UI +
the MCP**, all pointing at a gateway of stock engines.

## acceptance criteria

1. **The cut list is finalized in-session, per file, with its reason** —
   the working list (verified against the tree when started):
   - **Bundled-engine path**: `catalog.rs` (the engine-model download
     pins — **keep the lab model downloads in `matting.rs`: MobileSAM /
     BiRefNet are a service feature with hash-pinned URLs, not chaff**;
     the upscaler downloads in `settings`/service are the same — keep),
     `installer.rs`, the bundled-process management in `engine.rs`
     (**the external-engine client parts of `engine.rs` stay** — it is
     the heart of the product), `engine.json`, `backend/start|stop|
     restart` routes, the setup wizard (`/setup*` routes + the UI),
     the model scanner/inspector (`/models/scan|inspect|new_dit|
     improve_vae|use|user` — the bundled-model lifecycle; the external
     picker `switch` stays).
   - **The Mac engine**: `engine-mac/` (whole dir + its AGENTS.md),
     `mac_bench.rs`, the `/mac-engine/*` routes, the tauri Mac bits.
   - **The updater**: `@tauri-apps/plugin-updater`, `src/lib/updates.ts`,
     the release-update plumbing (the product updates by pulling a new
     image, not self-updating).
   - **The release logistics**: `scripts/build-release*.ps1/sh`,
     `scripts/release/fetch-bundled.mjs` (+ `npm run bundled`),
     whatever else exists only to make installers.
   - **Upstream's marketing**: `site/`, `marketing/`, the root
     `index.html` + `wrangler.jsonc` (the kokon.app Pages setup — dead in
     the fork, no deploy secrets here), the README badges that point at
     his releases/ko-fi/stars (D-031-3).
   - **The Tauri shell** — only if D-031-1 says cut (see blockers).
2. **The Tauri shell is cut** (D-031-1, confirmed 2026-11-17): `src-tauri/`
   gone, the tauri branches in `src/` (`platform.ts`, `native.ts`, the FFI
   calls in `api.ts`/`store.ts`) gone or collapsed to the web path, the
   tauri dev script and the updater plugin gone; the product's local mode
   = run the service binary directly (`--remote`) — no per-OS installer,
   no window, no per-OS build/test logistics.
3. **The stock path is gone and the config says so**: the service is
   EE-first; the config schema advances (15 → 16) with a migration that
   maps a pre-cut config (any stock settings) to the EE equivalent or a
   clear, kind error ("this version only runs against an external
   engine — point it at your gateway"); the `multiUser` gate and the
   schema-14 native-PE wiring carry through byte-for-byte (the live box
   config is the test fixture — it must load on the new schema with
   zero data loss; the `config.json.bak-*` convention applies). The
   **stock-path byte-identity obligation ends here** — that was the
   standing constraint behind the D-MU13 JSON decision; its retirement is
   recorded in the task file (and noted in task-032's rationale).
4. **The docs follow**: `docs/fresh-box.md` rewritten to the new identity
   and the post-cut shape (clone `kokon-ee` → `docker build` → compose →
   the stock engines at their pins → the models per the provenance
   convention → done; the stock-path sections that described the bundled
   engine go), `docs/service-on-prod-box.md` updated (it already
   describes the split deployment — mostly a rename pass), AGENTS.md
   (the fork preamble becomes the project preamble; the architecture
   diagram drops the installer), CONTRIBUTING (the build/test table
   loses the per-OS installer rows), `docs/linux.md`/`platforms.md`
   pruned to what exists.
5. **The suite green and smaller on purpose**: the tests that covered
   cut code are removed or repointed (the count drop is recorded, e.g.
   "qls-core 406 → N: the catalog/installer/engine-bundled tests are
   gone; the external-engine + multi-user suites intact"); the image
   builds smaller (the size delta recorded — the ONNX lab models and the
   UI are the payload now); deploy window (service-image swap, CPU-side,
   web sessions die and re-login — known), box check, the one-step
   rollback staged (the previous image tag stays on the box).
6. **The MCP re-list is staged for the owner** (the registry pages
   under the new identity — glama/mcpservers.org — are owner-account
   actions; the repo-side files are ready in this task).

## blockers

- ~~Owner: the Tauri shell (D-031-1)~~ — **resolved 2026-11-17: cut.**
  (The rationale stands as recorded in D-031-1: the shell exists to
  distribute the bundled-engine installer; our product is docker +
  browser + MCP, and keeping it keeps the per-OS logistics we are
  shedding.)
- **task-030 first** — cut the merged tree (we delete 0.12.1's final
  shape of each file, so we never have to re-delete a variant).
- The config-schema migration touches the live box config (schema 15,
  `multiUser` on, the native-PE adoption) — the migration is tested
  against a copy of the real config before the cutover (the 11-11
  lesson: a settings round-trip must never wipe a secret or a gate).
- The lab's in-browser models (`onnxruntime-web`) and the service-side
  BiRefNet/MobileSAM are **not** part of this cut — they are product
  features (the Layers lab), however their download path is service-side
  (kept) and the prod-box half of task-027 (the lab as a thin client
  for weak devices) may rework it later.

## decisions

- **D-031-1 — The Tauri shell: CUT (owner-confirmed 2026-11-17).** The
  decision record: it exists to distribute the bundled engine (the
  installer is the product's body upstream); our product is docker +
  browser + MCP, and keeping it keeps the per-OS build/test logistics
  the unbundling was meant to shed. The "no docker, window next to the
  service" mode is covered by running the service binary directly
  (`--remote`) — no window needed.
- **D-031-2 — (fill in in-session) the final cut list** (the working
  list in the acceptance criteria is the starting point; every file
  named, every keep argued).
- **D-031-3 — Marketing: delete, no tag.** `site/`, `marketing/`,
  `index.html`, `wrangler.jsonc` are deleted (the git history is the
  archive — no `parked/*` tag needed; the 11-06 tidy already set the
  pattern for the branches). The README keeps the "Derived from Kokon"
  attribution and links to upstream for the original product.
- **D-031-4 — Lab model downloads stay.** `matting.rs`'s hash-pinned
  MobileSAM/BiRefNet downloads and the upscaler downloads are service
  features of the shipped product, not standalone-app chaff.
- **D-031-5 — (fill in in-session) the config schema 16 migration:**
  stock settings → dropped with a note in the saved file's
  `migrations` log (same pattern the config already uses) or mapped to
  the EE equivalent; the `externalEngine`/`vlm`/`multiUser` surface
  untouched.

## progress

- 2026-11-17: task created (owner's 11-17 message: "shave off chaff —
  there's a lot of 'standalone app' philosophy code from upstream we no
  longer need in this new decision/direction, so maybe clean it up now").
  Ordered after the rebase (D-030-1) and before task-032 (the
  byte-identity constraint has to actually be gone first).
- 2026-11-17 (from task-030's post-close note, sharpened 17:30): the
  deployed box kept toasting "inference server did not report its
  capabilities (HTTP 500)" with no inference running — two *failed
  jobs from hours earlier* (pre-engine-restore). Mechanism: `store.ts`
  toasts on every *observation* of a failed job (the job-update
  handler ~line 746 and `onReviewJob` ~line 760), so stale failures
  re-toast on every reconnect/refresh. Small scope item: toast only
  when the UI *witnesses the transition* to failed (dedupe per job id
  in client state; a job already failed when the session starts is
  history, not an alarm), and put the failure time in the detail.
  (Operational stopgap until it lands: a service restart clears the
  in-memory job store — jobs never hit disk.)
