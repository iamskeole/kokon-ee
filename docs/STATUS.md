# Status

**Updated 2026-11-23.** This page is the fork's view. The upstream project's status (its
releases, its issues) is the origin's; the working state **of this fork** — what is in
progress, next, the parked triggers — is [tracker/STATUS.md](../tracker/STATUS.md), which
supersedes this page, and the current task is always the oldest file in
[`tracker/tasks/todo/`](../tracker/tasks/todo/).

## What this fork is

Kokon EE: the Kokon service unbundled from its bundled engine. One custom layer — the
service (this repo) — plus stock upstream engines (stable-diffusion.cpp, llama.cpp,
vision.cpp) scheduled by the box's gateway. The product is the box: the Docker image
(service + browser UI + MCP), `config.json` pointing at the gateway's entrypoints, and
the prompts.

- The split-box deployment (service box + engine box) is the supported shape:
  [docs/fresh-box.md](fresh-box.md), [docs/service-on-prod-box.md](service-on-prod-box.md).
- The lab (segment, matte) runs on the service box through the `kokon-vision` door or
  in-process; the browser never runs a model.
- The vision.cpp server is upstreamed as a PR
  ([Acly/vision.cpp#25](https://github.com/Acly/vision.cpp/pull/25)); until it merges,
  the fork's pinned build (ml repo, `backends/kokon-vision`) carries it.

## Recently shipped on this fork

- task-037: the `kokon-vision` lab door, the in-process lab, the service's 2×2 capability
  matrix, the vision.cpp server PR.
- task-038: upscalers on the engine box (the split box asks the engine, not the service).
- task-031: the chaff cut — the product is the box. The standalone app (Tauri shell, Mac
  engine, in-app updater, bundled engine management, the setup wizard, the release and
  marketing machinery) is out; what remains is the service, the browser UI and the MCP.
