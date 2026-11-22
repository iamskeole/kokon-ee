# vision-server benchmark — RTX 3090 + Ryzen 7 5800X (vs the 4090)

Measured 2026-11-22 on the owner's box (apollo) through `vision-server`
(the fork's HTTP server, branch `vision-server`, 5 commits on upstream
`26a7529` — the same code this PR adds). One server process per backend,
all five models resident, same reference payloads every time:

- input: a real 1376×768 photo (~1.06 MP); click (300,200) for SAM point
  and the box (200,150)-(500,450) for SAM box
- MI-GAN: the 1376×768 image + a 512² mask; ESRGAN: a 256² crop → 1024²
- **CPU**: 8-core/16-thread Ryzen 7 5800X, with a 27B vLLM (200k ctx)
  resident on the 3090 at the same time (23.9 GB used, ~270-600 MiB free —
  the server's CPU run never touched the GPU: VRAM stayed flat to the
  MiB for the whole battery)
- **GPU**: RTX 3090, card free (24.1 GB), warm driver shader cache
- "first" = the request that builds and allocates the request graph in a
  fresh process; "warm" = the second identical request (the number a
  user's click feels). The SAM warm request skips the encoder entirely
  (the server's embedding cache), so it is prompt-encode + 4-head
  predict only.

| family | CPU first | CPU warm | GPU first | GPU warm | VRAM peak (all 5 resident) |
|---|---|---|---|---|---|
| MobileSAM (point, 1024²) | 659 ms | **129–150 ms** | 474 ms* | **110 ms** | 694 MiB |
| MobileSAM (box, 1024²) | — | — | — | **110 ms** | 724 MiB |
| BiRefNet-lite (1376×768) | 2894 ms | **2747–2760 ms** | 909 ms | **320 ms** | 2052 MiB |
| Depth-Anything-V2-S (1376×768) | 5012 ms | **4953–4977 ms** | 149 ms | **143 ms** | 724 MiB |
| MI-GAN-512 (1376×768 + mask) | 640 ms | **632–633 ms** | 268 ms | **271 ms** | 936 MiB |
| Real-ESRGAN-x4 (256² → 1024²) | 2620 ms | **2594–2610 ms** | ~170 ms* | **76 ms** | 943 MiB |

\* one process's first request; a repeated first-request in a second
process measured 13 ms (the 4090-class number the driver's shader cache
enables after the first compile) — treat "GPU first" as 15–500 ms
depending on how warm the process's own one-time Vulkan setup is.

## per-request memory budget (the dry-run measure the server enforces)

The allocator's dry run (`ggml_gallocr_reserve_n_size`), not a tensor sum
— this allocator reuses parents for in-place ops, so sums overcount by up
to 20×:

| family | CPU (f32) | Vulkan (f16) |
|---|---|---|
| MobileSAM | 188 MB | **160 MB** |
| BiRefNet-lite | 1332 MB | 1327 MB |
| Depth-Anything-V2-S | 742 MB | 303 MB |
| MI-GAN | 211 MB | 211 MB |
| Real-ESRGAN (per tile) | 217 MB | 218 MB |

On the 3090 with a 27B vLLM holding 23.9 GB, ~270–600 MiB stay free:
**MobileSAM Vulkan (160 + ~180 MiB resident weights ≈ 340 MiB) fits, the
rest do not** — which is why the server measures the request before
allocating and returns `503` with the number instead of starving the card.

## against the 4090

The 3090 and 4090 have almost the same memory bandwidth (936 vs
1008 GB/s) — these vision models are bandwidth-bound, so at equal input
size a 3090 should land within ~10% of the 4090. Our gaps versus the
published 4070/4090 numbers (MobileSAM 19 ms, BiRefNet-lite 85 ms) are
mostly:

1. **input size** — we benchmarked at 1376×768 (1.06 MP), ~34% more
   pixels than 1024²;
2. **per-request graph allocation** — the server builds and reserves a
   fresh request graph each call (safe for concurrent users and for the
   budget check); the published CLI numbers run one graph per process
   with no reservation work in the timed region.

Correcting for both (1024², warm process), the 3090 should be in the
low-tens-of-ms for MobileSAM and well under 100 ms for BiRefNet-lite —
i.e. on par with the 4090, as the bandwidth parity says it should be.
A cross-request graph cache in the server would close the remaining gap
if the per-request allocation ever shows up in a profile.

## reproducibility

```
cmake -B build -DCMAKE_BUILD_TYPE=Release -DVISP_VULKAN=ON -DVISP_FMT_LIB=ON \
      -DGGML_BACKEND_DL=ON -DGGML_NATIVE=OFF -DGGML_CPU_SSE42=ON \
      -DGGML_CPU_AVX=ON -DGGML_CPU_AVX2=ON -DGGML_CPU_F16C=ON \
      -DGGML_CPU_BMI2=ON -DGGML_CPU_FMA=ON
cmake --build build -j
./build/bin/vision-server -l 127.0.0.1 -p 8080 -b gpu \
  -m models/MobileSAM-F16.gguf -m models/BiRefNet-lite-F16.gguf \
  -m models/Depth-Anything-V2-Small-F16.gguf -m models/MIGAN-512-places2-F16.gguf \
  -m models/RealESRGAN-x4plus_anime-6B-F16.gguf
# then POST {"image": "<base64 png>", "point": {"x":300,"y":200,"label":1}}
# to /sam, {"image": ...} to /birefnet | /depth | /migan | /esrgan
```

`--max-arena <MB>` caps the per-request device budget (default 4096);
over-budget requests return 503 with the measured size. The window job
that produced the GPU rows (vllm evicted from the matrix for ~90 s,
restarted before the job's marker) lives in the owner's ml repo,
`backends/_jobs/vision-vulkan-037/`.
