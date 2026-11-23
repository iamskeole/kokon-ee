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
- **D-031-2 — final cut list (in-session, 11-23, after inventory of the
  post-037/038 tree).**
  **Cut, whole files/dirs:**
  - `src-tauri/` (whole dir: main.rs, lib.rs, icons, tauri configs)
  - `engine-mac/` (whole dir)
  - root `engine.json`; root `wrangler.jsonc` (kokon.app Pages deploy)
  - `crates/qls-core/src/{installer,mac_bench,engine_manifest}.rs` + their `mod` lines
  - `scripts/build-release.ps1`, `scripts/build-release-mac.sh`, `scripts/build-release-linux.sh`,
    `scripts/release/fetch-bundled.mjs`, `scripts/release/updater.mjs`,
    `scripts/dev/mac-dev.sh`, `scripts/dev/sidecars.mjs`, `scripts/dev/m1-backend-test.ps1`,
    `scripts/dev/test-12gb.ps1`, `scripts/dev/test-fresh-setup.ps1`, `scripts/dev/test-model-update.ps1`,
    `scripts/dev/test-own-models.ps1`, `scripts/dev/test-vae-update.ps1`,
    `scripts/dev/validate-upscale.ps1`, `scripts/dev/mac-bench/` (dev dir)
  - `site/`, `marketing/`, `docs/releasing.md`
  - UI: `src/components/{TitleBar,Setup,OwnModels,WhatsNew}.tsx`, `src/lib/updates.ts`
  **Cut, internally (bundled path; the EE client stays and becomes the only path):**
  - `process.rs`: `build_server_args`, `build_mac_engine_args`, `spawn_and_wait`, `launch`,
    `launch_port`, `wait_port_free`, `kill_pid` (if unused), the owned-server record
    (read/write/record_path), `free_port` (if unused); `ensure_external` + `adopt_external`
    become the only `ensure_ready` / `adopt`.
  - `engine.rs`: the bundled branch of `ensure_ready`, `server_exe`, `missing_files_error`,
    `uses_mac_engine`, `engine_update_needed`, `remove_unused_old_dit`, the owned
    stop/restart paths, any bundled model-download/update flow.
  - `catalog.rs`: the engine download pins (`engine_downloads`, `EngineManifest`,
    `apply_engine_release`, the tag helpers, `manual_download_hint` if bundled-only).
    **Keeps:** the lab model pins (MobileSAM/BiRefNet — D-031-4), the upscaler download
    pins (D-031-4 / D-038: the download route stays and is refused in EE mode), the LoRA pins.
  - `config.rs`: the bundled-only *helper fns* go (`server_exe`, `missing_files_error`,
    `uses_mac_engine`, …); the **fields stay** in the struct (zero data loss, dormant).
  - service `api.rs`: routes `/setup*` (all six), `/models/{scan,inspect,user,use,vae,dit}`,
    `/mac-engine/*` (all four), `/backend/{start,stop,restart}` go.
    **Keeps:** `/models` + `/models/switch` (the picker — the external picker `switch` is the
    product's model control), `/upscalers*`, `/matte*`, `/segment*`, and everything else
    (jobs, gallery, luts, loras, projects, remote, users, auth, events, diagnostics, logs,
    plan, inputs, examples, provenance, shutdown).
  - `diagnostics.rs`: reworked to EE-only checks (the engine check = "the engine answers at
    the gateway"; the hardware check and its `parse_device_list` consumer go).
  - `modelfile.rs`: **kept** (the LoRA pipeline uses it); only its bundled-only callers go.
  - UI internals: `native.ts` → browser-only (all tauri branches cut); `platform.ts` →
    `inBrowser` drops (it is always true); `store.ts`/`api.ts` → tauri branches cut (the
    `__KOKON_VERSION__` compare stays — it compares the page against the service, both in
    the image); `App.tsx` → the `needsSetup` gate goes (a config without `externalEngine`
    fails at boot with the kind error instead); `Settings.tsx` → the Setup-wizard and
    bundled-engine sections go; `logic.ts`/`types.ts` → `InstallItem/InstallStatus/
    ModelsStatus/ModelScan/ModelSlot/SetupReport/HardwareReport/ModelFileInfo`,
    `whatsNewDue`/`modelOfferDue`/`afterStartScreens`, the `setupComplete` wiring go.
  - `mcp/src/service.ts` → re-aimed at the box service (endpoint/env discovery, not
    "Kokon.app/Contents/MacOS"); the not-found error says "start the box service".
  - `package.json`: the six `@tauri-apps/*` deps, `@tauri-apps/cli`, the `tauri` and
    `bundled` scripts go; the root `Cargo.toml` workspace loses `src-tauri`.
  - Docs: `README.md`, `CONTRIBUTING.md`, `AGENTS.md`, `CLAUDE.md`, `SECURITY.md` reworded
    (the product = the box service + the browser UI + the MCP); `docs/platforms.md` and
    `docs/linux.md` rewritten for the box; `docs/engineering-notes.md` architecture section
    updated; `docs/fresh-box.md` and `docs/service-on-prod-box.md` stay authoritative
    (minor updates only).
- **D-031-3 — Marketing: delete, no tag (11-17; clarified 11-23).** `site/`, `marketing/`,
  `wrangler.jsonc` are deleted (the git history is the archive). The **root**
  `index.html` is the app's own HTML shell (vite entry, PWA manifest, boot splash) and is
  **kept** — the 11-17 mention of `index.html` was the site's landing page, which goes with
  `site/`. The README keeps the "Derived from Kokon" attribution and links to upstream.
- **D-031-4 — Lab model downloads stay.** `matting.rs`/`segment.rs`'s hash-pinned
  MobileSAM/BiRefNet downloads and the upscaler downloads are service features of the
  shipped product, not standalone-app chaff.
- **D-031-5 — config migration, schema 16 → 17 (resolved 11-23; the 11-17 note said 15 → 16,
  which task-037 already used for `visionEngine`).** The migration keeps **every** field of
  any stock config (zero data loss), records the cut in the config's `migrations` log, and
  after it a config **without `externalEngine` fails boot with a kind error** ("this version
  runs against an external engine — point it at your gateway"). The bundled-only fields
  stay in the struct, dormant. The `externalEngine`/`vlm`/`visionEngine`/`multiUser`/native-PE
  surface is untouched (the live-box config is the fixture — it must load under 17 unchanged).

## progress

- 2026-11-17: task created (owner's 11-17 message: "shave off chaff —
  there's a lot of 'standalone app' philosophy code from upstream we no
  longer need in this new decision/direction, so maybe clean it up now").
  Ordered after the rebase (D-030-1) and before task-032 (the
  byte-identity constraint has to actually be gone first).
- 2026-11-17: the 'stale failed job keeps toasting' bug found during the
  task-030 window work is **not** part of this cut — it is a few-line UI fix
  (toast only on a witnessed live-to-failed transition, plus a 60 s freshness
  window for first sightings) and shipped the same afternoon as **task-034**
  (image `multiuser-508182b`) so this task stays focused on the chaff itself.
- 2026-11-23 (night, owner's GO in the day message; owner asleep, demo day
  the next morning — the cut was done, deployed and closed in one sitting,
  the box left serving): **DONE**. Phases as commits: P1 the Tauri shell
  + updater (`c953acb`), P2 the Mac engine (`8a45344`), P3 the engine-update
  machinery (`a1de77d`), P4 the installer + setup wizard + the model
  machinery (`d68bc38`), P5 the bundled process management (service
  `c5fb7a9` — process.rs is now a pure adopt/reconnect state machine with
  `pid_alive`; the backend routes are gone; the config's desktop fields
  sleep as schema-17 values; plus the shared-`config.json.tmp` flake fixed
  by a unique temp name) + P5 the UI (`cbd32b2` — `inBrowser` and every
  desktop branch are out; the UI is unconditionally the browser client),
  P7 the image header (`ed4a1ce`), P8 the docs/CI/scripts (`9a825d9` —
  linux/platforms/releasing out; engineering-notes, http-api, the
  fresh/prod-box docs, README, AGENTS, CONTRIBUTING, NOTICE, the workflows
  retargeted to the fork's ghcr), P9 the MCP (`cf3b510` — finds the box's
  `kokon-service`, tells the user to start the box), P10 the boot guard
  (`b3cdbed` — a config without `externalEngine` is a stock config and
  fails the boot with a kind `InvalidConfig` error, AC3).
  **AC check:** 1 the cut list (`65bf5e0`, per file with reasons);
  2 the shell is out (AC2); 3 the stock path is out, schema 16→17 with the
  live box config as fixture (it loads, migrates in memory and keeps every
  field; the boot guard lands in P10); 4 the docs follow (P8 — the
  fresh/prod-box docs stay authoritative); 5 the suite: qls-core
  **417 → 383** (the bundled engine/installer/catalog/model-profile tests
  are gone; the external-engine, multi-user, security and lab suites
  intact), service 20 (the flake gone — 15/15 green runs), UI **248 → 242**,
  mcp 35+3sk; the image **203 → 201 MB** (57.9 → 57.4 compressed); the
  deploy window ran on CPU only — image `multiuser-b3cdbed` built,
  recreated on the dev box (web sessions die and re-login, as planned),
  ping + LAN UI 200, the engine still adopted (`kokon-1013-1814814`,
  owned=false), vllm healthy on 9118 (200); the rollback tag
  `multiuser-9f1f070` stays on the box (ml repo `4115e86`, `0388a46`).
  6 the MCP re-list is staged: the repo side is done (P9); the registry
  pages (glama/mcpservers.org) are owner-account actions — **flagged for
  the owner in STATUS**. **Next: task-032** (projects → the database —
  its recorded trigger, the retirement of the byte-identity constraint,
  fired with this cut).
