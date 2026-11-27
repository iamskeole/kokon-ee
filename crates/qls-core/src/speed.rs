//! Per-machine speed calibration for time estimates.
//!
//! The planner's time model was measured on one RTX 5070 Ti; other GPUs, engines and memory
//! modes can be 10x off (a 512² image on an M2 Pro took ~2 min against a ~9 s estimate).
//! After every finished image the measured backend time is compared with the uncalibrated
//! prediction, and the ratio (geometric moving average) scales later estimates on the same
//! machine. Stored in `<data>/speed.json`; nothing leaves the PC.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, GpuBackend};
use crate::diagnostics::GpuInfo;

/// Weight of the newest measurement. High enough that one image corrects a wrong prior,
/// low enough that one outlier (a cold SSD, another app on the GPU) does not dominate.
const NEW_WEIGHT: f32 = 0.5;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Calibration {
    /// measured / predicted.
    pub factor: f32,
    pub samples: u32,
}

pub struct SpeedBook {
    path: Option<PathBuf>,
    entries: Mutex<HashMap<String, Calibration>>,
}

impl SpeedBook {
    /// Loads `<data_dir>/speed.json` (missing or unreadable: starts empty).
    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("speed.json");
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            path: Some(path),
            entries: Mutex::new(entries),
        }
    }

    /// Not persisted (tests, private use).
    pub fn in_memory() -> Self {
        Self {
            path: None,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<Calibration> {
        self.entries.lock().unwrap().get(key).copied()
    }

    /// Records one finished job; returns the updated factor. Ignores nonsense values.
    pub fn record(&self, key: &str, measured_s: f32, predicted_s: f32) -> Option<f32> {
        if !(measured_s.is_finite() && predicted_s.is_finite())
            || measured_s <= 0.5
            || predicted_s <= 0.0
        {
            return None;
        }
        let ratio = (measured_s / predicted_s).clamp(0.05, 200.0);
        let mut map = self.entries.lock().unwrap();
        let e = map.entry(key.to_string()).or_insert(Calibration {
            factor: ratio,
            samples: 0,
        });
        if e.samples > 0 {
            e.factor = (e.factor.ln() * (1.0 - NEW_WEIGHT) + ratio.ln() * NEW_WEIGHT).exp();
        }
        e.samples += 1;
        let factor = e.factor;
        if let Some(p) = &self.path {
            if let Ok(json) = serde_json::to_string_pretty(&*map) {
                let _ = std::fs::write(p, json);
            }
        }
        Some(factor)
    }
}

/// What makes timings comparable: backend, GPU and how the weights are held — and the
/// model: different models on the same box must not share one factor (task-043: the
/// qwen-learned factor was applied to H3's plans, and H3's measurements were recorded
/// under it, poisoning both).
pub fn machine_key(cfg: &AppConfig, gpu: Option<&GpuInfo>, model: &str) -> String {
    let gpu = gpu.map(|g| g.name.as_str()).unwrap_or("unknown GPU");
    format!(
        "{}|{}|{:?}|sage={}|model={}",
        cfg.gpu_backend.device_prefix(),
        gpu,
        cfg.text_encoder_placement,
        cfg.uses_sage_attention(),
        model
    )
}

/// Whether the uncalibrated estimate may be shown at all. `trust_uncalibrated` covers
/// the bundled engine (the curve fits only the hardware it was measured on). On a
/// split box an external engine runs on a machine this box cannot see: the local GPU
/// is absent or unknown, so the uncalibrated curve — fitted on yet another box's
/// hardware — is not a measurement. Hide it until the model's own first measured job
/// (task-043: the split box showed ~9 min for H3's 16 MP against ~2 min measured).
pub fn show_uncalibrated(cfg: &AppConfig, gpu: Option<&GpuInfo>) -> bool {
    if !trust_uncalibrated(cfg) {
        return false;
    }
    if cfg.external_engine.is_some() {
        return gpu.is_some();
    }
    true
}

/// Whether the uncalibrated model is close enough to show before the first measurement:
/// only for the hardware it was measured on (NVIDIA through CUDA).
pub fn trust_uncalibrated(cfg: &AppConfig) -> bool {
    cfg.gpu_backend == GpuBackend::Cuda
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_and_smooths() {
        let b = SpeedBook::in_memory();
        assert_eq!(b.get("m"), None);
        // M2 Pro case: ~120 s measured against a 9 s prediction.
        let f = b.record("m", 120.0, 9.0).unwrap();
        assert!(
            (f - 13.33).abs() < 0.1,
            "the first image sets the factor: {f}"
        );
        // An outlier only moves it halfway (geometrically).
        let f2 = b.record("m", 240.0, 9.0).unwrap();
        assert!(f2 > 13.4 && f2 < 26.0, "{f2}");
        assert_eq!(b.get("m").unwrap().samples, 2);
        assert_eq!(b.record("m", f32::NAN, 9.0), None);
        assert_eq!(b.record("m", 0.1, 9.0), None, "too short to be a real job");
    }

    #[test]
    fn persists_and_keys_by_machine() {
        let dir = tempfile::tempdir().unwrap();
        SpeedBook::open(dir.path()).record("k", 30.0, 10.0);
        assert_eq!(
            SpeedBook::open(dir.path()).get("k").map(|c| c.factor),
            Some(3.0)
        );
        let mut cfg = AppConfig::default();
        cfg.gpu_backend = GpuBackend::Cuda;
        let gpu = GpuInfo {
            name: "NVIDIA GeForce RTX 5070".into(),
            ..Default::default()
        };
        let a = machine_key(&cfg, Some(&gpu), "model-a");
        cfg.gpu_backend = GpuBackend::Vulkan;
        assert_ne!(
            a,
            machine_key(&cfg, Some(&gpu), "model-a"),
            "backends are calibrated separately"
        );
        assert_ne!(
            a,
            machine_key(&cfg, Some(&gpu), "model-b"),
            "models are calibrated separately (task-043)"
        );
        cfg.gpu_backend = GpuBackend::Vulkan;
        assert!(!trust_uncalibrated(&cfg));
    }

    #[test]
    fn uncalibrated_hides_for_external_engines_without_a_local_gpu() {
        let mut cfg = AppConfig::default();
        cfg.gpu_backend = GpuBackend::Cuda;
        // Bundled engine: the measured-on-NVIDIA trust stands, even without a detected GPU.
        assert!(show_uncalibrated(&cfg, None));
        // Split box, external engine, no local GPU: hidden until the first measurement.
        cfg.external_engine = Some(crate::config::ExternalEngine::default());
        assert!(!show_uncalibrated(&cfg, None));
        // The engine box reports a GPU through the gateway: shown again.
        let gpu = GpuInfo { name: "some card".into(), ..Default::default() };
        assert!(show_uncalibrated(&cfg, Some(&gpu)));
        // A non-CUDA box was never trusted in the first place.
        let mut vulkan = AppConfig::default();
        vulkan.gpu_backend = GpuBackend::Vulkan;
        assert!(!show_uncalibrated(&vulkan, Some(&gpu)));
    }
}
