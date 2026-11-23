# task-037 — the lab's models as stock GGUF doors (vision.cpp)

## status: **DONE 2026-11-23 (late night)** — GO received 2026-11-22; P1 11-22 night, PR submitted 11-23, P2 door live 11-23, P3 + P4 11-23 night. **P1 complete 2026-11-22 (night):**
fork at `backends/kokon-vision/` (**5 commits** on upstream `26a7529`,
branch `vision-server` pushed to the fork) — `vision-server` (all five
families, 4 SAM heads, embed cache, 100-continue) + `util/json.h` +
base64 in `util/string.h` + the per-request device-memory budget check
(dry-run of the graph allocator, 503 with the measured size before any
allocation) + test-json (19) and test-server (10, functional, incl. the
budget refusal). All C++ ctest green; ASAN clean under mixed-request
hammers. **All five families verified end-to-end on CPU (with vllm
resident) and on GPU (3090, card free, via the window protocol)** —
numbers and the 3090-vs-4090 analysis in
[`task-037-vision-cpp-bench.md`](task-037-vision-cpp-bench.md) (the doc to
forward). **Two real bugs found and fixed in the server along the way**
(both recorded below with measurements): the embedding-cache corruption
(commit `73a7699` — a cache hit predicted from stale intermediate data;
now bit-identical across repeated requests on CPU and Vulkan) and the
budget overcount (fixed by dry-running the allocator). Model artifacts
kept per the owner's word: `models/gguf/Acly/<5>/` on the ml box with
provenance (ml `582bcd0`). **PR body revised twice and re-shown**
(`task-037-vision-cpp-pr-body.md`); **not opened without the owner's
sign-off (D-037-7/9).** **PR submitted 11-23: [Acly/vision.cpp#25](https://github.com/Acly/vision.cpp/pull/25)** (owner sign-off;
+2202/−0, 8 files, purely additive) — awaiting the author's
review; the fork stays pinned either way (D-037-5's fallback).
**P2 COMPLETE 11-23 (owner: "agree on CPU for now"): the
`kokon-vision` door is LIVE in the matrix** — CPU, both lab models
(MobileSAM + BiRefNet-lite), permanent co-tenant of every set (`& kv`,
the sidekick pattern), `ttl 86400`, `/health` readiness, one-word
`-b gpu` flip documented in the boot script. Proven through the
gateway in a 65-second window (the change window: no restore on
success): cold-door first click 0.91 s, warm 0.29 s, IoUs bit-identical
to the CPU reference (`[0.619, 0.8813, 0.8677, 0.7937]`); BiRefNet matte
2.75 s, bimodal; vllm revived 58 s later and a co-resident click cost
**0 MiB of VRAM** (608 free before and after) — zero-VRAM co-tenancy
proven against the live assistant runtime. Owner exposure 62 s.
(The 21:24 window's 404s — post-mortem below; that lesson shaped
this window: functional router waits, rename restore, healthy-runtime
no-op fail.) **P3 DONE 11-23 (night)** — the service's ladder (door →
in-process → plain error) for both models, `visionEngine` in schema 16,
the browser a thin client (onnxruntime-web gone from the app);
deployed (final image `multiuser-6e5a0cf`) and proven as the full
2×2 ladder matrix on the live box (split-box half included — the
whole lab runs over the wire with zero local model bytes; details in
the P3 item below), **P4 (docs/handover) done the same night — the
task's work is complete.** The only open thread is the PR's fate
(D-037-5: pin upstream if it merges, else the fork stays pinned) —
a watch item, not work.

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

**Measured 2026-11-22 night (the Vulkan window + timing battery — the
owner's "test what we claim" ask; `glslc` was installed by the owner,
`libvulkan-dev` was already there). Four window runs of
`backends/_jobs/vision-vulkan-037/` (config swap out of the matrix → card
free → battery → rename-restore → daemon reboots vllm → 1-token probe
200 → marker; vllm never running mid-window, revived before every marker):

- **All five families on the 3090 (card free, 24.1 GB, warm shader cache):**
  first requests 13–909 ms, **warm 76–320 ms** (SAM point/box 110 ms,
  BiRefNet 320, Depth 143, MI-GAN 271, ESRGAN 76); peaks 694–2052 MiB with
  all five resident. IoUs: point [0.6271, 0.8843, 0.8670, 0.7910], box
  [0.5830, 0.8646, 0.8105, 0.7231] — the box row matches the CPU box row
  [0.5804, 0.8644, 0.8099, 0.7227] to <0.004 per head, and the point row
  matches CPU (0.6184/0.8812/0.8677/0.7940) within 0.009: **Vulkan and CPU
  are the same model, per D-037-4.**
- **Per-request budget table, Vulkan (the dry-run measure the server
  enforces with `--max-arena`):** SAM **160 MB**, BiRefNet 1327, Depth
  303, MI-GAN 211, ESRGAN 218 (CPU: 188 / 1332 / 742 / 211 / 217). The
  two-graph SAM split (below) cut this from 200 → 160 and the all-five
  peak from 883 → 694 MiB.
- **CPU battery, vllm co-resident (the production scenario, zero GPU
touch — VRAM flat to the MiB):** warm 129–2760 ms as in the bench doc.

**The embedding-cache corruption (the significant find; fixed in commit
`73a7699`).** While measuring the warm (cache-hit) requests, the SAM IoUs
drifted run to run — first request of a fresh server correct
(0.6190/0.8813/…), every later request on the same image different
(0.398/0.512/…, 0.719/0.781/…, never converging). Root cause, proven with
a 6-request probe: the server captured the encoder's output tensor *after*
the combined encode+predict graph ran, and this ggml's allocator may reuse
a parent's buffer for a later **in-place** op — so the "cached embedding"
was sometimes the tail of the prediction's intermediate data, which the
next cache-hit request then predicted from (each run's intermediates
feeding the next run's "embedding" — the drift never settled). Fix:
phase 1 runs the encoder in **its own graph** (its output registered via
`compute_graph_output`, which also registers the chain and image input in
the graph so the allocator gives them buffers — the missing registration
surfaced as a null-buffer assert on the first attempt); phase 2 runs the
predictor with the embedding always re-uploaded from the host copy.
Result: 6/6 point and 3/3 box requests bit-identical on CPU *and*
Vulkan; ASAN hammer clean; full ctest green. Lesson recorded: in this
ggml, never read an intermediate node after the graph that contains it
ran — only terminal outputs are stable.

**The 21:24 owner-facing 404s (post-mortem, corrected against the daemon's
own journal, unit `ml-llama-swap`).** The 404s the owner's session saw at
21:24:2x–21:24:4x ("no router for requested model") were the **intended
window exposure**: run 2's battery kept vllm out of the registry for its
whole ~4.5-min duration (21:20:xx swap → 21:24:29 restore-reload), and the
owner happened to send a message near the tail of it. Their 21:26:24 ping
reached me (pi, 127.0.0.1) and triggered the daemon's on-demand boot:
vllm up 21:27:22, healthy 21:27:27, the owner's OWUI refreshed 200 at
21:27:28 — impact ≈ 3 min, then clean; no further owner-facing 404s all
night (the only 404s in the last 30 min of journal are my own
mis-named probes). **My fault extended the churn after the owner had
already recovered:** run 2's reload-wait (journal line offsets) never saw
the 21:24:29 reload that did happen — the proven 036 template reads the
`ml-llama-swap` unit and its offset method has never failed across many
windows, under vllm's log volume included, so the 037 original's wait was
not that form (it watched something without the reload line; the daemon's
unit is `ml-llama-swap` — there is no `llama-swap` user unit). The 3-min
timeout fired at 21:29:27 as a false FAIL; the fail path's re-cp (21:32:22,
content-identical bytes) made the daemon reload again and **restart the
healthy vllm instance** (3208917 → 3214207, healthy 21:33:24) — four more
minutes of unnecessary churn, though the registry never lacked vllm in
that stretch, so it stayed invisible to the owner. Fixes, proven in the
two subsequent windows (runs 3–4, 21:50–21:52 and 22:07–22:09, ~92 s
each, every rename picked up in 3 s, vllm revived and 1-token-verified
before the DONE marker, the current instance 3246006 has been serving
since 22:09 without a single error): (1) the reload wait is
**functional** — it polls the gateway's `/v1/models` (the router truth the
owner's clients see), never journal parsing; (2) restores are **renames**
(`cp` to temp + `mv`) — every rename all night was picked up in 3–12 s,
and the in-place-cp delay that bit run 2 (2 s, just past a fast poll) can
no longer matter because nothing waits on file-watch events anymore;
(3) markers are wiped at job start (a stale FAIL from run 1 had blinded
run 2's watcher); (4) `fail()` is a no-op when the runtime is already
healthy (the 21:32 churn class). Standing rule recorded: a full-battery
window exposes vllm for the whole battery (minutes, not seconds) —
short windows for quick proofs, long batteries only when the owner is
off chat; everything after the restore is verified before the marker.

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
  **Refined by the night windows (D-037-12):** after the two-graph split,
  MobileSAM Vulkan's per-request budget is **160 MB** (peak 694 MiB with
  all five resident); with vllm's post-boot free memory measured at
  608 MiB, **Vulkan MobileSAM now fits on this card — BiRefNet/Depth/
  MI-GAN/ESRGAN do not** (211–1327 MB per request). Standing door variant:
  **CPU co-resident** (zero VRAM, never touches vllm's headroom); Vulkan
  SAM is a measured one-word flip (`-b gpu`) the moment the owner wants
  the 110 ms click, with the budget check making any regression a clean
  503. P2's ruling is therefore: CPU door now (proven end-to-end both
  ways), or CPU door + the knowledge that a 3090 with a trimmed KV also
  runs Vulkan SAM.
- **D-037-12 (RESOLVED 11-23, owner): P2 = CPU door, both lab models**
  ("agree on CPU for now") — `kokon-vision`, CPU, MobileSAM +
  BiRefNet-lite, permanent co-tenant of every matrix set (`& kv`, the
  sidekick pattern): zero VRAM, never evicts, never evicted, ~0.5 s cold
  boot, 334 MB idle, 163 ms warm click / 2.75 s matte measured with vllm
  co-resident. The Vulkan-SAM variant stays a documented one-word flip in
  the boot script (`-b gpu`) for a day when vllm's KV is trimmed — not
  wired tonight.
- **D-037-13 (owner, 11-23, before sleep): P3 has NO in-browser
  fallback.** The point of this entire workstream is that the gateway /
  service handles the lab's models and the UI is a thin client; any
  failure means *we* failed at our job — so the UI reports it plainly
  instead of silently degrading to onnxruntime-web. P3 retires the
  in-browser path from the capability ladder (door → in-process →
  hard, honest error), including removing the fallback wiring that
  task-035 kept (the in-browser stock default is gone; the browser
  canvas/outline interaction stays — only the *model* must come from
  the service).

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

**P2 — the matrix door (DONE 11-23).** One entry `kokon-vision`
(both models, one boot script, `checkEndpoint /health`, `ttl 86400`
resident, `& kv` in all seven sets — sidekick pattern). The 037p2 change
window ran the standing protocol (wipe markers → stage → `-validate`
→ rename swap → functional router waits → last-good re-seed with the NEW
config → gateway proofs → **vllm revived before the marker**): 65 s
total, no restore needed (the change is the point). Measured: cold
first click 0.91 s / warm 0.29 s, matte 2.75 s, co-resident click
0 MiB VRAM cost, door RSS 134 MB, vllm 23.5 GB untouched.

**P3 — service integration.** A `visionEngine` config section (gateway base
+ key + door id — the D-036-10 shape: one base, one key, door id in the
path, sibling of the sdcpp doors). **Revive the relay** that left the tree
this afternoon — it is the right client for this door (the contract it
speaks, 4-mask PNGs + IoUs, is exactly what a vision.cpp door returns; the
deleted code is the starting point, git history has it). Capability, in
order: the door (through the gateway) → in-process ONNX (single box).
**There is no in-browser fallback (D-037-13)** — if neither is available
the `/segment` capability says so and the UI shows a plain failure: the
gateway/service owns the lab's inference, the UI is a thin client.

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
- **D-037-21 (P3): the ladder is door-first.** A configured door that
  answers takes priority over local files (on the split box the door is
  the intended path with zero local bytes; on the mixed box the door is
  the box's lab engine; the in-process copy is the escape when the door
  is down, and the single-box default when no door is named).
- **D-037-22 (P3): statuses say which door and whether it answers.**
  `/segment` and `/matte` report `doorConfigured` (named in config) and
  `door` (answered `/health`) in addition to `installed`/`bytes`, so the
  browser can distinguish "no door named" (single box — download its own
  copy, run in-process) from "door named but down" (thin box — say so
  plainly, never download, never run in-browser).
- **D-037-23 (P3): the door's `encode` is a no-op.** The door's server
  runs the encoder itself and caches embeddings per image bytes, so the
  client sends no embeddings; the first `predict` pays the one-time
  encoder run, later clicks on the same image hit the cache. In-process
  `Sam::encode` keeps its own LRU (8) as before.
- **D-037-24 (P3): the door gets the image at `w`×`h`.** The client
  resizes with the Triangle filter (the in-process encoder's exact
  resize) and base64-PNGs it; the door does its own 1024-side resize
  internally. Masks come back at the size sent, so they are the exact
  `w`×`h` the in-process path would have produced.
- **D-037-25 (P3): a box is two clamped corners.** `{x1,y1,x2,y2}` into
  the door, clamped to `[0, n-1]`, minimum 1 px per side (a zero-area
  ask grows by one pixel) — the door rejects degenerate boxes, and a box
  is the stronger prompt, so a box-plus-clicks ask sends the box alone.
- **D-037-26 (P3): one point = the last positive click.** The door's
  contract has no negative-click concept; the lab sends the last
  positive-labeled click (the object) or the first of any. The lab's UI
  never sends a bare negative ask, so nothing is lost.
- **D-037-27 (P3): a door matte cannot be cancelled.** `/birefnet` has
  no run id; a Stop mid-flight lets it finish (~3 s on CPU) rather than
  kill the door's request.
- **D-037-28 (P3): door failures are plain and loud.** User-facing: "The
  selection engine is not answering (the accelerator's door is down)." /
  "The selection engine failed: 503 …"; raw bodies and contract
  violations (wrong mask count, non-JSON) ride in the error's `detail`
  (the log), `server_unavailable` for down/unreachable, `internal` for
  contract violations, `missing_model` for the single-box not-downloaded
  state. No retry, no silent fallback.
- **D-037-29 (P3): `/segment/models/{file}` is gone.** The model bytes
  never cross to a browser anymore; the route (and its allowlist
  entries) are removed with the in-browser runtime.

## acceptance criteria

- [x] P1: the door boots from a script on `${PORT}`, answers `/health`, and
      runs both GGUFs (point, multi-point, box, matte) on apollo with vllm
      resident and revived; nvidia-smi delta recorded. **Done 11-22 night**
      (boot 0.52 s; 0 MiB VRAM delta, vllm untouched; bench file has the
      full matrix).
- [x] P1: per-head parity vs the in-process reference within the D-037-4 bar
      on the standing test set. **Done 11-22 night** (server's lantern IoUs
      bit-identical to the stock CLI v0.3.0; ≤ 0.057 from the in-process
      reference per head, same head picked — D-037-4).
- [x] P2: `kokon-vision` in the live matrix (window protocol, vllm revived
      before the marker); the matrix's last-good re-seeded. **Done 11-23**
      (65 s window; ml `de12cff`).
- [x] P3: a service with no local ONNX files but a configured door runs
      the whole lab over the wire (the split-box proof, again, with the
      stock engine); in-process unchanged and tested (the in-browser path
      is **retired** per D-037-13 — no fallback below the ladder). **Done
      11-23 night** (code `77544ab`→`51df995`: schema 16 `visionEngine`,
      the `Door` client — 10 tests on the revived fake-door fixture — the
      door → in-process → plain-error ladder for both models, statuses
      reporting `door`/`doorConfigured`, `/segment/models/*` out, the
      browser made a thin client — onnxruntime-web, its wasm hook, the
      dev-server copy, the download-fallback all gone from the app; a
      door-down thin box says so plainly, a single box still downloads
      its own copy in the same session). **Deployed as image
      `multiuser-51df995` and proven as the full 2×2 ladder matrix** (all
      through the live gateway, vllm resident throughout, zero VRAM
      delta): door-up+files → the door (the daemon's journal:
      `POST /sam → 200 (168 ms)` server-side; point 0.50 s cold /
      0.26 s warm, box 0.25 s, matte 3.0 s); **door-up + no local files
      (the split box)** → point 0.28 s with bit-identical IoUs, matte
      2.78 s — the whole lab runs over the wire with zero local model
      bytes; door-down+files → the in-process escape (point 0.78 s
      first / 0.14 s warm; matte 9.3 s first, the 224 MB ORT load);
      door-down + no files → the plain error ("The selection engine is
      not answering (the accelerator's door is down)." —
      `server_unavailable`, no download, no fallback, D-037-13). Parity
      re-confirmed on the deployed box: door IoUs [0.8161, 0.5809, 0.979,
      0.8251] vs in-process [0.8284, 0.574, 0.9705, 0.8331] — ≤ 0.015 per
      head, both picking head 2; the box prompt is all-high
      ([0.9827, 0.9438, 0.9932, 0.9898]). Matte determinism noted: two
      door runs differ on 944 / 1,056,768 px (0.089 %, all within one
      32-value bucket — F16 multithread-CPU jitter, not the bimodal
      flip); the in-process ORT matte is a valid same-scene matte at a
      different edge threshold (60 KB vs 82 KB). The deployed 782 KB
      bundle has **zero** onnxruntime references; the LAN listener's
      allowlist still carries the lab routes.
- [x] P4: docs current; this file closed with the measured numbers.
      **Done 11-23 (late)**: `docs/fresh-box.md` (the `kokon-vision` fork
      as the one non-pure checkout + the vision-server build/door section,
      the two Acly GGUFs in the models table, the door row in the
      port-allocation table with the vLLM pair's 9108/9109 → 9109/9110
      shift, `kv` in the matrix block — member of every set, the sidekick
      pattern —, `visionEngine` in the config section as schema 16, the
      lab paragraph rewritten as the ladder with the in-browser rung
      named as retired, and a lab item in the verification checklist);
      `docs/service-on-prod-box.md` (the config seed carries `visionEngine`
      and the migration copy list; checklist item 4 is the ladder with the
      door as the standing path on the thin prod box). Two P3 omissions
      found and fixed while closing (committed as addenda, the image
      rebuilt as `multiuser-6e5a0cf` and redeployed): `51df995` removed
      onnxruntime-web from `package.json` but not the lockfile (the image
      still installed it — now gone, zero onnxruntime in the image's
      node_modules), and `09989fe` called `matting::birefnet_installed`
      without committing the function (the committed tree didn't compile;
      the working tree — what all the proofs ran on — had it). Final state
      on the dev box: image `multiuser-6e5a0cf`, both lab statuses
      `{door:true, doorConfigured:true, installed:true}`, a click 0.26 s
      (door) with bit-identical IoUs, vllm healthy, local files present as
      the door-down escape.

## blockers

- ~~`glslc`~~ **resolved 11-22 night** (owner installed it); the Vulkan
  build, the all-five GPU E2E and the budget table are done — Vulkan is
  proven, not pending.
- ~~**P2 (the matrix door)**~~ **done 11-23** (owner's CPU ruling; the
  037p2 window, 65 s, all proofs in the status above). The `-b gpu`
  variant stays a documented one-word flip in the boot script for a
  future trimmed-KV vllm.
- **Upstream PR opened: [Acly/vision.cpp#25](https://github.com/Acly/vision.cpp/pull/25)**
  (2026-11-23, after the owner’s sign-off — D-037-7 satisfied; +2202/−0,
  8 files, purely additive). Awaiting the author’s review; the fork
  (`iamskeole/vision.cpp:vision-server`) stays pushed either way — if it
  merges the door pins upstream `Acly/vision.cpp`, if not we pin the fork
  at `26a7529` + our 5 commits (D-037-5’s fallback). Not on P2’s
  critical path.
- ~~**P2 co-residency question**~~ **closed by the owner's ruling 11-23**
  (D-037-12): vllm is the only *resident* big GPU tenant (the assistant
  runtime, always loaded) — evicting it for a lab click is what D-037-8
  ruled out; every other matrix model is on-demand/evictable by the same
  set mechanics a qwen job already uses, so it is not a co-residency
  problem, just set membership. On GPU, MobileSAM is the only family
  that even *fits* beside vllm (160 MB request + ~180 MB weights vs
  300–608 MiB free, knife-edge); a vllm-co-resident GPU door would
  therefore carry MobileSAM only, and the lab's second model (BiRefNet,
  1.3 GB) could not ride it — **ruling: “agree on CPU for now.”** The
  standing door is CPU (both models, zero VRAM, ~334 MB idle, 0.5 s
  cold); the `-b gpu` flip stays a documented one-word change in the
  boot script for a day vllm's KV is trimmed by a few hundred MiB (then
  a MobileSAM-only GPU door is viable, BiRefNet still CPU).
- ~~**Remaining**~~ **the task is complete (P1–P4 all done, 11-23
  late night).** The only open thread is the PR's fate (D-037-5): if
  [Acly/vision.cpp#25](https://github.com/Acly/vision.cpp/pull/25)
  merges, re-pin the boot script to upstream; if it doesn't, the fork
  (`26a7529` + 5 commits) stays the pin. Either way nothing here
  changes — the contract is identical.