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
        let (m_shape, m_data) = outputs[i_masks].try_extract_tensor::<f32>().map_err(ort_err)?;
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
}
