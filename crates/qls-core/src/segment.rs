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

use image::{GrayImage, RgbaImage, imageops::FilterType};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::matting;
use base64::Engine as _;
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

/// MobileSAM as the service runs it.
pub struct Sam {
    sdcpp_dir: PathBuf,
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

/// The split-box relay (D-036-1): when this box has no local copy of the two pinned
/// files, the lab's encode/predict travel to the gateway's `kokon-sam` door — the engine
/// on the compute box loads the same pinned files and answers with the *same JSON this
/// service would have produced itself* (its contract mirrors this module), so the relay
/// is byte-transparent: the browser never knows where the model ran, and the serving box
/// stores no 45 MB and runs no ONNX.
pub struct Relay {
    client: reqwest::blocking::Client,
    key: Option<String>,
    base: String,
}

impl Relay {
    /// `gateway` is the box's load-on-demand gateway (the `externalEngine.gateway` root);
    /// the door is `kokon-sam`, a sibling of the sdcpp door the box already names.
    /// `key` is the gateway's admission key (empty = a keyless gateway, no header).
    ///
    /// The request timeout covers a *cold* door: a request to an unloaded model makes
    /// the gateway boot it (venv + the two models, ~10-20 s) before the first answer.
    pub fn new(gateway: &str, key: &str) -> AppResult<Self> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection relay cannot start: {e}")))?;
        let key = key.trim().to_string();
        Ok(Self {
            client,
            key: (!key.is_empty()).then_some(key),
            // Same base contract as the box's sdcpp door: the configured `externalEngine.gateway`
        // already carries the gateway's per-model prefix (a v269 matrix gateway is configured
        // as `.../upstream`, whose /upstream/<model>/<path> route is the only per-model path —
        // a bare /<model>/... on the gateway root 404s, verified on the box 2026-11-22). So the
        // relay is a sibling of the sdcpp door: one configured base, one key, the door id in the
        // path. The door itself is keyless; the box's key is the gateway's door ticket.
        base: format!("{}/kokon-sam", gateway.trim_end_matches('/')),
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
    /// demand). Any failure here means "not installed", not an error worth toasting.
    pub fn capabilities(&self) -> AppResult<()> {
        let res = self
            .req(reqwest::Method::GET, "/segment/capabilities")
            .send()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection relay is unreachable: {e}")))?;
        if res.status().is_success() {
            Ok(())
        } else {
            let s = res.status().as_u16();
            Err(AppError::new(
                ErrorCode::Internal,
                format!("The selection engine door answered {s} at /segment/capabilities."),
            ))
        }
    }

    /// `POST /segment/encode`: the image bytes once per (image, size) on the door.
    pub fn encode(&self, image: &[u8], w: u32, h: u32) -> AppResult<()> {
        let body = serde_json::json!({
            "image": base64::engine::general_purpose::STANDARD.encode(image),
            "width": w,
            "height": h,
        });
        let res = self
            .req(reqwest::Method::POST, "/segment/encode")
            .json(&body)
            .send()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection relay failed: {e}")))?;
        if !res.status().is_success() {
            let s = res.status().as_u16();
            let t = res.text().unwrap_or_default();
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("The selection engine failed to read the image: {s} {t}"),
            ));
        }
        Ok(())
    }

    /// `POST /segment/predict`: the 4 masks as 0/255 PNG bytes plus the predicted IoUs —
    /// the same shape this service's own handler returns to the browser.
    pub fn predict(
        &self,
        image: &[u8],
        w: u32,
        h: u32,
        points: &[(f32, f32, i32)],
        box_: Option<(f32, f32, f32, f32)>,
    ) -> AppResult<(Vec<Vec<u8>>, Vec<f32>)> {
        let body = serde_json::json!({
            "image": base64::engine::general_purpose::STANDARD.encode(image),
            "width": w,
            "height": h,
            "points": points.iter().map(|(x, y, l)| serde_json::json!({ "x": x, "y": y, "label": l })).collect::<Vec<_>>(),
            "box": box_.map(|(x0, y0, x1, y1)| serde_json::json!({ "x0": x0, "y0": y0, "x1": x1, "y1": y1 })),
        });
        let res = self
            .req(reqwest::Method::POST, "/segment/predict")
            .json(&body)
            .send()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection relay failed: {e}")))?;
        if !res.status().is_success() {
            let s = res.status().as_u16();
            let t = res.text().unwrap_or_default();
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("The selection engine failed: {s} {t}"),
            ));
        }
        let v: serde_json::Value = res
            .json()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("The selection engine answered unparseably: {e}")))?;
        let masks = v
            .get("masks")
            .and_then(|m| m.as_array())
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "The selection engine answered without masks."))?;
        let iou = v
            .get("iou")
            .and_then(|m| m.as_array())
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "The selection engine answered without IoUs."))?;
        let pngs = masks
            .iter()
            .map(|m| {
                m.as_str()
                    .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
                    .ok_or_else(|| AppError::new(ErrorCode::Internal, "A selection mask came back unparseable."))
            })
            .collect::<AppResult<Vec<_>>>()?;
        let iou: Vec<f32> = iou.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect();
        if pngs.len() != 4 || iou.len() != 4 {
            return Err(AppError::new(
                ErrorCode::Internal,
                "The selection engine returned the wrong number of masks or IoUs.",
            ));
        }
        Ok((pngs, iou))
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

    /// A fake `kokon-sam` door for the relay tests: answers the queued responses one per
    /// connection and records (first line, auth header, body) of every request.
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
                let req_body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                let resp_bytes = resp_body.clone().into_bytes();
                let blen = resp_bytes.len();
                seen2.lock().unwrap().push((line, auth, req_body));
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

    #[test]
    fn relay_sends_the_key_and_parses_the_contract() {
        let masks: Vec<String> = (0u8..4).map(|i| base64::engine::general_purpose::STANDARD.encode([i; 3])).collect();
        let body = serde_json::json!({ "masks": masks, "iou": [0.1, 0.2, 0.3, 0.4] });
        let (gw, seen) = fake_door(vec![(200, r#"{"ready":true}"#.to_string()), (200, body.to_string())]);
        let relay = Relay::new(&gw, "sekret").unwrap();
        relay.capabilities().expect("capabilities");
        let img = b"the image bytes";
        let (pngs, iou) = relay.predict(img, 800, 600, &[(10.0, 20.0, 1)], Some((1.0, 2.0, 3.0, 4.0))).expect("predict");
        assert_eq!(pngs.len(), 4);
        assert_eq!(pngs[0], vec![0u8; 3]);
        assert_eq!(pngs[3], vec![3u8; 3]);
        assert_eq!(iou, vec![0.1, 0.2, 0.3, 0.4]);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, "GET /kokon-sam/segment/capabilities HTTP/1.1");
        assert_eq!(seen[0].1.to_ascii_lowercase(), "bearer sekret");
        assert_eq!(seen[1].0, "POST /kokon-sam/segment/predict HTTP/1.1");
        assert_eq!(seen[1].1.to_ascii_lowercase(), "bearer sekret");
        let v: serde_json::Value = serde_json::from_str(&seen[1].2).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        assert_eq!(b64.decode(v["image"].as_str().unwrap()).unwrap(), img);
        assert_eq!(v["width"], 800);
        assert_eq!(v["points"], serde_json::json!([{ "x": 10.0, "y": 20.0, "label": 1 }]));
        assert_eq!(v["box"], serde_json::json!({ "x0": 1.0, "y0": 2.0, "x1": 3.0, "y1": 4.0 }));
    }

    #[test]
    fn relay_keyless_sends_no_auth_header() {
        let (gw, seen) = fake_door(vec![(200, r#"{"ready":true}"#.to_string())]);
        Relay::new(&gw, "").unwrap().capabilities().expect("capabilities");
        assert_eq!(seen.lock().unwrap()[0].1, "");
    }

    #[test]
    fn relay_maps_failures() {
        let (gw, _) = fake_door(vec![(503, r#"{"error":"down"}"#.to_string()), (500, r#"{"error":"boom"}"#.to_string())]);
        let relay = Relay::new(&gw, "k").unwrap();
        let e = relay.capabilities().unwrap_err();
        assert!(e.to_string().contains("503"));
        let e = relay.encode(b"x", 8, 8).unwrap_err();
        assert!(e.to_string().contains("boom"));
        let (gw, _) = fake_door(vec![(200, r#"{"masks":["QQ=="],"iou":[]}"#.to_string())]);
        let e = Relay::new(&gw, "k").unwrap().predict(b"x", 8, 8, &[], None).unwrap_err();
        assert!(e.to_string().contains("wrong number"));
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
}
