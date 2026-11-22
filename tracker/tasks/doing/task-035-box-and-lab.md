# Task-035 — demo day: the box editor's two UI faults, and the lab's models on the box (task-025's trigger fires)

**Created:** 2026-11-22 — the owner, before demo day: "fix the flux piece on box
creation / edits", plus "labs / layers still whines about not being able to
load models (the SAM probably?) … I thought we had already fixed this so the
models themselves are also ran on the gpu / external engine box instead of
browser / clientside so take a look and make sure its fixed", plus "make the
new-box setup instructions representative of the latest state". **The
upstream/origin sync check the owner asked for first: upstream `master` is
`bff35ca` (0.12.1), an ancestor of our HEAD (the task-030 rebase base) — zero
commits ahead, no rebase needed; `origin/main` == HEAD (verified 2026-11-22
after `git fetch` of both).**

## Scope

1. **The box description input (BoxEditor)** stretched to nearly the whole
   tools row (`flex: 1` → ~1135 px on a 1600 px window, 71 % of the row) —
   ugly and awkward to type in. Cap it to a normal field width, in keeping
   with the rest of the UI.
2. **Dragging a box resizes it instead of moving it.** Reproduced in a
   headless browser (the harness below) before touching anything: a body drag
   moved only the top-left corner — `onMove`'s move branch wrote
   `{ ...s, x1: nx, y1: ny }` and left `x2`/`y2` at the start values, so the
   far corner stayed put and the rect collapsed toward the drag direction
   (keep box 100,150,250,300 dragged +0.15/+0.10 became 250,250,100,200).
   Both create (New tab) and edit (reference tab) modes, as reported.
3. **The Layers lab runs its models on the box** — task-025's parked trigger
   fires ("take a look and make sure its fixed"): MobileSAM (click-to-select)
   currently runs **in the browser** (onnxruntime-web: WebGPU or
   single-threaded WASM); the service only downloads and serves the two ONNX
   files. On a weak client (the 12-inch MacBook) or a client whose browser
   cannot run ONNX (WebGPU refused, WASM OOM — the recorded spike s17) the lab
   "could not be downloaded / failed to start" whines and the feature does
   not work. The box variant: the service runs MobileSAM itself (its existing
   ONNX Runtime CPU runtime — the same one BiRefNet uses; no image change,
   no GPU window) with a small encode/predict API, and the UI's
   `Segmenter.load` prefers it whenever the service has the model files.
   The in-browser path stays the stock-mode default (task-025's pre-logged
   decision, upstream behaviour wins) and remains the fallback when the
   service has no copy. LabScene's `samOutline` keeps the in-browser path
   (its image is a client-derived canvas, not a service source).

   **D-035-1** — the capability selects the mode, no new setting: the UI
   asks the service whether the segment model is installed; if so it uses
   the service's SAM (the box runs it), else the in-browser one (today's
   stock behaviour). This is task-025's "config-selected" decision in its
   simplest honest form — the box's own capability is the config.
   **D-035-2** — CPU first: BiRefNet is on the CPU *by measurement* (s17),
   and the ORT CPU runtime is already in the image; MobileSAM is smaller
   still. The CUDA-EP half of task-025 (a different image, arena caps, a
   GPU window) stays as the follow-up optimisation for task-025, not for
   demo day.

## Reproduction harness (dev-only, deleted before close)

`boxdev.html` + `src/boxdev-harness.tsx`: the BoxEditor with a seeded
composer (one 800×600 SVG-data-URI reference, one box per kind), driven and
screenshot from playwright's chromium (`/tmp/uitest`). The pre-fix run
proves item 2 (a body drag changed only `x1`/`y1`) and item 1 (the input at
71 % of the row); the post-fix run must show the rect translate intact
(corner + size), the handles still resizing, and the input capped.

## Acceptance

- [ ] A body drag of any box kind (keep/move/new/remove, New tab and
      reference tabs) translates the rect without changing its size; the
      four corner handles still resize; drawing a new box is unchanged.
- [ ] The description input has a bounded width that reads like the rest of
      the UI (no near-full-row field); the tools row stays centered and
      wraps on narrow windows.
- [ ] The box's `BoxRow` JSON table (`boxRows`) is byte-compatible: same
      fields, same 0-1000 rounding (the model's contract).
- [ ] `POST /segment/encode` + `POST /segment/predict` run MobileSAM in the
      service (CPU): encoder once per (image, size) with cached embeddings,
      decoder per click/box, masks as compact PNGs; missing model → the
      existing MissingModel error; both files hash-pinned as today.
- [ ] `Segmenter.load` prefers the service when it has the model, else the
      in-browser one (stock behaviour untouched); InpaintEditor's Select and
      the lab's split use it; LabScene stays in-browser (its canvas is
      client-derived).
- [ ] Live on the box after the deploy: lab opens, the encoder runs on the
      box (service log), a click returns a mask through the UI — no
      in-browser ONNX; weak-client path is thin (one encode POST, small
      PNGs). No GPU involvement; vllm untouched (CPU-only ORT, the BiRefNet
      precedent).
- [ ] The fresh-box / prod-box docs carry the current reality: pull the
      repo, `docker compose build` + `up -d`, nginx in front, the model
      downloads (including the lab's segment pair) and verification steps.
- [ ] Suites green: cargo test -p qls-core / npm test / mcp (as far as the
      host allows); the harness screenshots in `done/`'s record.

## blockers

None.

## decisions

- D-035-1, D-035-2 as above.
- Task-025 stays parked for its CUDA-EP/BiRefNet-GPU half; its
  MobileSAM-on-the-box half is executed here (the trigger's "someone with a
  weak device / the owner wants the lab to work" is the owner himself,
  2026-11-22).
