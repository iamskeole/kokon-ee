# task-037 — the upstream PR body (D-037-7: shown to the owner 2026-11-22, NOT submitted)

Source: `iamskeole/vision.cpp` branch `vision-server` (3 commits on
upstream main `26a7529`), pushed. PR target: `Acly/vision.cpp`.

**Title:** `Add vision-server: an HTTP server for the GGUF model families`

---

## body

`vision-server` is a small HTTP/1.1 server in `src/cli/` that serves the
project's GGUF model families over a minimal JSON API. Today the models are
only reachable from the command line (`vision-cli`); this adds a small door
for services that want to run them, with no new dependencies: POSIX sockets,
the project's `util/string.h` (gained base64), and a new self-contained
`util/json.h`.

**API** — images are base64 PNG or JPEG (`data:` URLs accepted):

| Route | Request | Response |
|---|---|---|
| `GET /health` | — | server, backend and model status |
| `POST /sam` | `{"image", "point": {"x","y"} \| "box": {"x1","y1","x2","y2"}}` | `{"width","height","iou":[4],"masks":[4 base64 PNGs]}` — all four heads, index-stable |
| `POST /birefnet` | `{"image"}` | `{"width","height","image"}` — base64 PNG alpha matte |
| `POST /depth` | `{"image"}` | `{"width","height","image"}` — base64 PNG depth map |
| `POST /migan` | `{"image","mask"}` | `{"width","height","image"}` — inpainted PNG |
| `POST /esrgan` | `{"image","tile"?}` | `{"width","height","image"}` — upscaled PNG |

**Design notes**

- Large POSTs use `Expect: 100-continue`; the server answers `100 Continue`
  before reading the body (covered by a test). Header limit 64 KB, body
  limit 64 MB, both answered 4xx.
- One in-flight inference at a time (a per-server mutex). This is a door for
  a single local client; a request queue is a reasonable follow-up.
- SAM keeps a per-image encoder-output cache (hashed on the preprocessed
  pixels), so a stream of clicks/boxes on the same image skips TinyViT after
  the first.
- GPU co-residency: on a Vulkan backend `--max-arena <MB>` (default 4096) is
  **probed at boot** — the server allocates and frees that size and fails
  with an actionable message if the device cannot provide it, so it fails
  loudly instead of starving other tenants of the card.
- POSIX only for now (Linux/macOS; UNIX guard in CMake). The code uses only
  POSIX socket APIs, so a Windows port is mechanical.

**Tests**

- `tests/test-json.cpp` (19): parse/write roundtrips, strict rejections
  (trailing data, leading zeros, depth limit, bad escapes, unpaired
  surrogates), base64 encode/decode.
- `tests/test-server.cpp` (9): forks the real server on a random port, polls
  `/health` for readiness, then exercises `/health`, `/sam` point and box on
  a real image, inverted-box → 400, a real `Expect: 100-continue` roundtrip,
  bad JSON → 400, bad image → 400, unknown route → 404, unloaded model → 404.

Verified on Linux x86_64 (Ryzen 5800X, `GGML_NATIVE=OFF` SSE4.2/AVX2/F16C,
`VISP_FMT_LIB=ON`, `GGML_BACKEND_DL=ON`): `ctest -C Release` — all C++ tests
pass, including an AddressSanitizer build of the server under a sustained
point/box mix. The pre-existing
`tests/test_birefnet.py::test_relative_position_index` Python failure is
unrelated (it fails on `main` on the same machine).

**One caveat for consumers**: F16 GGUF outputs are not byte-identical to an
FP32/ONNX reference of the same model; compare by selection (IoU / which
head), not by pixels.

Happy to first discuss the shape of this in an issue if you prefer — the
branch is ready either way.

Signed-off-by: p <iamskeole@users.noreply.github.com>

---

## submission state

- [x] Built and tested (above) — before any submission, per D-037-7
- [x] Followed the author's CONTRIBUTING.md: `clang-format` on all modified
      C++ files (their LLVM-based .clang-format), tests added for the new
      behaviour, plain structs/functions, `snake_case`, no new dependencies,
      C++20, comments explain *why*
- [x] Pushed to `iamskeole/vision.cpp:vision-server` (our fork — allowed)
- [ ] **Owner sign-off of this body** — required before `gh pr create`
      against `Acly/vision.cpp`
- [ ] Open the PR (command ready: `gh pr create --repo Acly/vision.cpp
      --head iamskeole:vision-server --base main --title "..." --body @file`)
