// Click-to-select for the layers lab (components/LayersEditor.tsx): MobileSAM (Apache-2.0, Acly's
// ONNX export, MIT). The **service runs it in-process** (D-035-1, crates/qls-core/src/segment.rs):
// the box downloads the two files the first time Select is used (~45 MB, pinned: crates/qls-core/
// src/matting.rs MOBILE_SAM) and runs the encoder/decoder on its own CPU, so any browser —
// including one that cannot run ONNX — gets the lab as a thin client, and the first use lands
// on the box, not in the browser. The in-browser onnxruntime-web copy (Segmenter below) is the
// last resort: a box with no reachable service, or whose download failed, still gets Select, as
// in stock 0.12. With no service copy at all, the vite dev server's `lab-models/mobilesam/`
// (gitignored).
//
// The encoder runs once per image (longest side 1024, 0-255 RGB, HWC); the decoder runs per click
// on the cached embeddings and returns SAM's 4 masks at the size asked for: index 0 = the single-mask
// output, 1-3 = sub-part / part / whole (their order and the predicted IoU are unreliable, S1).

import type * as Ort from "onnxruntime-web";
import { api, urls } from "./api";
import { downloadIsFor } from "./logic";

const DEV_DIR = "/lab-models/mobilesam/";
const ENCODER = "mobile_sam_image_encoder.onnx";
const DECODER = "sam_mask_decoder_multi.onnx";

/** Everyone waiting for the one download (the lab's split and scene both load the model at once). */
const progressListeners = new Set<(pct: number) => void>();
let downloading: Promise<void> | null = null;

/** Downloads the files through kokon-service and waits for it. A request the service refuses
 * because an install is already running (the other caller's) still waits for that install: before,
 * the refusal read as "could not be downloaded" while the download went on (Mac, 2026-11-04). */
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
 * waiting `load` then returns null, as for a failed download. */
export async function cancelSamDownload() {
  if (downloadIsFor(await api.installStatus().catch(() => null), "segment")) await api.cancelInstall();
}

/** The one download, with a progress listener wired in for its duration (the in-browser path
 * and the service path share the same in-flight install). */
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

/** Where the model files are: kokon-service's copy (downloaded now if it isn't there, `onProgress`
 * 0-100), else the dev server's. Null when neither has them. */
async function modelUrls(onProgress?: (pct: number) => void): Promise<((file: string) => string) | null> {
  const st = await api.segmentStatus().catch(() => null);
  if (st && !st.installed) await awaitDownload(onProgress);
  if ((await api.segmentStatus().catch(() => null))?.installed) return urls.segmentModel;
  const head = await fetch(DEV_DIR + DECODER, { method: "HEAD" }).catch(() => null);
  if (head?.ok && !head.headers.get("content-type")?.includes("text/html")) return (f) => DEV_DIR + f;
  return null;
}
const SIDE = 1024;

let ort: Promise<typeof Ort> | null = null;
const loadOrt = () => (ort ??= import("onnxruntime-web"));

export interface SamResult {
  /** Logits (> 0 = selected) at the size passed to `predict`, one per output. */
  masks: Float32Array[];
  iou: number[];
}

/** The lab's click-to-select model run **in the service** (D-035-1): the box owns MobileSAM
 * (the same pinned files this one would download) and runs it on its CPU, so a thin browser
 * that cannot run ONNX gets the lab too — the 11-11 "could not be downloaded" whine's other
 * support. `src`: the service's image ("input:"/"gen:"). The wire contract mirrors `Segmenter`:
 * encode once per (image, size) (embeddings cached in the service), then per click the decoder;
 * masks come back as 0/255 PNGs at the size asked (the consumers test >0, so the same bytes). */
export class ServiceSegmenter {
  /** Where the model runs (Segmenter.device is "webgpu"/"wasm"). */
  readonly device = "service";

  constructor(private src: string) {}

  /** `img` is not needed: the service reads its own copy of the image. */
  async encode(_img: CanvasImageSource, w: number, h: number): Promise<void> {
    await api.segmentEncode(this.src, w, h);
  }

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

/** The 0/255 PNG the service returns for one mask, as the Float32Array the in-browser path
 * produces. Downsampled from a same-size PNG, so no smoothing is involved. */
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

/** The lab's segmenter: the service's model when the service has the files (the box runs it —
 * any browser is a thin client, D-035-1). A box that has the service but not yet the files
 * downloads them now (the pinned ~45 MB, `onProgress`) and runs them in-process **in this same
 * session** — first use lands on the box. The in-browser copy (the stock default) is the last
 * resort: no reachable service, or the box's download failed. Null when nothing can run it (the
 * callers' "missing"). */
export async function loadSegmenter(
  source: string | null,
  onProgress?: (pct: number) => void,
): Promise<Segmenter | ServiceSegmenter | null> {
  if (source) {
    const st = await api.segmentStatus().catch(() => null);
    if (st?.installed) return new ServiceSegmenter(source);
    if (st) {
      await awaitDownload(onProgress);
      if ((await api.segmentStatus().catch(() => null))?.installed) return new ServiceSegmenter(source);
    }
  }
  return Segmenter.load(true, onProgress);
}

export class Segmenter {
  private emb: Ort.Tensor | null = null;
  private scale = 1;

  private constructor(
    private o: typeof Ort,
    private enc: Ort.InferenceSession,
    private dec: Ort.InferenceSession,
    /** The execution provider the encoder runs on. */
    readonly device: string,
  ) {}

  /** Null when the model files could not be had (no service copy, the download failed). */
  static async load(webgpu = true, onProgress?: (pct: number) => void): Promise<Segmenter | null> {
    const at = await modelUrls(onProgress);
    if (!at) return null;
    const o = await loadOrt();
    o.env.logLevel = "error";
    // WebGPU when the webview has it, else WASM (one thread: the page is not cross-origin isolated).
    let enc: Ort.InferenceSession | null = null;
    let device = "wasm";
    if (webgpu && "gpu" in navigator) {
      try {
        enc = await o.InferenceSession.create(at(ENCODER), { executionProviders: ["webgpu"] });
        device = "webgpu";
      } catch {
        enc = null;
      }
    }
    enc ??= await o.InferenceSession.create(at(ENCODER), { executionProviders: ["wasm"] });
    const dec = await o.InferenceSession.create(at(DECODER), { executionProviders: ["wasm"] });
    return new Segmenter(o, enc, dec, device);
  }

  /** Encodes `img` (any size); clicks are then in the coordinates of a `w`×`h` mask of it. */
  async encode(img: CanvasImageSource, w: number, h: number) {
    this.scale = SIDE / Math.max(w, h);
    const rw = Math.max(1, Math.round(w * this.scale));
    const rh = Math.max(1, Math.round(h * this.scale));
    const c = document.createElement("canvas");
    c.width = rw;
    c.height = rh;
    const ctx = c.getContext("2d")!;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(img, 0, 0, rw, rh);
    const px = ctx.getImageData(0, 0, rw, rh).data;
    const x = new Float32Array(rw * rh * 3);
    for (let i = 0, j = 0; i < px.length; i += 4, j += 3) {
      x[j] = px[i];
      x[j + 1] = px[i + 1];
      x[j + 2] = px[i + 2];
    }
    const out = await this.enc.run({ input_image: new this.o.Tensor("float32", x, [rh, rw, 3]) });
    this.emb = out.image_embeddings;
  }

  /** `points` in mask px (`w`×`h`, the size passed to `encode`); label 1 = object, 0 = not.
   * `box`: the object is inside it (corners as labels 2 and 3, in place of the padding point). */
  async predict(points: { x: number; y: number; label: 0 | 1 }[], w: number, h: number, box?: { x0: number; y0: number; x1: number; y1: number }): Promise<SamResult> {
    if (!this.emb) throw new Error("encode first");
    const all: { x: number; y: number; label: number }[] = [...points];
    if (box) all.push({ x: box.x0, y: box.y0, label: 2 }, { x: box.x1, y: box.y1, label: 3 });
    const n = all.length + (box ? 0 : 1);
    const coords = new Float32Array(n * 2);
    const labels = new Float32Array(n);
    all.forEach((p, i) => {
      coords[i * 2] = p.x * this.scale;
      coords[i * 2 + 1] = p.y * this.scale;
      labels[i] = p.label;
    });
    if (!box) labels[n - 1] = -1; // padding point: no box
    const T = this.o.Tensor;
    const out = await this.dec.run({
      image_embeddings: this.emb,
      point_coords: new T("float32", coords, [1, n, 2]),
      point_labels: new T("float32", labels, [1, n]),
      mask_input: new T("float32", new Float32Array(256 * 256), [1, 1, 256, 256]),
      has_mask_input: new T("float32", new Float32Array([0]), [1]),
      orig_im_size: new T("float32", new Float32Array([h, w]), [2]),
    });
    const m = out.masks;
    const [, k, mh, mw] = m.dims as number[];
    if (mw !== w || mh !== h) throw new Error(`mask size ${mw}x${mh}, expected ${w}x${h}`);
    const data = m.data as Float32Array;
    const masks = Array.from({ length: k }, (_, i) => data.subarray(i * w * h, (i + 1) * w * h));
    return { masks, iou: Array.from(out.iou_predictions.data as Float32Array) };
  }
}
