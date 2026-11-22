# task-036 — the lab's selection model over the wire on the split box (owner-assigned 2026-11-22; atlas serves, apollo computes)

The owner's demo architecture: **atlas** (a 2011 Mac mini, the stable box)
serves the UI behind nginx and stores the app data; **apollo** (this box)
runs every model. "All heavy lifting needs to happen over the wire." So on
the split box the lab's MobileSAM must not run on atlas at all (a 2011 CPU
running 45 MB of ONNX is the wrong machine for it), and the owner asked
whether it belongs **in the gateway matrix**. Yes — this task builds it.

Scope:

1. **A SAM engine in apollo's gateway matrix** — a small server
   (`/home/p/ml/backends/kokon-sam/`, own venv) that loads the same two
   hash-pinned MobileSAM files and serves the door `kokon-sam`:
   `GET /segment/capabilities` (the `checkEndpoint`),
   `POST /segment/encode` (image bytes → embeddings, LRU-cached) and
   `POST /segment/predict` (points → 4 masks + IoUs, the 0/255 PNG
   contract the UI already consumes). Boot script in `_scripts/`
   (dash-safe, sha256-preflighting), matrix entry (on-demand boot,
   `ttl` unload — the card returns to vllm when the lab idles).
2. **The service's relay** — `segment_encode`/`segment_predict` in
   qls-core: local files run in-process (single-box, unchanged);
   **no local files + an external engine configured → relay to
   `{gateway}/kokon-sam/segment/*`** (the same `{gateway}/{model}`
   client design the sdcpp EE path uses). `/segment` status reports
   `installed` when either source is usable, so the browser's
   preference logic (D-035-1) is untouched: it asks its own service;
   the service decides where the model runs. Atlas stores no 45 MB,
   runs no ONNX — the prod handover's guardrail stays true.

## decisions

- **D-036-1 — capability, one level up**: the box's own pinned files win
  (in-process); otherwise the configured gateway's `kokon-sam` door
  (split-box); otherwise the in-browser fallback remains the last
  resort. No new user setting.
- **D-036-2 — a Python engine, not another Rust ORT build**: apollo's
  system `libonnxruntime` ships no CUDA provider; the official
  `onnxruntime-gpu` wheel (own CUDA runtime, into the backend's venv)
  is the stock path. The server is stdlib-HTTP + numpy + pillow — it
  *is* the engine; the boot script preflights and execs it,
  daemon-tracked like every other matrix model.
- **D-036-3 — CPU on the gateway (owner-decided 2026-11-22)**: CUDA is
  flaky here and shows no perceivable speed win over CPU for this
  model size; the owner ruled that CPU keeps the matrix simple (no
  VRAM, no evicting vllm, no window protocol) — the card stays vllm's,
  always. The engine still takes `SAMS_EP=auto|cuda|cpu`; the matrix
  entry runs `cpu`. If a timed CUDA run ever shows a perceivable win,
  one env var re-arms it.

## the all-255 investigation (2026-11-22 — closed as a test-harness false alarm)

While proving parity for the relay, a client check of the **deployed**
service's masks reported every mask as all-255 (1056768/1056768 on at
1376×768) while the IoUs were correct and a native sample on the same
tree looked fine. A day of forensics (instrumented rebuilds, ORT
arena/thread variants, `MALLOC_PERTURB_`, ORT-version A/B across
glibc) found no corruption — because **there was none**:

- The client counted pixels with `sum(1 for x in Image.open(...).getdata() if x)`
  on an **RGB-mode** PNG. `getdata()` yields 3-tuples there, and a
  non-empty tuple like `(0,0,0)` is *truthy* in Python — so **every**
  pixel of an RGB image counted as "on", whatever its value. The
  service's masks are written as RGB luma; the native sample writes
  grayscale (ints, where 0 is falsy), which is why the sample counted
  correctly and the service appeared to "flip" — the asymmetry was in
  the test, not the box.
- Proof: the received "all-255" PNG's grayscale histogram is
  **1016569 zeros + 40199 ones** — exactly the correct lantern mask
  (the sample's own numbers); the earliest "corrupt" mask saved from
  the first rebuilt image is a valid partial mask; repeated runs
  against the live deployed box (39151, `fe302e7`) return
  40199/17688/148237/558506 with IoUs 0.676/0.896/0.85/0.811 —
  identical to the native sample and to the Python engine, run after
  run, no flips.

Consequences, recorded so nobody re-litigates it:

1. **The in-process service SAM (task-035, images `95450c1`/`fe302e7`)
   was correct from the start.** The owner's 11-22 browser complaint
   was the LAN-allowlist 403 (fixed in `fe302e7`), nothing else.
2. The investigation's speculative changes (arena-disabled sessions,
   instrumentation) were **all reverted**; the one keeper is a
   defensive copy of the masks tensor in `segment.rs`
   (`try_extract_tensor` yields a view; copying before the iou
   extraction matches every other ORT consumer of this tree).
3. **Test rule for the record: pixel counts on PIL images require
   `.convert("L")` first** (or per-channel logic) — a tuple is never
   "off" in Python.

## acceptance

- [x] the python engine works standalone (capabilities, encode, predict,
      CPU EP, 8-entry embedding LRU) — proven on port 8091
- [ ] the gateway matrix carries `kokon-sam` (CPU, on-demand boot,
      `checkEndpoint: /segment/capabilities`, ttl unload) and boots it
      on demand: keyed `curl :8081/kokon-sam/segment/capabilities` → 200
- [ ] the service's relay: a service with **no** local segment files +
      `externalEngine` set serves the lab over the wire — `/segment`
      reports installed via the relay, and encode + predict through
      `{gateway}/kokon-sam` return the same masks as the in-process run
      (live proof, CPU)
- [ ] the dev box (apollo) still runs the lab **in-process** with its
      local files (no regression from the relay change)
- [ ] parity, measured properly: service masks vs python-server masks,
      same image + click, compared on grayscale pixels (the harness of
      this task, not the RGB-tuple one)
- [ ] prod-box + fresh-box docs carry the split reality (atlas relays;
      the 45 MB never lands on atlas; the matrix entry is part of the
      handover)

## blockers

None — D-036-3 (CPU on the gateway) removed the vllm-eviction window.
