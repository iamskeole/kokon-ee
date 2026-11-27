# Status

**Updated 2026-11-27.** This page is the fork's view. The upstream project's status (its
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

- task-043: **H3's UI data and honest estimates** — the owner's three post-042 questions
  (with screenshots): (1) does the UI cap H3 at 16 refs? The service did (the row's cap),
  the UI didn't — it read the stale qwen profile (10). The /config reply now carries the
  active model's derived row (`activeModel`), and the UI's refs cap and the quality-preset
  labels read from it: H3 shows ~4 MP / ~8 MP / **native 16 MP · slowest** instead of the
  qwen strings, and 16 refs; (2) the "native 2K" preset label was a hardcoded qwen story —
  the presets now follow the model's own tiers (the qwen triple keeps its historical
  labels; the native mark follows the model's top resolution tier, the size menu's rule);
  (3) the ~2 m 9 s / ~9 min estimates came from the qwen-calibrated time model run at
  Turbo's 8 steps with qwen's sigmas, scaled by a qwen-learned machine factor — for H3 the
  plan now is the job (the door's 6 steps, its built-in schedule, no sigmas of its own),
  the speed book calibrates **per model**, and on a split box whose GPU the service cannot
  see the uncalibrated estimate hides ("time known after the first image") until the
  model's own first measured job.
- task-042: **H3's card, enforced** — the PE's reply is now grammar-constrained per
  request (the makers' strict JSON schema, the card's thinking parameters per request),
  the door sized to the card's 16-reference maximum (`-c 65536`, measured 41.3k prompt
  tokens at the max), the picker entry gained its capability flags — no brush (the H3
  card has no mask path; a mask sent through the raw API is dropped with a note) and its
  own resolution list with a true **4K / native 16 MP** tier (new `4k` mode) — and the
  H3 contract itself now keys on the bundle family, not the model id. MTP on the PE is
  ruled out at the source (the makers' release ships no MTP head).
- task-041: **MiniMax H3 Image** as a picker entry — the makers' 6-step/no-CFG recipe at
  up to 16 MP (measured to fit a 24 GB card at 4096²), its Prompt+ door the makers' own
  PE fine-tune GGUF (one VLM door serves t2i and edit; the two big doors take turns on
  the card through the gateway's eviction dance), and the 9B Qwen PUPs confirmed as the
  Qwen 2.1 pipeline's own (they don't speak the H3 caption contract). The default model
  is unchanged.
- task-037: the `kokon-vision` lab door, the in-process lab, the service's 2×2 capability
  matrix, the vision.cpp server PR.
- task-038: upscalers on the engine box (the split box asks the engine, not the service).
- task-031: the chaff cut — the product is the box. The standalone app (Tauri shell, Mac
  engine, in-app updater, bundled engine management, the setup wizard, the release and
  marketing machinery) is out; what remains is the service, the browser UI and the MCP.
