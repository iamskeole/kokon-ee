// Click-to-select for the lab (the InpaintEditor's Select, LabScene's outlines): MobileSAM
// (Apache-2.0, Acly's ONNX export; on a split box it runs as a stock GGUF through vision.cpp).
// **The model never runs in the browser** (task-037 P3, D-037-13): the service decides where
// it runs — a stock GGUF door behind the box's gateway (the split-box path: the accelerator's
// `vision-server`), or the box's own two pinned ONNX files (the single-box path, the stock
// 0.12 shape) — and the browser just asks and draws. Nothing else: a box with neither (a thin
// client of a down accelerator) is said plainly, because the owner's rule is that this lab's
// model lives on the box, the browser is a thin client, and a failure here is *ours*, not
// something an in-browser copy papers over. (The in-browser onnxruntime-web path is retired
// with this file's old second half; the only download left is a single box's own copy, below.)

import { api } from "./api";
import { downloadIsFor } from "./logic";

/** Everyone waiting on the one download (the single-box path: the service's own ONNX copy,
 * the two pinned ~45 MB files, crates/qls-core/src/matting.rs MOBILE_SAM). */
const progressListeners = new Set<(pct: number) => void>();
let downloading: Promise<void> | null = null;

/** Downloads the box's copy through kokon-service and waits for it. A request the service
 * refuses because an install is already running (the other caller's) still waits for that
 * install: before, the refusal read as "could not be downloaded" while the download went on
 * (Mac, 2026-11-04). */
async function download() {
  try {
    await api.downloadSegment();
  } catch (e) {
    console.warn("segment download not started (another install running?)", e);
  }
  progressListeners.forEach((f) => f(0));
  for (;;) {
    await new Promise((r) => window.setTimeout(r, 700));
    const i = await api.installStatus().catch(() => null);
    if (!i) break;
    const items = i.items.filter((x) => x.id.startsWith("mobile_sam"));
    if (items.some((x) => x.state === "failed" || x.state === "cancelled")) break;
    const size = items.reduce((n, x) => n + (x.size || 0), 0);
    if (size) {
      const pct = Math.floor((100 * items.reduce((n, x) => n + x.received, 0)) / size);
      progressListeners.forEach((f) => f(pct));
    }
    if (!i.running) break;
  }
}

/** Cancel for the selection model's download (only when the running install is that one). The
 * waiting `loadSegmenter` then returns null, as for a failed download. */
export async function cancelSamDownload() {
  if (downloadIsFor(await api.installStatus().catch(() => null), "segment")) await api.cancelInstall();
}

/** The one download, with a progress listener wired in for its duration. */
function awaitDownload(onProgress?: (pct: number) => void) {
  if (onProgress) progressListeners.add(onProgress);
  return (async () => {
    try {
      await (downloading ??= download().finally(() => (downloading = null)));
    } finally {
      if (onProgress) progressListeners.delete(onProgress);
    }
  })();
}

export interface SamResult {
  /** Logits (> 0 = selected) at the size passed to `predict`, one per output. */
  masks: Float32Array[];
  iou: number[];
}

/** The lab's segmenter, wherever the box runs the model (its door, or its own files — the
 * ladder is the service's, crates/qls-core/src/segment.rs). `src`: the service's image
 * ("input:"/"gen:") — the browser sends no image bytes; the box reads its own copy and, for
 * the door, the door's server caches the embedding by the image's bytes (encode is a no-op
 * for it, its first predict pays the one-time encoder run). */
export class ServiceSegmenter {
  /** Where the model runs ("service" — the box, door or in-process). */
  readonly device = "service";

  constructor(private src: string) {}

  /** Encodes once per (image, size); the box keeps the embeddings until its cache turns. */
  async encode(w: number, h: number): Promise<void> {
    await api.segmentEncode(this.src, w, h);
  }

  /** `points` in mask pixels of a `w`×`h` mask (`label` 1 = object, 0 = not); `box` confines
   * the object to a rectangle in the same pixels. The 4 masks come back as 0/255 PNGs at
   * that size (the consumers test > 0, so the bytes are the contract). */
  async predict(
    points: { x: number; y: number; label: 0 | 1 }[],
    w: number,
    h: number,
    box?: { x0: number; y0: number; x1: number; y1: number },
  ): Promise<SamResult> {
    const r = await api.segmentPredict(this.src, w, h, points, box);
    return { masks: await Promise.all(r.masks.map((m) => pngToLogits(m, w, h))), iou: r.iou };
  }
}

/** The 0/255 PNG the service returns for one mask, as the Float32Array the consumers expect.
 * Downsampled from a same-size PNG, so no smoothing is involved. */
async function pngToLogits(b64: string, w: number, h: number): Promise<Float32Array> {
  const im = new Image();
  im.src = `data:image/png;base64,${b64}`;
  await im.decode();
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const ctx = c.getContext("2d")!;
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(im, 0, 0, w, h);
  const d = ctx.getImageData(0, 0, w, h).data;
  const out = new Float32Array(w * h);
  for (let i = 0; i < out.length; i++) out[i] = d[i * 4] > 0 ? 255 : 0;
  return out;
}

/** The one segmenter, or `null` when nothing can run the model (the callers' "missing") —
 * said plainly, never papered over by an in-browser copy (D-037-13).
 *
 * The ladder, mirrored from the service: the box's **door** when it answers (a split box's
 * accelerator runs it); the box's **own files** when it has them (a single box, the stock
 * path — and a mixed box's escape when its door is down); a single box **without the files**
 * downloads them now and uses them in this same session (b25e7af, 11-19) — the only download
 * left in this file. A box with a *named but not answering* door is a thin client of a down
 * accelerator: `null`, plainly (the caller says so). */
export async function loadSegmenter(
  source: string | null,
  onProgress?: (pct: number) => void,
): Promise<ServiceSegmenter | null> {
  if (!source) return null;
  const st = await api.segmentStatus().catch(() => null);
  if (!st) return null; // no service at all: nothing to ask.
  if (st.installed || st.door) return new ServiceSegmenter(source);
  if (st.doorConfigured) return null; // a named door that is not answering: a thin box, plainly.
  // No door at all (a single box): download its files now and use them this session.
  await awaitDownload(onProgress);
  return (await api.segmentStatus().catch(() => null))?.installed ? new ServiceSegmenter(source) : null;
}
