# AGENTS.md

**Project mission (kokon-ee — read before anything else).** This project
unbundles Kokon from shipping built-in engines: the service in this repo is
the only custom layer; every inference engine is **stock upstream**
(stable-diffusion.cpp, llama.cpp — the host's `backends/` named slots)
scheduled by the host's gateway (llama-swap); the service points only at
**entrypoints, API keys, and prompts**. Where this file (upstream text below)
and the project direction disagree, **the project direction wins** — notably,
"Kokon's own build of sd-server" in the architecture below is the upstream
bundled design; here engines are external and stock. The rest of this file
(build/test, UI conventions, contracts, the model-change quality policy)
stays in force for our work.

**Working state.** The project's plan is [tracker/PLAN.md](tracker/PLAN.md); what
is in progress here is [tracker/STATUS.md](tracker/STATUS.md), which
supersedes `docs/STATUS.md` here. **The current task is always the oldest
file in [`tracker/tasks/todo/`](tracker/tasks/todo/)** — when asked to "pick
the next task": move that file to `tasks/doing/`, work it to its
acceptance criteria, move it to `tasks/done/`, and update `STATUS.md`.
Tasks waiting on a trigger live in
[`tracker/tasks/parked/`](tracker/tasks/parked/) — they enter `todo/` only
when their trigger fires, so they never become the current task. Each task
carries `## blockers` and `## decisions` — surface blockers before
guessing, and record new decisions in the task file. `tracker/`
is project-owned and is never contributed upstream.

**Rebase and release rules (owner-noted 2026-11-06 — do not litigate).**
Rebases always target **upstream master**, never a release commit
(upstream doesn't tag its releases; master is by definition the latest
release plus harmless post-release work). **This project does no releases** —
releases are the origin's/author's view (their installers, their tags);
if the upstream PR doesn't land, our release mechanism is: clone the project
→ `docker build` → `docker compose up -d` (the service image is the
product; `docs/fresh-box.md`).

**Commit discipline (this host, this repo).** DCO-signed commits
(`git commit -s` — no GPG key here); **never run `cargo fmt`** (no
rustfmt.toml on this host — hand-format only); one small commit per
logical unit; tracker changes (task file moved, `STATUS.md` line)
committed with the work they describe; BFL prompt/template strings never
appear in chat-visible diffs, logs, or repo docs — they live in assets
and are referenced, not inlined.

**Autonomy protocol (long / GPU jobs — engaged when the owner kicks one
off; the full version lives in the host's OPS notes, `/home/p/ml/OPS.md`).**
One 24GB GPU is shared with the operator runtime (vllm); evicting it kills
it, and the revival is the reload. A long GPU job:
1. is a self-contained **POSIX-dash** job script (no arrays or `[[` — dash
   parses incrementally, so a bash array makes it run the clean prefix and
   then die mid-script with no marker; check with `sh -n`, not just
   `bash -n`): frees the GPU, starts the model under
   test, waits on a **1-token `/v1/completions` probe** for true
   readiness (200 = ready, 503 = still loading — `/health` lies), runs
   the tests, writes a digest, stops the model, **revives vllm
   canonically** (its `serve.sh start --no-wait` — never a gateway
   on-demand curl, that trigger drops), **and writes the `DONE`/`FAIL`
   marker last** (the marker means "the runtime is back");
2. is launched with `setsid sh /path/job.sh < /dev/null > /path/job.log
   2>&1 &` (a surviving host process — never `setsid sh -c '…'`);
3. gets a second setsid watcher as backstop (hard cap ≤3h; fallback
   digest + revival if the job dies);
4. ends the agent's turn in a **blocking wait that is the last step of the
   LAUNCH tool call itself** — the same bash invocation that setsid-launches
   the job and the watcher then runs
   `sh /home/p/ml/wait-marker.sh /tmp/<jobdir> <cap>`; bash tool timeout =
   cap + 120 s, where **cap exceeds the job's worst case (the watcher cap
   + revival margin), not its expected runtime** — a timeout firing while
   vllm is down kills the session for good, and a turn ended with no pending
   wait never resumes (both observed 2026-11-03; four consecutive windows
   that ended their launch turn before the wait, the last leaving the owner
   mid-crisis — a launch not immediately followed by the wait in the same
   call is a process failure, not a judgment call). While it blocks the
   agent is suspended, not polling; the
   harness's result injection — marker + digest + log tails + live state
   in one payload — is the nudge that continues the work without the owner;
5. tolerates the agent dying mid-job (the job and the watcher are host
   processes; the revival revives the runtime regardless);
6. neutralizes in the gateway registry any model the window must keep down
   (config swap), or vets it in a loop: the daemon brings dead registry
   models back by re-running their cmd (three vLLM resurrections inside the
   2026-11-03 pair window; the same day's no-registry window was clean),
   and a gateway unload cannot stop a model started outside the daemon.
   **When removal is in effect the job's defense is a YIELD, never a
   kill-veto**: with vllm out of the registry the daemon cannot bring it
   back, so any vllm that appears mid-window is a human reclaiming the GPU
   — the job dies cleanly and the restore leaves a healthy vllm alone.
   Killing a human's deliberate boots is the protocol's worst failure mode
   (2026-11-03 qwabl: three manual boots killed in a row, 20:52-20:55, ~21
   min of owner chat lost). The owner's sessions stay on qwen-direct for the
   window's duration (verified 2026-11-03: qwen-direct is truly direct,
   18020/v1 — it cannot trigger loads, but its turns fail while the model
   is down).
It exists to kill the proven failure modes: poll loops, "background it
and hope the owner asks", false-DONE on `/health`, crashed jobs
stranding the GPU, a job that dies on a parse error before any marker
(interpreter/dialect mismatch — the watcher's silence fallback recovers
the box; kleenq-fail1, 2026-11-03), a script created without its exec bit
(the daemon's fork/exec fails `permission denied` at info level while the
job's waits just 500 — preflights exec-audit every cmd/cmdStop script;
2026-11-03 qwabl, 13 min), premature markers, the job killing the
owner's manual boots (yield rule, item 6), and a turn that ends idle after
a launch (the wait is part of the launch call, item 4).

The entry point for anyone working on Kokon EE: people and coding agents (Claude Code, Codex,
Cursor and others). Read this first, then the fork blocks at the top for what is in progress.

## What the box is

Kokon is the simplest local photo editor: **edit photos by describing the change**. The product
of this fork is the box: a Docker image (or bare binary) that serves the browser UI and the MCP
server, and points at the box's gateway, which schedules the stock inference engines. The user
opens a browser, signs in, brushes an area (or not) and types what they want.

- **We pick the best model, the user picks nothing.** Today that is Qwen-Image 2.1 Turbo
  (8 steps, CFG 1) on the engine box, with three quality presets. No settings to tune: sizes,
  memory and presets are decided by the code.
- **Expert controls are opt-in only**, in Settings, off by default, never next to the prompt box.
- **A request fits if it helps someone who has never used ComfyUI.** Power-user options go to
  the bottom of the list.
- **Changing the default model** needs a model that beats the current one overall on
  text-to-image, edit, multi-image edit and memory use, on the same test set; the old model
  stays selectable, with a rollback. Proposals: the upstream project's Discussion #35.

## Architecture in one minute

```
browser UI (React, embedded in the service)      kokon-mcp (mcp/, stdio)      scripts
        └──────────────────────────── loopback HTTP + SSE, token / password ─┘
                                       ▼
   kokon-service (crates/qls-service + qls-core)  127.0.0.1:39151 (token) · LAN :39152 (password)
   planner · pipeline · job queue · gallery · the lab (MobileSAM, BiRefNet, in-process or door)
                                       │  /sdcpp/v1/* through the box's gateway
                                       ▼
   the gateway (llama-swap) — stock engines, loaded/evicted on demand:
       stable-diffusion.cpp (the image model + its VLM sidecars) · vision.cpp (the lab door)
```

- `crates/qls-core`: all the logic. `sdcpp.rs` is the only file that knows sd-server's request
  format; `catalog.rs` pins the service box's downloads (upscalers, watermark, lab GGUFs);
  `planner.rs` decides sizes and memory; the lab is `matting.rs`/`segment.rs` + `onnx.rs`.
- `crates/qls-service`: the headless HTTP service, single instance; `remote.rs` embeds the
  browser UI and runs the LAN listener. `src/`: the browser UI (React), logic in
  `src/lib/logic.ts`. `mcp/`: MCP server (TypeScript, official SDK).
- The engines are **stock upstream** and live on the box, not in this repo: the gateway's
  config names them (pinned in the ml repo). The service adopts the image engine through the
  gateway and never owns a process.
- There is no desktop app, no bundled engine, no in-app updater, and no browser-side model:
  the capability ladder for the lab is **door → in-process → a hard, honest error**.

Module map and the measured facts behind each default:
[docs/engineering-notes.md](docs/engineering-notes.md).

## Build and test

Setup and the full commands are in [CONTRIBUTING.md](CONTRIBUTING.md). In short:

```bash
cargo build -p qls-service            # the service (embeds the built UI)
npm run build && cargo build -p qls-service --release   # or the whole image:
docker build -f docker/Dockerfile -t kokon-service .
docker compose up -d                  # the product, on this box
```

Tests, before every commit that touches code:

```bash
cargo test -p qls-core
npm test                 # UI logic (vitest)
cd mcp && npm test       # MCP server (the protocol test needs a built kokon-service)
```

Most changes (planner, geometry, UI logic, MCP) are testable without a GPU. Anything that runs
the model for minutes (real inference, benchmarks) runs on the box through the gateway — the
long-job protocol above applies.

## Conventions

- Logic in `qls-core` (or `src/lib/logic.ts`), where it can be unit-tested; the service and
  the MCP stay thin.
- **Measured defaults carry their source.** If you change a size, memory budget or backend
  flag, say in the comment what you measured it on (GPU, OS, bench). Comments explain *why*.
- Add or update a test for behaviour you change. Tests never touch real user folders.
- UI text and docs: plain, short English. Follow the style of the file you edit (strict
  TypeScript; on this host hand-format Rust — never `cargo fmt`).
- Methods must generalize: no constants tuned on one image; every step falls back safely when
  it doesn't apply.
- Don't commit model files or large binaries. Code is AGPL-3.0-only; contributors sign the
  [CLA](CLA.md).

## Where the docs live

| Doc | What |
|---|---|
| [README.md](README.md) | the product for users: run it on a box, features, how it works, FAQ |
| [docs/STATUS.md](docs/STATUS.md) | the fork's status page (the working state itself is [tracker/STATUS.md](tracker/STATUS.md)) |
| [CONTRIBUTING.md](CONTRIBUTING.md) | build, tests, code style, licences |
| [docs/engineering-notes.md](docs/engineering-notes.md) | architecture, module map, the engine door, the lab, verified backend facts |
| [docs/fresh-box.md](docs/fresh-box.md) | the whole stack on a new machine (fork layout): the pinned gateway + matrix config, the stock engines, the models, the service image |
| [docs/service-on-prod-box.md](docs/service-on-prod-box.md) | the split deployment: the service + browser UI on the stable box behind nginx, all inference on the accelerator through its one-door gateway — with the paste-able handover prompt, compose/config/nginx references, and the verification checklist |
| [docs/http-api.md](docs/http-api.md), [docs/remote-access.md](docs/remote-access.md), [docs/docker.md](docs/docker.md) | the service API, LAN access, the image |
| [docs/prompting.md](docs/prompting.md), [docs/showcase-prompts.md](docs/showcase-prompts.md) | prompt guide and examples |
| [docs/ai-marking.md](docs/ai-marking.md) | the AI watermark and provenance checks |

## Keeping the status pages current

At the end of every task that changes what users get or what is planned: update
[tracker/STATUS.md](tracker/STATUS.md) (the fork's working state) and, for user-visible
changes, [docs/STATUS.md](docs/STATUS.md) and the README, with the "Updated" date moved.
Check the items against the repos (`gh`) rather than memory: a status page must never claim
something the repo doesn't show.

If a `private/` folder exists, read `private/AGENTS.md` next (maintainer only).
