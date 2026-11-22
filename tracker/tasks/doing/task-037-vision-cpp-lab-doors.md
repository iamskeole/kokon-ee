# task-037 — the lab's models as stock GGUF doors (vision.cpp)

## status: **doing** — GO received 2026-11-22. **P1 complete 2026-11-22 (evening):**
fork at `backends/kokon-vision/` (4 commits on upstream `26a7529`, branch
`vision-server` pushed to the fork) — `vision-server` (all five families,
4 SAM heads, embed cache, 100-continue) + `util/json.h` + base64 in
`util/string.h` + **the per-request device-memory budget check** (dry-run
of the graph allocator, 503 with the measured size before any allocation)
+ test-json (19) and test-server (10, functional, incl. the budget
refusal). All C++ ctest green (29 total, 5x flake loop clean), ASAN build
clean under a 60-call point/box mix (two memory bugs found and fixed via
ASAN: dangling `elapsed_str` stack pointer; use-after-free reading a freed
ggml tensor header from the embed cache). **All five families verified
end-to-end on CPU over the API** (1376×768): measured per-request graphs
188 / 1332 / 742 / 211 / 217 MB (all inside the 4 GB default), 0.65 / 2.72 /
4.99 / 0.64 / 2.58 s, semantic checks pass (bimodal matte, full-range
depth, masked-only inpaint change, 4× upscale); per-head SAM parity on the
standing click met (D-037-4). **Model artifacts kept per the owner's word:**
`models/gguf/Acly/<5>/` on the ml box with provenance (ml `582bcd0`).
**PR body revised and re-shown** (`task-037-vision-cpp-pr-body.md`):
`Signed-off-by` out per D-037-10, budget-check story and the all-five E2E
in; **not opened without the owner's sign-off (D-037-7/9).** Open items:
Vulkan build needs `glslc` (owner's one-liner; `libvulkan-dev` is already
installed), and P2 (the matrix door) is gated on the owner's ruling — with
vllm resident on this box the door runs **CPU** (everything fits, no VRAM),
which D-037-8's fallback makes acceptable; Vulkan co-residency on this
card is knife-edge at best (measured, below).

## trigger (fired 2026-11-22): the owner's go/no-go on the plan below —
raised 2026-11-22 when the owner, digging on their own, found that Acly
(the same HuggingFace author of the two MobileSAM/BiRefNet ONNX exports the
lab already uses) also publishes **GGUF** builds of both models, runnable by
his **vision.cpp** — a stock, self-contained C++ inference library. Owner's
framing: *"the whole point is for us not to create and maintain an inference
engine but use stock inference engines — e.g. llama.cpp, vllm, and now this
vision.cpp."* **GO (same day, evening):** *"go on P1. After you've built and
tested the code, show me the PR body before submitting — do not submit
before I sign off. Make sure you follow either specific contribution
guidelines the author specifically asks for if those exist, if not try to
match their own coding style if possible and finally if thats too
cumbersome i guess something close to llama.cpp server style would make
sense."* Plus the co-residency ruling: *"vllm + kokon work will never run
inference concurrently … just given the size delta i wouldn't want to
evict vllm just for a click on an image."*

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
  build). C++20, CMake; backends **CPU and Vulkan — no CUDA**, and that is
  the project's deliberate design, not a gap: one build for NVIDIA/AMD/
  Intel. (Owner's "why not CUDA?", answered 11-22 — D-037-6: the build
  system has exactly one GPU option, `VISP_VULKAN`; the release bundles
  only the vulkan + cpu backends; it benches at parity with CUDA on these
  models (their 4070 table: SAM 19 vs 16 ms, BiRefNet-lite 85 vs 84 vs
  PyTorch-CUDA); the box's NVIDIA driver already ships the Vulkan ICD — a
  runtime separate from vllm's CUDA context. A CUDA port would be engine
  work, which we have agreed not to do; if it ever matters, that's an
  upstream conversation.)
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
- **No HTTP server in the project** (CLI + C API + Python bindings only)
  — until our fork adds one (D-037-5): a generic `vision-server` built
  into the project and submitted upstream as a PR. If it merges, the door
  runs the upstream release — zero code of ours anywhere, the sdcpp-shaped
  ideal. If not, we run our fork pinned at `upstream-commit + the server
  delta`, rebased as upstream moves (the llama-cpu pattern, aimed at
  dissolving). The PR stays generic (all five model families, plain JSON,
  no Kokon naming) and its tests cover large POSTs with
  `Expect: 100-continue` — the hole the 11-22 Python door fell into.

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

**Measured 2026-11-22 evening (both numbers below settle the two open P1
questions):**

- **Vulkan co-residency delta (D-037-1).** Prebuilt v0.3.0 `vision-cli`
  with `-b gpu`, MobileSAM F16, vllm resident (23878 MiB used / **268 MiB
  free**, probe 200 before and after — vllm never touched): backend init
  150 ms, **weights load into VRAM in 14.5 ms (f16)**, then inference dies:
  `vk::Device::allocateMemory: ErrorOutOfDeviceMemory` on a **4.5 MB**
  arena chunk, with nvidia-smi peaking at **24144 MiB** — i.e. MobileSAM
  Vulkan wants **~266 MiB** (25.7 weights + ~240 ggml arena/activations),
  2 MiB over the stable free budget, and it is the *last small chunk* that
  fails, so it is borderline, not close. Verdict: **uncapped Vulkan does
  not co-reside with vllm's current footprint.** The server's
  `--max-arena` cap is the lever: 26 + ≤200 ≈ ≤226 MiB would fit under
  268 — proving it needs a Vulkan build of the capped code (SDK headers
  only; blocked on the owner's `sudo apt install libvulkan-dev`). Until
  then the door's default on this box is **CPU co-resident** (0.62 s/click
  measured, zero VRAM — D-037-8's fallback); if the owner ever wants the
  19 ms Vulkan variant, the lever is trimming vllm's KV budget by a few
  hundred MiB, which is the owner's call.
- **Per-head parity (D-037-4), the standing lantern click (300,200), CPU:**
  our `vision-server` returns **IoUs [0.619, 0.881, 0.868, 0.794]** —
  **bit-identical to the prebuilt stock CLI v0.3.0** (0.619/0.881/0.868 +
  its 4th head), which independently validates our pipeline code. Against
  the in-process ONNX reference **[0.676, 0.897, 0.850, 0.811]**: per-head
  |Δ| = **0.057 / 0.016 / 0.018 / 0.017** (max on head 0, the weakest
  head), and **both pick head 1 as best** (0.881 vs 0.897) — the D-037-4
  bar (same rule, same object, IoU within a few points) is met on the
  reference click.
- **All five families, CPU E2E, measured 2026-11-22 (Ryzen 5800X, one
  server process, all five models loaded, vllm resident, zero GPU touch):**
  true per-request graph sizes from the server's allocator dry run —
  SAM **188 MB** (0.65 s), BiRefNet-lite **1332 MB** (2.72 s), Depth-V2-
  Small **742 MB** (4.99 s), MI-GAN **211 MB** (0.64 s), ESRGAN **217 MB**
  per 1024² tile (2.58 s) — all inside the 4096 MB default budget, all
  200. (An earlier naive sum of tensor sizes said 5.6–24 GB: this ggml's
  allocator reuses in-place ops' parents, so the dry run is the only honest
  measure — that finding is why the budget check dry-runs instead of
  summing.) Semantic checks: the BiRefNet matte is 99.4% extreme (clean
  bimodal alpha); depth spans 0–254 (std 73.6); MI-GAN changes the masked
  region (mean Δ 20.9) and is bit-identical outside it (0.00); ESRGAN is
  4× and not nearest-neighbour. On a 15 GB RAM box the heaviest (BiRefNet)
  peaks ~1.3 GB — comfortable.
- **Q8/quantization question (owner, 11-22, answered with measurements):**
  Acly publishes **F16 only** for all five families (no Q8/Q4 files exist
  upstream). The ggml-vulkan kernels do implement Q4_0/Q8_0 (matmul FAs,
  incl. the 3090's coopmat path), so quantized GGUFs would run — but
  quantization is not the co-residency lever: the weights are a small share
  of the graph (MobileSAM 26 MB of 188; BiRefNet 89 MB of 1332) and the
  activations stay fp16/fp32 regardless. Making our own Q8 exports would be
  engine work — against the stock-engine principle.

## plan

**P1 — scratch proof on apollo. No matrix change, no vllm impact, ~a day.**
1. **The fork (D-037-5):** fork `Acly/vision.cpp` → `iamskeole/vision.cpp`
   at the 2026-04 main commit and build the **server into the project** —
   a `vision-server` binary next to `vision-cli` (vendored single-header
   HTTP, JSON API for all five model families, all four SAM heads exposed
   with their indices, HTTP/1.1 answering `Expect: 100-continue` — the
   Python door's lesson, tested), **and a hard arena cap** (e.g. 512 MB:
   fail the door's own startup check rather than over-allocate a shared
   card — the co-residency story's safety valve; D-037-1). Home on the box:
   `backends/kokon-vision/`
   = the fork checkout (upstream + our delta). **Draft the upstream PR in
   the same pass** (generic, no Kokon naming, with the test suite); the
   owner sends it or we do, per their word. Build `VISP_VULKAN=ON` (Vulkan
   SDK headers only at build time; the prebuilt lib works as the fallback).
2. Download the two GGUFs into **`models/gguf/Acly/{MobileSAM,BiRefNet-
   lite}/`** with a `provenance.md` in each (HF repo + pinned revision +
   SHA-256 + size) — the box's model convention; SETUP.md's model step is
   what re-creates them on a fresh box.
3. **Bench, vllm resident**: per-call latency (CPU and Vulkan variants),
   **the `nvidia-smi` delta for the Vulkan run is the decision number for
   D-037-1** (weights ~110 MiB + activations + the capped arena, against
   the stable ~0.7 GB vllm left free — under ~0.4 GB ⇒ co-resident is the
   default), and 1-token vllm probes before/after.
4. **Per-head parity** vs the in-process ONNX reference set (the existing
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
the ml repo tracks only text — the fork pin (repo + commit, the PIN-file
pattern), the boot script (`_scripts/serve-kokon-vision.sh`), the matrix
entry, and the model pin records; the server code itself lives in the
`iamskeole/vision.cpp` fork (GitHub) — or in upstream once the PR merges.
SETUP.md's step 1 gains the 8th pinned checkout (the vision.cpp fork — or
upstream, once merged) and its model step the two `provenance.md`
recipes, so a bare machine rebuilt from the apollo-ml repo gets the door in
the same pass as the gateway and every other door. Nothing exists only on
this box.

## decisions

- **D-037-1 (open — the owner's call): GPU or CPU door on apollo.**
  *CPU* variant: zero VRAM, **co-resides with vllm** (no eviction, no window
  drama; ~0.5 s encode / ~0.1–0.2 s per click — about where in-process sits,
  but on the accelerator, off the serving box, and ¼ the model bytes).
  *Vulkan* variant: 19 ms / 85 ms (their 4070 bench; ours to measure). The
  co-residency question, refined on the owner's point (11-22 evening): the
  110 MB of GGUFs is only the *weights*; realistic peak for both models in
  one process is ~0.3–0.5 GB (BiRefNet's activations dominate), and the
  card's free ~0.7 GB is a **stable** budget (vllm pre-allocated its KV at
  startup — it doesn't grow at runtime and squeeze), so it is a
  fixed-budget question, not a wobble question. The door caps its own
  ggml-Vulkan arena (hard fail at the cap instead of eating a neighbor's
  card — a generic feature, part of the upstream PR), and **P1's
  nvidia-smi delta decides**: under ~0.4 GB ⇒ Vulkan co-resident is the
  default (no exclusion set, no eviction); tight ⇒ CPU variant (still
  co-resident) or the exclusion pattern. Leaning Vulkan-co-resident, on
  the measurement.
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
- **D-037-5 (owner, 11-22): the server is built into a fork of vision.cpp
  and submitted upstream as a PR** — merge ⇒ pin upstream (zero code of
  ours; the sdcpp-shaped ideal); no merge ⇒ run the fork pinned at
  `upstream-commit + the server delta`, rebased as upstream moves (the
  llama-cpu pattern, aimed at dissolving). This replaces the external
  C-API wrapper from the original plan — the fork has no version-skew seam.
  The PR stays generic: all five model families, plain JSON, no Kokon
  naming, tests covering large POSTs with `Expect: 100-continue`.
- **D-037-6 (recorded 11-22): GPU = Vulkan, by the project's design** —
  the only GPU it ships (one build, all vendors, parity with CUDA on these
  models, ICD already on the box, a runtime separate from vllm's CUDA
  context). A CUDA build would be engine work — out of scope; if it ever
  matters, that's an upstream conversation.
- **D-037-7 (owner, 11-22): the PR gate.** The server code is built and
  tested before any PR; the PR body is shown to the owner in full and is
  **not opened against `Acly/vision.cpp` without their sign-off** (pushing
  a branch to *our* fork is fine — that is just our repo). Style order:
  (1) the author's explicit contribution guidelines, if any exist;
  (2) their own coding style; (3) llama.cpp server style as the fallback
  reference (ggml lineage).
- **D-037-8 (owner, 11-22): the co-residency ruling.** vllm and Kokon
  inference never run concurrently on this box, and on the size delta
  alone, evicting vllm for a lab click is not worth it — so the target is
  **Vulkan co-resident** (arena-capped, no exclusion set, no eviction),
  **CPU co-resident** the fallback, the eviction pattern a last resort.
  P1's measured nvidia-smi delta is what chooses.
- **D-037-9 (recorded 11-22): the PR is built, tested, pushed, and its body
  is drafted and shown** (`task-037-vision-cpp-pr-body.md`) — opening it
  against `Acly/vision.cpp` waits for the owner's sign-off. Style followed
  their CONTRIBUTING.md (clang-format on the C++ files, tests, plain C++),
  matching their existing patterns; CMake left in the repo's own unformatted
  style (their .clang-format is C++-only and mangles CMake — verified,
  restored).
- **D-037-10 (owner, 11-22): no `Signed-off-by` in the PR body** —
  *"we are one. we are legion."* The body carries no identity trailer; the
  pushed branch's DCO commit trailers are unaffected (they predate the
  ruling and history is not rewritten).
- **D-037-11 (measured 11-22 evening): the co-residency shape on this box.**
  With vllm holding 23878 MiB of the 3090 (268 MiB free, stable), the
  `kokon-vision` door runs **CPU**: all five families fit under the 4 GB
  default budget in RAM (max ~1.3 GB peak), 0.65–4.99 s, zero VRAM, no
  eviction, no window drama — D-037-8's fallback, now the measured default
  rather than the guess. **Vulkan co-residency on this card is out:** the
  uncapped MobileSAM peak measured 266 MiB (already over the 268 free), and
  BiRefNet/Depth graphs are 0.7–1.3 GB even at half precision — the free
  budget is a knife-edge for the smallest family and ~3–5× short for the
  others. If the owner ever trims vllm's KV by a few hundred MiB, Vulkan
  SAM becomes viable (the budget check makes any overflow a clean 503, not
  a starvation of the LLM); the door stays backend-agnostic (`-b cpu|gpu`)
  so that flip is a boot-script word.

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

- **`glslc`** (owner's one-liner: `sudo apt install -y glslc` —
  `libvulkan-dev` is already installed; only the shader compiler is missing)
  for the Vulkan build of the fork, so the budget check and the all-five
  routes can be proven on GPU. Not on the CPU path: P1's CPU proof is
  complete, and the door's default on this box is CPU (D-037-11).
- **P2 (the matrix door) stays gated on the owner's ruling** with the
  complete numbers in hand: CPU door (proven, co-resident, 0.65–4.99 s) vs
  waiting on a Vulkan-capable budget (would need vllm's KV trimmed). Window
  protocol per the standing rules when it goes.
- **The upstream PR stays gated on the owner's sign-off of the revised body**
  (D-037-7/9/10) — the branch is pushed and the command is ready; the PR is
  not on P2's critical path.
