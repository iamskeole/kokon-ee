//! The Layers lab's click-to-select model (MobileSAM, Apache-2.0, Acly's ONNX export) running
//! **in the service**, so any browser can use the lab against the box — including a thin one
//! that cannot run ONNX at all (the webview variant is `src/lib/sam.ts`; this mirrors its
//! contract, and `src/lib/sam.ts` prefers this one whenever the service has the files, D-035-1).
//!
//! The encoder takes the image at 1024 on its long side, 0-255 RGB, HWC — the export normalises
//! inside the graph, as the webview variant sends it — and returns `image_embeddings`; it runs
//! once per (image, size) and its output is cached (the lab encodes once per photo, per click
//! only the decoder runs, on the CPU — the runtime BiRefNet already uses). The decoder takes
//! the embeddings plus the clicks (mask pixels × 1024/max(mask side); label 0 = not, 1 = object,
//! a box is two corners as 2 and 3, else a -1 padding point) and `orig_im_size`, and returns
//! the 4 masks at that size (index 0 = the single-mask output; 1-3 are sub-part / part / whole,
//! their order and the predicted IoU are unreliable, S1). Masks come back thresholded to 0/255:
//! the browser's `fromLogits` only tests >0, so the bytes are the same contract.
//!
//! The files are the same two the browser version downloads and runs (`matting.rs` MOBILE_SAM,
//! hash-pinned): the service never needs a second copy.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine as _;

use image::{GrayImage, RgbaImage, imageops::FilterType};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::matting;
use ort::session::{RunOptions, Session};
use ort::value::Tensor;

/// The model's input side (the export is fixed at 1024).
const SIDE: u32 = 1024;
/// Cached embeddings: each is ~0.7 MB at 1024 (2809 × 64 f32), and the lab encodes one per
/// (photo, size) it is used on.
const EMB_CACHE: usize = 8;

#[derive(Clone)]
struct Embedding {
    data: Vec<f32>,
    dims: Vec<usize>,
}

struct SamSessions {
    /// Kept alive with the sessions (they need the environment alive; never read).
    _env: ort::environment::Environment,
    enc: Session,
    dec: Session,
}

/// The two sessions, loaded once and kept (encoder ~0.1 GB, decoder a few MB, RAM).
static SESSIONS: Mutex<Option<(PathBuf, PathBuf, SamSessions)>> = Mutex::new(None);
/// Embeddings by (image bytes hash, size) — a re-uploaded file hashes differently and re-encodes.
static EMB: Mutex<VecDeque<(String, Embedding)>> = Mutex::new(VecDeque::new());

fn ort_err(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal, format!("The selection model failed: {e}"))
}

/// Both MobileSAM files are where the service's ONNX runtime can see them.
pub fn installed(sdcpp_dir: &Path) -> bool {
    matting::sam_installed(sdcpp_dir)
}

/// The encoder's input size for a `w`×`h` mask ask — the webview variant's exact computation
/// (1024 on the long side; the ask is already ≤ that in practice).
pub fn encode_size(w: u32, h: u32) -> (u32, u32) {
    let scale = SIDE as f32 / (w.max(h)) as f32;
    (
        (w as f32 * scale).round().max(1.0) as u32,
        (h as f32 * scale).round().max(1.0) as u32,
    )
}

/// The encoder's input: `img` at `w`×`h`, 0-255 HWC — rank 3, no batch dim: the export takes
/// exactly what the webview variant sends it (`[rh, rw, 3]`).
pub fn encoder_input(img: &RgbaImage, w: u32, h: u32) -> (Vec<f32>, Vec<usize>) {
    let small = image::imageops::resize(img, w, h, FilterType::Triangle);
    let n = (w * h) as usize;
    let mut x = vec![0f32; 3 * n];
    for (i, p) in small.pixels().enumerate() {
        x[i * 3] = p.0[0] as f32;
        x[i * 3 + 1] = p.0[1] as f32;
        x[i * 3 + 2] = p.0[2] as f32;
    }
    (x, vec![h as usize, w as usize, 3])
}

/// The decoder's points, in the encoder's coordinates: the webview variant's exact layout —
/// clicks × scale (1024/max(mask side)), a box as two corners (2, 3), else one -1 padding point.
pub fn decoder_points(
    points: &[(f32, f32, i32)],
    box_: Option<(f32, f32, f32, f32)>,
    scale: f32,
) -> (Vec<f32>, Vec<f32>) {
    let mut all: Vec<(f32, f32, f32)> = points
        .iter()
        .map(|(x, y, l)| (x * scale, y * scale, *l as f32))
        .collect();
    match box_ {
        Some((x0, y0, x1, y1)) => {
            all.push((x0 * scale, y0 * scale, 2.0));
            all.push((x1 * scale, y1 * scale, 3.0));
        }
        None => all.push((0.0, 0.0, -1.0)),
    }
    if all.is_empty() {
        all.push((0.0, 0.0, -1.0));
    }
    let coords = all.iter().flat_map(|(x, y, _)| [*x, *y]).collect();
    let labels = all.iter().map(|(_, _, l)| *l).collect();
    (coords, labels)
}

/// The decoder's mask at (w)×(h) -> 0/255 (the webview's `fromLogits` tests >0).
pub fn mask_to_gray(data: &[f32], w: u32, h: u32) -> GrayImage {
    GrayImage::from_fn(w, h, |x, y| {
        image::Luma([if data[(y * w + x) as usize] > 0.0 { 255 } else { 0 }])
    })
}

/// Where the lab's model runs for a call (the ladder, `Engine::segment_ladder`): this
/// box's own pinned files (in-process, the single-box path) or the door behind the box's
/// gateway (the split-box path). Both arms answer the same 4-mask contract.
pub enum SegmentPath {
    InProcess(Sam),
    Door(Door),
}

/// MobileSAM as the service runs it.
pub struct Sam {
    sdcpp_dir: PathBuf,
}

/// The lab's click-to-select engine run **outside this box** (task-037 P3, D-037-13):
/// a stock `vision-server` (vision.cpp — the author of this lab's own ONNX exports,
/// MIT) scheduled by the box's load-on-demand gateway, the split-box path. The serving
/// box stores no model files and runs no ONNX; the accelerator box loads the pinned
/// GGUFs and answers the *same 4-mask contract* this module produces in-process, so the
/// browser stays a thin client either way and never knows where the model ran (the
/// task-036 relay's byte-transparency, kept).
///
/// The door speaks a coarser prompt than the in-process decoder: exactly one of a single
/// point or a box (its `POST /sam`), never both, and no negative clicks. The lab's actual
/// calls are exactly those two shapes (a click, or a box with no clicks), so the mapping
/// is faithful for the product's interactions: a box goes as its corners, otherwise the
/// last positive click (else the first, whatever its label) goes as *the* point.
pub struct Door {
    client: reqwest::blocking::Client,
    key: Option<String>,
    /// The gateway root's `{gateway}/{door}` — a v269 matrix gateway is configured as
    /// `.../upstream`, whose `/upstream/<model>/<path>` route is the only per-model path
    /// (a bare `/<model>/...` on the gateway root 404s, verified on the box 2026-11-22).
    base: String,
}

/// An error body as a short plain string (for a user message; the raw body goes to the log).
fn plain(t: &str) -> String {
    let t = t.trim();
    if t.len() <= 200 {
        return t.to_string();
    }
    let mut e = 200;
    while e > 0 && !t.is_char_boundary(e) {
        e -= 1;
    }
    format!("{}…", &t[..e])
}

impl Door {
    /// `gateway` is the box's load-on-demand gateway root (the `visionEngine.gateway`;
    /// same shape as the box's `externalEngine.gateway`); `door` the matrix id of the
    /// vision.cpp door (`kokon-vision`). `key` is the gateway's admission key
    /// (empty = a keyless gateway, no header).
    ///
    /// The request timeout covers a *cold* door: a request to an unloaded model makes the
    /// gateway boot it (the two small GGUFs, ~1 s on the box) before the first answer.
    pub fn new(gateway: &str, door: &str, key: &str) -> AppResult<Self> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection door cannot start: {e}")))?;
        let key = key.trim().to_string();
        Ok(Self {
            client,
            key: (!key.is_empty()).then_some(key),
            base: format!("{}/{}", gateway.trim_end_matches('/'), door.trim_start_matches('/')),
        })
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::RequestBuilder {
        let r = self.client.request(method, format!("{}{}", self.base, path));
        match &self.key {
            Some(k) => r.bearer_auth(k),
            None => r,
        }
    }

    /// The door's `checkEndpoint`: 200 when the engine is up (a request boots it on
    /// demand, the gateway's job). Any failure here means "not available", not an error
    /// worth toasting: the caller falls through the ladder (in-process, then said plain).
    pub fn capabilities(&self) -> AppResult<()> {
        let res = self
            .req(reqwest::Method::GET, "/health")
            .send()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection door is unreachable: {e}")))?;
        if res.status().is_success() {
            Ok(())
        } else {
            let s = res.status().as_u16();
            Err(AppError::new(
                ErrorCode::Internal,
                format!("The selection door answered {s} at /health."),
            ))
        }
    }

    /// The door has no encode step: its server runs the encoder itself per request and
    /// caches the embedding by the image's bytes, so a stream of clicks on the same image
    /// is cheap exactly as with the in-process cache. The service's encode route is a
    /// no-op on this path (the first predict pays the one-time encoder run).
    pub fn encode(&self) -> AppResult<()> {
        Ok(())
    }

    /// SAM's 4 masks at `w`×`h` (0/255) plus the predicted IoUs — the same shape as
    /// [`Sam::predict`]. `img` is the source image at any size: the door answers masks at
    /// the size it is *sent*, so it is sent at exactly `w`×`h` (the in-process decoder's
    /// `orig_im_size` equivalent), with the clicks in that same space. The image is
    /// re-encoded as PNG: the door takes base64 PNG/JPEG.
    pub fn predict(
        &self,
        img: &RgbaImage,
        w: u32,
        h: u32,
        points: &[(f32, f32, i32)],
        box_: Option<(f32, f32, f32, f32)>,
    ) -> AppResult<(Vec<GrayImage>, Vec<f32>)> {
        let small = image::imageops::resize(img, w.max(1), h.max(1), FilterType::Triangle);
        let png = crate::imaging::encode_png(&small, &[], None, true)?;
        let mut body = serde_json::json!({ "image": base64::engine::general_purpose::STANDARD.encode(&png) });
        match box_ {
            // A box prompt: the door takes two corners (its x1,y1,x2,y2; the service's
            // x0,y0,x1,y1 in mask pixels, clamped into the image, never degenerate — the
            // door rejects a zero-area box).
            Some((x0, y0, x1, y1)) => {
                let (a, b) = (x0.min(x1), x1.max(x0));
                let (c, d) = (y0.min(y1), y1.max(y0));
                let clamp = |v: f32, n: u32| (v.round().clamp(0.0, (n - 1) as f32)) as u32;
                let (x1, x2) = (clamp(a, w), clamp(b, w).max(clamp(a, w) + 1));
                let (y1, y2) = (clamp(c, h), clamp(d, h).max(clamp(c, h) + 1));
                body["box"] = serde_json::json!({ "x1": x1, "y1": y1, "x2": x2, "y2": y2 });
            }
            None => {
                // The door takes one point: the last positive click (the object), else the
                // first of any (the lab sends single clicks; a box-plus-clicks ask never
                // arrives from the UI, and a box is the stronger prompt when present).
                let p = points
                    .iter()
                    .rev()
                    .find(|(_, _, l)| *l == 1)
                    .or_else(|| points.first())
                    .ok_or_else(|| {
                        AppError::invalid("Selection needs a click or a box.")
                    })?;
                let (x, y) = (p.0, p.1);
                let clamp = |v: f32, n: u32| (v.round().clamp(0.0, (n - 1) as f32)) as u32;
                body["point"] = serde_json::json!({ "x": clamp(x, w), "y": clamp(y, h) });
            }
        }
        let res = self
            .req(reqwest::Method::POST, "/sam")
            .json(&body)
            .send()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection door failed: {e}")))?;
        if !res.status().is_success() {
            let s = res.status().as_u16();
            let t = res.text().unwrap_or_default();
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("The selection door failed: {s} {t}"),
            ));
        }
        let v: serde_json::Value = res
            .json()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection door's answer is not JSON: {e}")))?;
        let iou = v["iou"]
            .as_array()
            .ok_or_else(|| AppError::internal("The selection door returned no IoUs."))?
            .iter()
            .map(|x| x.as_f64().unwrap_or(0.0) as f32)
            .collect::<Vec<_>>();
        let masks_b64 = v["masks"]
            .as_array()
            .ok_or_else(|| AppError::internal("The selection door returned no masks."))?;
        if masks_b64.len() != 4 {
            return Err(AppError::internal(
                "The selection door returned the wrong number of masks.",
            ));
        }
        let mut masks = Vec::new();
        for m in masks_b64 {
            let raw = base64::engine::general_purpose::STANDARD
                .decode(m.as_str().unwrap_or(""))
                .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection door's mask is not base64: {e}")))?;
            let im = image::ImageReader::new(std::io::Cursor::new(&raw[..]))
                .with_guessed_format()
                .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection door's mask is not an image: {e}")))?
                .decode()
                .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection door's mask will not decode: {e}")))?;
            let (mw, mh) = (im.width(), im.height());
            if mw != w || mh != h {
                return Err(AppError::new(
                    ErrorCode::Internal,
                    format!("The selection door returned masks at {mw}×{mh}, expected {w}×{h}."),
                ));
            }
            // The door's masks are already thresholded; normalise to the same 0/255 bytes
            // the in-process path returns (the browser's `fromLogits` tests > 0).
            let raw = im.to_luma8().into_raw();
            masks.push(GrayImage::from_fn(w, h, |x, y| {
                image::Luma([if raw[(y * w + x) as usize] > 0 { 255 } else { 0 }])
            }));
        }
        Ok((masks, iou))
    }

    /// The door's matte (`/birefnet`): the image as sent, its matte PNG at the size sent —
    /// the same output contract as the in-process run (a 0-255 matte of the image's own
    /// size: the in-process path runs on 1024 and resizes the matte back, this one sends
    /// the image and the door's server does its own resize). The door request cannot be
    /// cancelled mid-flight (a Stop while it is in flight lets it finish: a few seconds).
    pub fn matte(&self, img: &RgbaImage) -> AppResult<GrayImage> {
        let png = crate::imaging::encode_png(img, &[], None, true)?;
        let body = serde_json::json!({ "image": base64::engine::general_purpose::STANDARD.encode(&png) });
        let res = self
            .req(reqwest::Method::POST, "/birefnet")
            .json(&body)
            .send()
            .map_err(|e| {
                AppError::new(
                    ErrorCode::ServerUnavailable,
                    format!("The sticker model's engine is not answering ({e})."),
                )
            })?;
        if !res.status().is_success() {
            let s = res.status().as_u16();
            let t = res.text().unwrap_or_default();
            return Err(AppError::new(
                ErrorCode::ServerUnavailable,
                format!("The sticker model's engine failed: {s} {}", plain(&t)),
            ));
        }
        let v: serde_json::Value = res
            .json()
            .map_err(|_| AppError::internal("The sticker model's engine answered without a matte."))?;
        let b64 = v["image"]
            .as_str()
            .ok_or_else(|| AppError::internal("The sticker model's engine answered without a matte."))?;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The sticker model's matte is not base64: {e}")))?;
        let im = image::ImageReader::new(std::io::Cursor::new(&raw[..]))
            .with_guessed_format()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The sticker model's matte is not an image: {e}")))?
            .decode()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The sticker model's matte will not decode: {e}")))?;
        let (mw, mh) = (im.width(), im.height());
        if mw != img.width() || mh != img.height() {
            return Err(AppError::new(
                ErrorCode::Internal,
                format!(
                    "The sticker model's engine returned a matte at {mw}×{mh}, expected {}×{}.",
                    img.width(),
                    img.height()
                ),
            ));
        }
        Ok(im.to_luma8())
    }

}

impl Sam {
    pub fn new(sdcpp_dir: impl Into<PathBuf>) -> Self {
        Self { sdcpp_dir: sdcpp_dir.into() }
    }

    fn sessions(&self) -> AppResult<std::sync::MutexGuard<'_, Option<(PathBuf, PathBuf, SamSessions)>>> {
        let (enc_p, dec_p) = (
            matting::sam_file(&self.sdcpp_dir, "mobile_sam_image_encoder.onnx"),
            matting::sam_file(&self.sdcpp_dir, "sam_mask_decoder_multi.onnx"),
        );
        let (Some(enc_p), Some(dec_p)) = (enc_p, dec_p) else {
            return Err(AppError::new(
                ErrorCode::MissingModel,
                "The selection model is not downloaded yet.",
            ));
        };
        let mut g = SESSIONS.lock().unwrap_or_else(|p| p.into_inner());
        if g.as_ref().is_none_or(|(e, d, _)| *e != enc_p || *d != dec_p) {
            if !installed(&self.sdcpp_dir) {
                return Err(AppError::new(
                    ErrorCode::MissingModel,
                    "The selection model is not downloaded yet.",
                ));
            }
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            let env = crate::onnx::environment().map_err(ort_err)?;
            let enc = crate::onnx::cpu_session(&env, &enc_p, threads).map_err(ort_err)?;
            let dec = crate::onnx::cpu_session(&env, &dec_p, threads).map_err(ort_err)?;
            *g = Some((enc_p, dec_p, SamSessions { _env: env, enc, dec }));
        }
        Ok(g)
    }

    /// The encoder for `img` at the size a `w`×`h` mask needs; `key` is the cache key the
    /// caller derives from the image (the engine uses its bytes' hash, so a replaced file re-encodes).
    pub fn encode(&self, key: &str, img: &RgbaImage, w: u32, h: u32) -> AppResult<()> {
        let (rw, rh) = encode_size(w, h);
        let (x, dims) = encoder_input(img, rw, rh);
        let mut g = self.sessions()?;
        let s = &mut g.as_mut().unwrap().2;
        let input = Tensor::from_array((dims, x.into_boxed_slice())).map_err(ort_err)?;
        let opts = RunOptions::new().map_err(ort_err)?;
        // By name: the export's one input.
        let mut inputs = HashMap::new();
        inputs.insert("input_image", input);
        // The export has one output: the embeddings.
        let outputs = s.enc.run_with_options(inputs, &opts).map_err(ort_err)?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(ort_err)?;
        let emb = Embedding { data: data.to_vec(), dims: shape.iter().map(|d| *d as usize).collect() };
        let mut c = EMB.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(pos) = c.iter().position(|(k, _)| k == key) {
            c.remove(pos);
        }
        c.push_back((key.to_string(), emb));
        while c.len() > EMB_CACHE {
            c.pop_front();
        }
        Ok(())
    }

    /// SAM's 4 masks at `w`×`h` (0/255) plus the predicted IoUs: `points` are clicks in mask
    /// pixels (label 0 = not, 1 = object); `box_` confines the object (mask pixels). The
    /// embedding is taken from the cache or made on the spot (the lab encodes first, so this
    /// usually just runs the decoder).
    pub fn predict(
        &self,
        key: &str,
        img: &RgbaImage,
        w: u32,
        h: u32,
        points: &[(f32, f32, i32)],
        box_: Option<(f32, f32, f32, f32)>,
    ) -> AppResult<(Vec<GrayImage>, Vec<f32>)> {
        {
            let c = EMB.lock().unwrap_or_else(|p| p.into_inner());
            if c.iter().all(|(k, _)| k != key) {
                drop(c);
                self.encode(key, img, w, h)?;
            }
        }
        let emb = EMB
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, e)| Embedding::clone(e))
            .ok_or_else(|| AppError::internal("The selection model's embedding is missing."))?;

        let scale = SIDE as f32 / (w.max(h)) as f32;
        let (coords, labels) = decoder_points(points, box_, scale);
        let mut g = self.sessions()?;
        let s = &mut g.as_mut().unwrap().2;
        let n = coords.len() / 2;
        // The decoder's output indices by name (masks, iou_predictions), before the run: the
        // session's outputs borrow the session, and so do its run's results.
        let i_masks = s.dec.outputs().iter().position(|o| o.name() == "masks").unwrap_or(0);
        let i_iou = s.dec.outputs().iter().position(|o| o.name() == "iou_predictions").unwrap_or(0);
        let emb_t = Tensor::from_array((emb.dims, emb.data.into_boxed_slice())).map_err(ort_err)?;
        let coords_t =
            Tensor::from_array((vec![1, n, 2], coords.into_boxed_slice())).map_err(ort_err)?;
        let labels_t =
            Tensor::from_array((vec![1, n], labels.into_boxed_slice())).map_err(ort_err)?;
        let mask_in = Tensor::from_array((vec![1usize, 1, 256, 256], vec![0f32; 256 * 256].into_boxed_slice())).map_err(ort_err)?;
        let has_in = Tensor::from_array((vec![1usize], vec![0.0f32].into_boxed_slice())).map_err(ort_err)?;
        let orig = Tensor::from_array((vec![2usize], vec![h as f32, w as f32].into_boxed_slice())).map_err(ort_err)?;
        let opts = RunOptions::new().map_err(ort_err)?;
        // By name, not by position: the webview's onnxruntime matches by name, and the
        // export's declared order is not the contract.
        let mut inputs = HashMap::new();
        inputs.insert("image_embeddings", emb_t);
        inputs.insert("point_coords", coords_t);
        inputs.insert("point_labels", labels_t);
        inputs.insert("mask_input", mask_in);
        inputs.insert("has_mask_input", has_in);
        inputs.insert("orig_im_size", orig);
        let outputs = s.dec.run_with_options(inputs, &opts).map_err(ort_err)?;
        // `try_extract_tensor` yields a *view* into the session's memory, not a copy: take
        // the data out before any further work on the session, as the other consumers of
        // this runtime do (watermark, matting).
        let (m_shape, m_view) = outputs[i_masks].try_extract_tensor::<f32>().map_err(ort_err)?;
        let m_data: Vec<f32> = m_view.to_vec();
        let k = *m_shape.iter().nth(1).unwrap_or(&4) as usize;
        if k != 4 {
            return Err(AppError::new(ErrorCode::Internal, "The selection model returned an unexpected number of masks."));
        }
        let (mw, mh) = (
            *m_shape.iter().last().unwrap_or(&(w as i64)) as usize,
            *m_shape.iter().nth(m_shape.len() - 2).unwrap_or(&(h as i64)) as usize,
        );
        if mw != w as usize || mh != h as usize {
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("The selection model returned masks at {mw}×{mh}, expected {w}×{h}."),
            ));
        }
        let iou = outputs[i_iou]
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?
            .1
            .to_vec();
        let masks = (0..k).map(|i| mask_to_gray(&m_data[i * w as usize * h as usize..(i + 1) * w as usize * h as usize], w, h)).collect();
        Ok((masks, iou))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_size_is_1024_on_the_long_side_as_the_webview_computes_it() {
        // The webview: sw = round(w * min(1, 1024/max(w,h))), then the encoder upsamples to
        // 1024 on the long side again.
        assert_eq!(encode_size(800, 600), (1024, 768));
        assert_eq!(encode_size(2048, 683), (1024, 342));
        assert_eq!(encode_size(1024, 1024), (1024, 1024));
        assert_eq!(encode_size(512, 512), (1024, 1024));
        assert_eq!(encode_size(1, 1), (1024, 1024));
        assert_eq!(encode_size(2048, 2048), (1024, 1024));
    }

    #[test]
    fn encoder_input_is_0_255_hwc_like_the_webview() {
        // Two columns, one red, one green.
        let img = image::RgbaImage::from_pixel(2, 1, image::Rgba([255, 0, 0, 255]));
        let mut img = image::DynamicImage::ImageRgba8(img).to_rgba8();
        img[(1, 0)] = image::Rgba([0, 255, 0, 255]);
        let (x, dims) = encoder_input(&img, 2, 1);
        assert_eq!(dims, vec![1, 2, 3]);
        assert_eq!(x, vec![255.0, 0.0, 0.0, 0.0, 255.0, 0.0]);
    }

    #[test]
    fn decoder_points_match_the_webview_layout() {
        // One click, no box: the click plus the -1 padding point.
        let (coords, labels) = decoder_points(&[(100.0, 50.0, 1)], None, 2.0);
        assert_eq!(coords, vec![200.0, 100.0, 0.0, 0.0]);
        assert_eq!(labels, vec![1.0, -1.0]);
        // With a box: the corners as 2 and 3, no padding.
        let (coords, labels) = decoder_points(&[(10.0, 20.0, 1)], Some((0.0, 0.0, 40.0, 30.0)), 2.0);
        assert_eq!(coords, vec![20.0, 40.0, 0.0, 0.0, 80.0, 60.0]);
        assert_eq!(labels, vec![1.0, 2.0, 3.0]);
        // No clicks at all still gets the padding point (the decoder wants at least one).
        let (_, labels) = decoder_points(&[], None, 1.0);
        assert_eq!(labels, vec![-1.0]);
    }

    #[test]
    fn masks_threshold_to_0_and_255() {
        let m = mask_to_gray(&[-1.0, 0.0, 0.5, 100.0], 2, 2);
        let p: Vec<_> = m.pixels().map(|p| p.0[0]).collect();
        assert_eq!(p, vec![0, 0, 255, 255]);
    }

    #[test]
    fn missing_model_is_said() {
        let d = tempfile::tempdir().unwrap();
        let sam = Sam::new(d.path());
        let e = sam.encode("k", &RgbaImage::new(4, 4), 8, 8).unwrap_err();
        assert_eq!(e.code, ErrorCode::MissingModel);
        assert!(e.to_string().contains("not downloaded"));
    }

    /// Real model (the two pinned files, `KOKON_SEGMENT_DIR` a folder with
    /// `models/segment/*.onnx`, `KOKON_SEGMENT_IMAGE` a photo): encode, click, and check the contract.
    #[test]
    #[ignore = "needs the model files"]
    fn real_model_encodes_and_predicts() {
        let dir = PathBuf::from(std::env::var("KOKON_SEGMENT_DIR").unwrap());
        if !installed(&dir) {
            eprintln!("skipped: the model is not downloaded here");
            return;
        }
        let img = image::open(std::env::var("KOKON_SEGMENT_IMAGE").unwrap()).unwrap().to_rgba8();
        let sam = Sam::new(&dir);
        let t = std::time::Instant::now();
        sam.encode("t", &img, 800, 600).expect("encode");
        eprintln!("encode in {:.2} s", t.elapsed().as_secs_f32());
        let t = std::time::Instant::now();
        let (masks, iou) = sam.predict("t", &img, 800, 600, &[(100.0, 100.0, 1)], None).expect("predict");
        eprintln!("predict in {:.2} s", t.elapsed().as_secs_f32());
        assert_eq!(masks.len(), 4);
        assert_eq!(iou.len(), 4);
        assert_eq!(masks[0].dimensions(), (800, 600));
        // A click somewhere returns a mask with some on and some off (not degenerate).
        let on = masks[0].pixels().filter(|p| p.0[0] == 255).count();
        assert!(on > 0 && on < 800 * 600);
    }

    // ---------------------------------------------------------------- the door

    /// A throwaway HTTP server that answers each request with the next queued
    /// (status, body) and records (request line, authorization, body) — the same
    /// fixture the task-036 relay's tests used (revived 11-23 for the vision.cpp
    /// door's contract: `/health`, `POST /sam` with base64 image, one of point/box).
    fn fake_door(responses: Vec<(u16, String)>) -> (String, std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String, String)>::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for (status, resp_body) in responses {
                let Ok((mut sock, _)) = listener.accept() else { break };
                let mut req = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    let n = sock.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    req.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&req).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let have = text[end + 4..].len();
                        let mut len = 0usize;
                        for l in text.lines() {
                            let lower = l.to_ascii_lowercase();
                            if let Some(v) = lower.strip_prefix("content-length:") {
                                len = v.trim().parse::<usize>().unwrap_or(0);
                            }
                        }
                        if have >= len {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&req).to_string();
                let head = text.split("\r\n\r\n").next().unwrap_or("").to_string();
                let line = head.lines().next().unwrap_or("").to_string();
                let mut auth = String::new();
                for l in head.lines() {
                    let lower = l.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("authorization:") {
                        auth = v.trim().to_string();
                    }
                }
                let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
                seen2.lock().unwrap().push((line, auth, body));
                let resp_bytes: Vec<u8> = resp_body.into_bytes();
                let blen = resp_bytes.len();
                let resp = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status, blen
                );
                let _ = sock.write_all(resp.as_bytes());
                let _ = sock.write_all(&resp_bytes);
                let _ = sock.flush();
            }
        });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    /// A 1×2 RGBA image as PNG, base64 — what the door is sent for a 1×2 mask ask.
    fn png_b64(img: &RgbaImage) -> String {
        let b = crate::imaging::encode_png(img, &[], None, true).unwrap();
        base64::engine::general_purpose::STANDARD.encode(&b)
    }

    /// `n` masks, each a real `w`×`h` PNG (half on, half off), base64 — the door sends
    /// PNGs, and this client decodes them, so the fixture has to too.
    fn mask_b64s(n: usize, w: u32, h: u32) -> Vec<String> {
        (0..n)
            .map(|i| {
                let im = RgbaImage::from_fn(w, h, |x, _| {
                    if (x as u32 * h + i as u32) % 2 == 0 {
                        image::Rgba([255, 255, 255, 255])
                    } else {
                        image::Rgba([0, 0, 0, 255])
                    }
                });
                let b = crate::imaging::encode_png(&im, &[], None, true).unwrap();
                base64::engine::general_purpose::STANDARD.encode(&b)
            })
            .collect()
    }

    #[test]
    fn door_sends_the_key_and_parses_the_contract() {
        let img = RgbaImage::from_pixel(2, 1, image::Rgba([9, 8, 7, 255]));
        let sent = png_b64(&img);
        let body = serde_json::json!({ "width": 2, "height": 1, "masks": mask_b64s(4, 2, 1), "iou": [0.1, 0.2, 0.3, 0.4] });
        let (gw, seen) = fake_door(vec![
            (200, r#"{"status":"ok","models":[]}"#.to_string()),
            (200, body.to_string()),
        ]);
        let door = Door::new(&gw, "kokon-vision", "sekret").unwrap();
        door.capabilities().expect("capabilities");
        door.encode().expect("encode is a no-op on the door");
        let (masks, iou) = door
            .predict(&img, 2, 1, &[(1.0, 0.0, 1)], None)
            .expect("predict");
        assert_eq!(masks.len(), 4);
        assert_eq!(iou, vec![0.1, 0.2, 0.3, 0.4]);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, "GET /kokon-vision/health HTTP/1.1");
        assert_eq!(seen[0].1.to_ascii_lowercase(), "bearer sekret");
        assert_eq!(seen[1].0, "POST /kokon-vision/sam HTTP/1.1");
        assert_eq!(seen[1].1.to_ascii_lowercase(), "bearer sekret");
        let v: serde_json::Value = serde_json::from_str(&seen[1].2).unwrap();
        assert_eq!(v["image"].as_str().unwrap(), sent);
        assert_eq!(v["point"], serde_json::json!({ "x": 1, "y": 0 }));
        assert!(v.get("box").is_none());
    }

    #[test]
    fn door_sends_a_box_as_two_corners() {
        // The service's (x0,y0,x1,y1) in mask pixels, 100×50; out-of-range corners
        // clamp into the image and a zero-area box grows by one pixel (the door
        // rejects degenerate boxes).
        let img = RgbaImage::from_pixel(100, 50, image::Rgba([1, 2, 3, 255]));
        let body = serde_json::json!({ "width": 100, "height": 50, "masks": mask_b64s(4, 100, 50), "iou": [0.1, 0.1, 0.1, 0.1] });
        let (gw, seen) = fake_door(vec![(200, body.to_string())]);
        let door = Door::new(&gw, "kokon-vision", "").unwrap();
        door.predict(&img, 100, 50, &[], Some((-5.0, 10.5, 104.2, 49.0)))
            .expect("predict");
        let v: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0].2).unwrap();
        assert_eq!(v["box"], serde_json::json!({ "x1": 0, "y1": 11, "x2": 99, "y2": 49 }));
        assert!(v.get("point").is_none());
        // A degenerate box (x0 == x1) still goes: clamped, one pixel wide.
        let (gw, seen) = fake_door(vec![(200, body.to_string())]);
        let door = Door::new(&gw, "kokon-vision", "").unwrap();
        door.predict(&img, 100, 50, &[], Some((30.0, 10.0, 30.0, 20.0))).expect("predict");
        let v: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0].2).unwrap();
        assert_eq!(v["box"], serde_json::json!({ "x1": 30, "y1": 10, "x2": 31, "y2": 20 }));
    }

    #[test]
    fn door_point_is_the_last_positive_click() {
        let img = RgbaImage::from_pixel(10, 10, image::Rgba([1, 1, 1, 255]));
        let body = serde_json::json!({ "width": 10, "height": 10, "masks": mask_b64s(4, 10, 10), "iou": [0.1, 0.1, 0.1, 0.1] });
        let (gw, seen) = fake_door(vec![(200, body.to_string())]);
        let door = Door::new(&gw, "kokon-vision", "").unwrap();
        // A negative then a positive click: the positive one (the object) goes.
        door.predict(&img, 10, 10, &[(1.0, 1.0, 0), (2.5, 3.5, 1)], None).expect("predict");
        let v: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0].2).unwrap();
        assert_eq!(v["point"], serde_json::json!({ "x": 3, "y": 4 }));
        // Only negative clicks: the first goes (the door's contract has no negative,
        // and the lab's UI never sends a bare negative ask).
        let (gw, seen) = fake_door(vec![(200, body.to_string())]);
        let door = Door::new(&gw, "kokon-vision", "").unwrap();
        door.predict(&img, 10, 10, &[(7.0, 8.0, 0)], None).expect("predict");
        let v: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0].2).unwrap();
        assert_eq!(v["point"], serde_json::json!({ "x": 7, "y": 8 }));
        // No prompt at all: a plain refusal, not a request.
        let (gw, _) = fake_door(vec![]);
        let door = Door::new(&gw, "kokon-vision", "").unwrap();
        let e = door.predict(&img, 10, 10, &[], None).unwrap_err();
        assert!(e.to_string().contains("click or a box"));
    }

    #[test]
    fn door_matte_parses_the_contract() {
        // A 4×2 RGBA in, the door's matte back: a real 4×2 PNG in the "image" field.
        let img = RgbaImage::from_fn(4, 2, |x, _| if x % 2 == 0 { image::Rgba([1, 2, 3, 255]) } else { image::Rgba([4, 5, 6, 255]) });
        let matte = RgbaImage::from_fn(4, 2, |x, _| image::Rgba([x as u8 * 40, 0, 0, 255]));
        let mb = crate::imaging::encode_png(&matte, &[], None, true).unwrap();
        let mb64 = base64::engine::general_purpose::STANDARD.encode(&mb);
        let sent = png_b64(&img);
        let (gw, seen) = fake_door(vec![(200, serde_json::json!({ "width": 4, "height": 2, "image": mb64 }).to_string())]);
        let door = Door::new(&gw, "kokon-vision", "k").unwrap();
        let m = door.matte(&img).expect("matte");
        assert_eq!(m.dimensions(), (4, 2));
        let v: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0].2).unwrap();
        assert_eq!(v["image"].as_str().unwrap(), sent);
        assert_eq!(seen.lock().unwrap()[0].0, "POST /kokon-vision/birefnet HTTP/1.1");
        // A matte at a different size is a contract failure, said plain.
        let big = RgbaImage::from_fn(8, 8, |_, _| image::Rgba([9, 9, 9, 255]));
        let bb = crate::imaging::encode_png(&big, &[], None, true).unwrap();
        let bb64 = base64::engine::general_purpose::STANDARD.encode(&bb);
        let (gw, _) = fake_door(vec![(200, serde_json::json!({ "width": 8, "height": 8, "image": bb64 }).to_string())]);
        let door = Door::new(&gw, "kokon-vision", "k").unwrap();
        let e = door.matte(&img).unwrap_err();
        assert!(e.to_string().contains("4×2"));
    }

    #[test]
    fn door_maps_failures() {
        let img = RgbaImage::from_pixel(8, 8, image::Rgba([1, 1, 1, 255]));
        let (gw, _) = fake_door(vec![
            (503, r#"{"error":"loading"}"#.to_string()),
            (500, r#"{"error":"boom"}"#.to_string()),
        ]);
        let door = Door::new(&gw, "kokon-vision", "k").unwrap();
        let e = door.capabilities().unwrap_err();
        assert!(e.to_string().contains("503"));
        let e = door.predict(&img, 8, 8, &[(1.0, 1.0, 1)], None).unwrap_err();
        assert!(e.to_string().contains("boom"));
        // A mask count that is not four is a contract failure, said plain.
        let (gw, _) = fake_door(vec![(200, r#"{"width":8,"height":8,"masks":["AA=="],"iou":[0.1]}"#.to_string())]);
        let door = Door::new(&gw, "kokon-vision", "k").unwrap();
        let e = door.predict(&img, 8, 8, &[(1.0, 1.0, 1)], None).unwrap_err();
        // A contract violation is an internal error: the user sees the plain form, the
        // specifics ride in the detail (the log), the codebase's convention.
        assert!(e.detail.as_deref().unwrap_or("").contains("wrong number of masks"));
    }
}
