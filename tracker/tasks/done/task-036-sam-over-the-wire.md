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
      CPU EP, 8-entry embedding LRU) — proven on port 8091 and through the
      boot-script path (8094, `SAMS_EP=cpu`, hash-preflighted)
- [x] the gateway matrix carries `kokon-sam` (CPU, on-demand boot,
      `checkEndpoint: /segment/capabilities`, `ttl: 86400` resident,
      member of **every** set — the sidekick pattern: never evicts, never
      evicted): window of 14:27 (11-22) staged/validated/swapped the
      v269 matrix (reload 19 s, vllm revived before the marker); the door
      boots on demand and is **keyed `GET :8081/upstream/kokon-sam/segment/
      capabilities` → `ready:true`** (504 MB RSS, co-resident with vllm)
- [x] the service's relay: a service with **no** local segment files +
      `externalEngine` set serves the lab over the wire — proven with a
      scratch service instance (fresh data dir, `externalEngine.gateway =
      http://127.0.0.1:8081/upstream` like the prod handover): `/segment`
      reports `installed:true` **via the relay**, encode `ready:true`, and
      predict returns the 4 reference masks
- [x] the dev box (apollo) still runs the lab **in-process** with its
      local files (no regression from the relay change; 420 qls-core
      tests, deployed box untouched at `fe302e7`)
- [x] parity, measured properly: service masks vs python-server masks,
      same image + click, compared on grayscale pixels — **1056768/1056768
      agreement (100.000%) on all four masks, IoUs identical**
  ([0.6762, 0.8965, 0.8496, 0.8106]); this required porting the service's
  exact resize (image-rs Triangle kernel, f32, half-away rounding) into
  the engine — PIL bilinear is not bit-identical (D-036-6)
- [x] prod-box + fresh-box docs carry the split reality (the serving box
      relays through the gateway base/key it already has; the 45 MB never
      lands there; the matrix entry is part of the handover)

## done record (2026-11-22, apollo half complete)

- **The engine** (`/home/p/ml/backends/kokon-sam/sam_server.py`): stdlib
  HTTP + numpy + pillow, its own venv (`onnxruntime-gpu` wheel, CPU EP);
  `predict` is self-healing — it carries the image bytes and re-encodes
  on a cache miss, so an engine unload between the browser's encode and
  click can't wedge the session (D-036-9). The engine never sees a path
  on any box.
- **The matrix** (v269 schema, the owner's overnight v2 config): entry
  `kokon-sam` + the `sam` var + `& sam` in all seven sets; the daemon's
  alphabetical port map puts it at **9116** (the staged comment's 9114
  was two off — a cosmetic fix for the next window, not worth a reload on
  its own). vllm moved 9117→9118 at the same reload; the gateway is the
  only thing that knows the map.
- **The gateway route contract (learned the hard way)**: v269 has **no
  bare `/<model>/…` prefix** — per-model paths live at
  `/upstream/<model>/<path>` (that's also what the service's configured
  `externalEngine.gateway` ends in). My first proof window 404'd on
  `/kokon-sam/…` and was fixed by pointing the proof (and the relay) at
  `/upstream/…`; `adding-a-model.md` had it right all along. The relay
  reuses the box's configured base (a sibling of the sdcpp doors: one
  base, one key, the door id in the path).
- **The relay** (`crates/qls-core/src/segment.rs`, `Relay`): blocking
  reqwest, 5 s connect / 120 s request (cold-boot tolerant), bearer only
  when the box configured a key, byte-transparent (raw image bytes in,
  the engine's JSON masks out), 3 unit tests against a std-thread fake
  door (key/no-key, error mapping, wrong-shape responses).
- **The split-box proof on one machine**: a scratch service (no local
  files, gateway pointed at apollo) produced masks **identical** to the
  in-process reference — the atlas half of the architecture demonstrated
  end-to-end before atlas exists.
- **Window protocol followed** (the 14:27 window): staged → `-validate`
  → atomic swap → `configuration reloaded` (19 s) → last-good re-seeded
  → proof → door left resident → **vllm revived (1-token probe) before
  the marker**. One window, no vllm-eviction risk (CPU co-tenant).

## owner decision (2026-11-22, after the split-box proof): the second engine
## is retired — the lab's model runs in-process on the serving box

The owner's question: "isn't sdcpp enough? why do we need another server? it
kinda beats the purpose of UI + external engine if we have engines, plural."
Agreed — the split overreached, and the record says why:

- **sdcpp cannot do it** — the sdcpp doors are generation engines (t2i/i2i/
  inpaint); MobileSAM is a segmentation model (point prompts → masks) and
  sd-cpp has no decoder or point input for it. Stock Kokon ran it in the
  browser (onnxruntime-web) — the thing that broke in a weak browser.
- **The split-box argument was made for the GPU generation models** (klein/
  qwen — they need the accelerator and already ride the sdcpp doors).
  MobileSAM is a different class: 45 MB, CPU, ~50 ms/click, ~1 GB RSS — it
  fits on any serving box and does not justify a bespoke third engine type
  (a Python server that is neither sdcpp nor llama.cpp) in the gateway
  matrix, nor putting a 50 ms click on the wire.
- **Decision: in-process on the serving box is the design** (what task-035
  built and what the deployed box already runs; both lab models — matte and
  segment — sit with the service). The relay stays in the service as a
  **dormant fallback**: it engages only for a box with no local files whose
  `externalEngine` points at a matrix that has the door — no standard box
  does. The `kokon-sam` door was first **unloaded via the API that evening**
  (no window, vllm untouched); the owner then asked for the config line gone
  too, and the **15:36 window** did it (79 s: staged from the ml repo's
  tracked HEAD config — the 036 diff was the only uncommitted drift, so the
  revert was the exact inverse — reload in 4 s, 404 proof, last-good
  re-seeded, vllm revived 69 s before the DONE marker). The matrix is back
  at the tracked 11-21 state (ml repo `207ddd5`, DCO-signed; both config
  files byte-identical to HEAD, the one-shot job dirs left untracked per the
  house convention), and the engine files stay on disk in
  `/home/p/ml/backends/kokon-sam/` (venv gitignored) as the dormant
  fallback's home — the boot script and server are the only things a future
  split-box deployment would re-point at.
- **The "seconds per click" was a measurement artifact, not the design.**
  The door's own server-side time: 50 ms warm / 0.57 s cold (journal). The
  1.07 s wall clock was curl's `Expect: 100-continue`: Python's
  `http.server` only answers 100-continue for HTTP/1.1+, the door was 1.0,
  so curl (which sends Expect for every POST > 1 KB) waited its full 1 s
  `--expect-100-timeout` before uploading a body that takes 4 ms. The Go
  gateway (the product path) answers 100 correctly (86 ms through it) and
  reqwest (the relay) never sends Expect — the product path was never slow.
  `sam_server.py` now runs HTTP/1.1 and answers 100-continue explicitly, so
  a direct curl is fast again (4–50 ms) if the door is ever revived.

## remaining (not apollo work)

The **atlas deployment itself** awaits owner-supplied details (atlas
address, nginx/TLS, the migrated data dir) — the handover in
`docs/service-on-prod-box.md` is written for it; nothing on apollo is
outstanding. With the owner's decision above, the serving box runs the lab
in-process (the files land with the data dir), so the next image build's
relay rides along dormant and the `kokon-sam` door is optional scenery.

## blockers

None — D-036-3 (CPU on the gateway) removed the vllm-eviction window.
