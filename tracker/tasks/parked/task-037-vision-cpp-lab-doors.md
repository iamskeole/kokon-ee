# task-037 — the lab's models as stock GGUF doors (vision.cpp)

**trigger (parked until it fires): the owner's go/no-go on the plan below** —
raised 2026-11-22 when the owner, digging on their own, found that Acly
(the same HuggingFace author of the two MobileSAM/BiRefNet ONNX exports the
lab already uses) also publishes **GGUF** builds of both models, runnable by
his **vision.cpp** — a stock, self-contained C++ inference library. Owner's
framing: *"the whole point is for us not to create and maintain an inference
engine but use stock inference engines — e.g. llama.cpp, vllm, and now this
vision.cpp."*

## goal

Move both lab models (MobileSAM click-to-select, BiRefNet-lite matte) from
**in-process ONNX on the serving box** to **stock GGUF models on the
accelerator's gateway matrix**, run by a stock inference engine (vision.cpp)
behind a small standard door — the same relationship sdcpp/vllm/llama.cpp
have to the matrix today. The serving box (atlas) stays the only custom
layer; the in-process ONNX path stays in the code as the single-box
fallback; the in-browser onnxruntime-web copy stays the last resort.

## what vision.cpp is (verified 2026-11-22, all links checked)

- **Repo**: github.com/Acly/vision.cpp — "Computer Vision ML inference in C++",
  MIT, 62 stars, created 2025-07, **last push 2026-04-02** (stable, not hot),
  3 open issues. Latest release **v0.3.0** (2025-12-08) ships a
  `linux-x64` tarball: `bin/vision-cli`, `lib/libvisioncpp.so` + ggml libs,
  **full headers, CMake package config, and a prebuilt `libggml-vulkan.so`** —
  no build toolchain needed to run it (only the driver's Vulkan ICD at
  runtime, which this box has: `/usr/share/vulkan/icd.d/nvidia_icd.json`,
  driver 620.70.06).
- **Not a llama.cpp fork**: a self-contained ggml-based library (ggml +
  stb-image vendored; Acly's llama.cpp as a submodule only for the ggml
  build). C++20, CMake; backends **CPU and Vulkan — no CUDA** (CMake has
  `VISP_VULKAN` and nothing else GPU).
- **Models**: MobileSAM, BiRefNet (full / lite / dynamic, 1024px; HR 2048),
  Depth-Anything V2, MI-GAN, Real-ESRGAN. The first two are exactly our lab
  models. Same provenance chain as today: MobileSAM = ChaoningZhang's
  Apache-2.0 model (the ONNX we run is also Acly's export of it); BiRefNet =
  MIT.
- **API** (`include/visp/vision.h`, all `VISP_API` C-callable):
  - `sam_load_model(path, backend_device)`, `sam_encode(model, image)`
    (embeddings cached on the model), high-level
    `sam_compute(model, point | box_2d)` → one `image_data` mask.
  - **Pipeline level** (what a door should use — the same building blocks as
    our in-process path): `sam_process_input`, `sam_process_point` /
    `sam_process_box` (→ 4-component prompt vectors), `sam_encode_image`,
    `sam_encode_points` (multiple), `sam_predict_mask` →
    `sam_prediction { tensor masks; tensor iou; }` (**all heads**),
    `sam_process_mask(mask_data, mask_index, target_extent)` (pick any head,
    resize to any size).
  - `birefnet_load_model`, `birefnet_compute(model, image)` → alpha matte.
  - So the full existing contract (N labeled points + optional box → 4
    masks + IoUs, at the requested size) is reachable; a door that exposes
    **all four heads** keeps the browser's mask selection byte-identical.
- **No HTTP server in the project** (CLI + C API + Python bindings only).
  The only non-stock code this project needs from us is therefore a thin
  **door wrapper** (HTTP ↔ the C API) — the same category as our qwen boot
  scripts and the matrix config, not an engine: no model math, no
  backends, no runtime of our own.

## models & numbers

| | today (in-process ONNX, FP32) | vision.cpp GGUF F16 |
|---|---|---|
| MobileSAM | 44.7 MB (28.2 + 16.5), CPU: ~0.4–0.6 s encode, 40–180 ms/click | **25.7 MB**; CPU 669 ms (their 6-core 5600X bench); **19 ms on a 4070 (their bench)** |
| BiRefNet-lite | 224 MB, CPU: ~4 s load, ~0.5 GB RAM, seconds per matte | **84.5 MB**; CPU 4.5 s (their bench, vs 7.0 s for our ONNX); **85 ms on a 4070** |

HF pins: `Acly/MobileSAM-GGUF/MobileSAM-F16.gguf` (26,898,624 B),
`Acly/BiRefNet-GGUF/BiRefNet-lite-F16.gguf` (88,647,936 B) — hash-pin on
adoption (SHA-256 into the catalog like the ONNX pair).

**Measured on apollo this evening (prebuilt v0.3.0 release, CPU only, zero
GPU touch, vllm untouched):** `vision-cli sam -b cpu` on the reference
lantern image: 1.1 s wall total (371 ms backend init + 32 ms model load +
**569 ms encode+decode** on the 8-core 5800X), model 25.7 MB, and the
Vulkan backend initialized fine against the 3090 (fp16, bf16, NV_coopmat2)
before I held it to CPU.

**Parity nuance (the one thing that would bite us):** the CLI's single-mask
output for the reference click (300,200) vs our in-process head 0:
**89.7% pixel agreement but IoU 0.268** — not a model disagreement: the CLI
emits its *own best head* (it self-reported 0.619/0.881/0.868 for its heads,
mirroring our 0.676/0.896/0.850/0.811), while our reference is head 0 (the
weakest head, by our S1 note). F16 vs FP32 plus head-choice difference.
Consequence, already built into the plan: the door exposes **all four
heads** (the pipeline API does), the service returns them in the exact
shape it does today, and the parity bar is **per-head selection equivalence**
(same rule picks the same object; IoU within a few points), not byte
parity — the F16 quantization makes 100% impossible and uninteresting.

## plan

**P1 — scratch proof on apollo. No matrix change, no vllm impact, ~a day.**
1. Pin vision.cpp to the 2026-04 main commit (or v0.3.0 if it proves
   equivalent); build `VISP_VULKAN=ON` (Vulkan SDK headers only at build
   time; the prebuilt lib works as the fallback). **Home: `backends/
   kokon-vision/`** (source + built door), per the box's engine layout.
2. Download the two GGUFs into **`models/gguf/Acly/{MobileSAM,BiRefNet-
   lite}/`** with a `provenance.md` in each (HF repo + pinned revision +
   SHA-256 + size) — the box's model convention; SETUP.md's model step is
   what re-creates them on a fresh box.
3. **The door**: `backends/kokon-vision/vision_server.cpp` (~300–500 lines
   over the C API, linked against the built lib): `GET /health`,
   `POST /sam/encode` (image bytes), `POST /sam/predict` (points/box → 4
   base64 PNGs + IoUs), `POST /birefnet` (image → 0/255 PNG). HTTP/1.1 with
   a proper `100 Continue` answer (the 11-22 Python-door lesson), `${PORT}`
   bound, one process hosting both models (they're ~110 MB combined).
4. **Bench, vllm resident**: per-call latency (CPU and Vulkan variants),
   `nvidia-smi` delta for the Vulkan run (weights ~110 MiB + ggml's arena —
   **the card is at 23.8/24.5 GB under vllm, so the Vulkan variant's budget
   is the free ~0.7 GB minus vllm's wobble — the number decides the set
   membership, see decisions**), and 1-token vllm probes before/after.
5. **Per-head parity** vs the in-process ONNX reference set (the existing
   test images + reference masks; the 100-continue-free harness).

**P2 — the matrix door (window protocol, only after P1's numbers and the
owner's call).** One entry `kokon-vision` (both models, one boot script,
one key, `checkEndpoint /health`, TTL unload — sidekick pattern). Set
membership per the P1 evidence (decision D-037-1 below). Window per the
standing protocol: stage → `-validate` → atomic swap → reload → last-good
re-seed → proofs → **vllm revived before the marker**.

**P3 — service integration.** A `visionEngine` config section (gateway base
+ key + door id — the D-036-10 shape: one base, one key, door id in the
path, sibling of the sdcpp doors). **Revive the relay** that left the tree
this afternoon — it is the right client for this door (the contract it
speaks, 4-mask PNGs + IoUs, is exactly what a vision.cpp door returns; the
deleted code is the starting point, git history has it). Capability, in
order: relay to the door → in-process ONNX (single box, the stock default
for boxes without an accelerator) → in-browser onnxruntime-web (last
resort). The browser contract is untouched either way.

**P4 — docs & handover.** fresh-box (the lab's models live on the
accelerator as a stock GGUF door; a single box stays in-process; the
45→26 MB + 224→85 MB shrink), prod-box handover, this file closed with the
numbers. **Durability (the owner's box-down question, answered 11-22):**
the ml repo tracks only text — the door source, the boot script
(`_scripts/serve-kokon-vision.sh`), the matrix entry, and the pin records;
SETUP.md's step 1 gains the 8th pinned checkout (vision.cpp) and its model
step the two `provenance.md` recipes, so a bare machine rebuilt from the
apollo-ml repo gets the door in the same pass as the gateway and every
other door. Nothing exists only on this box.

## decisions

- **D-037-1 (open — the owner's call): GPU or CPU door on apollo.**
  *CPU* variant: zero VRAM, **co-resides with vllm** (no eviction, no window
  drama; ~0.5 s encode / ~0.1–0.2 s per click — about where in-process sits,
  but on the accelerator, off the serving box, and ¼ the model bytes).
  *Vulkan* variant: 19 ms / 85 ms (their 4070 bench; ours to measure), but
  the card is 97% vllm → it can only follow the **qwen-doors' eviction
  pattern** (the lab session evicts vllm until its TTL; the owner's chat
  goes down for the lab's lifetime) or a tight-fit co-residency if the
  nvidia-smi delta leaves comfortable headroom. Leaning: **CPU first**
  (free, immediate, no eviction), Vulkan as a follow-up with the exclusion
  set if the owner wants the 19 ms.
- **D-037-2 (proposed): one door, both models** (`kokon-vision`) — one
  process, ~110 MB, one boot/TTL lifecycle, two endpoint families; a door
  per model would double the matrix footprint for no isolation gain.
- **D-037-3 (proposed): keep the in-process ONNX path** as the single-box /
  no-accelerator fallback (it is built, tested, and the stock box's only
  way); this task *adds the door as the first-choice path on split boxes*,
  it does not delete anything. (Contrast with 11-22: that door was bespoke
  Python; this one is a stock engine — different category, per the owner's
  own principle.)
- **D-037-4 (proposed): the parity bar is per-head selection equivalence**
  (above), because F16 ≠ FP32 byte-for-byte and the heads' IoUs are
  unreliable by the model's own S1 note.

## acceptance criteria

- [ ] P1: the door boots from a script on `${PORT}`, answers `/health`, and
      runs both GGUFs (point, multi-point, box, matte) on apollo with vllm
      resident and revived; nvidia-smi delta recorded.
- [ ] P1: per-head parity vs the in-process reference within the D-037-4 bar
      on the standing test set.
- [ ] P2: `kokon-vision` in the live matrix (window protocol, vllm revived
      before the marker); the matrix's last-good re-seeded.
- [ ] P3: a service with no local ONNX files but a configured door runs the
      whole lab over the wire (the split-box proof, again, with the stock
      engine); in-process and in-browser paths unchanged and tested.
- [ ] P4: docs current; this file closed with the measured numbers.

## blockers

- The owner's go/no-go on the plan (especially D-037-1) — nothing here
  touches the live matrix or vllm until P2, which won't start without it.
- P1 needs the Vulkan SDK headers for the GPU build (apt; trivial) and ~1
  GB of /tmp for the build — both fine on apollo.
