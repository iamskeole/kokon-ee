//! The files this service pins: the upscaler models the engine box loads, the lab's models
//! and the AI watermark (the lab ones in `matting.rs`, the watermark in `watermark/`).
//!
//! Every file is pinned by URL, size and SHA-256. If a link moves, the download reports it
//! and asks the user to fetch the file by hand from `page` and to open an issue at [`ISSUES_URL`].

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where users report broken download links. Set before publishing the repository.
pub const ISSUES_URL: &str = "https://github.com/GabrieleLozupone/kokon/issues/new";

/// Folder under the stable-diffusion.cpp folder that holds the Qwen weights.
pub const MODEL_SUBDIR: &str = "models/qwen21";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// stable-diffusion.cpp binaries (a zip extracted into the folder).
    Engine,
    Model,
}

/// The engine release the pins were measured against. The box's engine is a pinned stock
/// build, named in the fresh-box doc; this constant is the reference for file layout.
pub const ENGINE_TAG: &str = "kokon-1013-1814814";
/// File in an engine's folder naming the release it was extracted from.
pub const ENGINE_MARKER: &str = "kokon-engine.txt";

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Download {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: FileKind,
    pub url: &'static str,
    /// Tried in order when `url` fails for good (moved link, refused, changed file).
    pub fallback_urls: &'static [&'static str],
    /// Page where the file can be downloaded by hand when `url` stops working.
    pub page: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    /// Destination relative to the stable-diffusion.cpp folder (for a zip: the archive path,
    /// extracted into its own folder and then deleted).
    pub dest: &'static str,
    /// Files a zip provides (relative to the stable-diffusion.cpp folder); their presence
    /// means it is installed.
    pub provides: &'static [&'static str],
    /// For an engine zip: the release its folder must be marked with ([`ENGINE_MARKER`]).
    pub engine_tag: Option<&'static str>,
}

/// `base` + a catalog path written with `/`, as native components. An absolute path (a model
/// downloaded into a folder of the user's, [`Download::at`]) is used as is.
pub fn join_rel(base: &Path, rel: &str) -> PathBuf {
    if Path::new(rel).is_absolute() {
        return PathBuf::from(rel);
    }
    rel.split('/').filter(|c| !c.is_empty()).fold(base.to_path_buf(), |p, c| p.join(c))
}

impl Download {
    pub fn is_zip(&self) -> bool {
        !self.provides.is_empty()
    }

    pub fn path(&self, sdcpp_dir: &Path) -> PathBuf {
        join_rel(sdcpp_dir, self.dest)
    }

    /// The file name it is saved under.
    pub fn file_name(&self) -> &'static str {
        self.dest.rsplit('/').next().unwrap_or(self.dest)
    }

    /// The same file, saved at `path` (anywhere) instead of the stable-diffusion.cpp folder.
    pub fn at(self, path: &Path) -> Download {
        Download { dest: Box::leak(path.to_string_lossy().into_owned().into_boxed_str()), ..self }
    }

    /// Every URL to try, in order.
    pub fn urls(&self) -> impl Iterator<Item = &'static str> {
        std::iter::once(self.url).chain(self.fallback_urls.iter().copied())
    }

    /// Installed: a zip's files exist (an engine's folder also carries its release marker), or
    /// the file exists with the expected size. The hash is checked after every download; a
    /// size match is enough for files already on disk.
    pub fn is_present(&self, sdcpp_dir: &Path) -> bool {
        if self.is_zip() {
            let files = self.provides.iter().all(|f| join_rel(sdcpp_dir, f).is_file());
            return files && self.engine_tag.is_none_or(|t| self.installed_tag(sdcpp_dir).as_deref() == Some(t));
        }
        std::fs::metadata(self.path(sdcpp_dir)).map(|m| m.is_file() && m.len() == self.size).unwrap_or(false)
    }

    /// The release an engine folder is marked with, if any.
    pub fn installed_tag(&self, sdcpp_dir: &Path) -> Option<String> {
        std::fs::read_to_string(zip_dir(self, sdcpp_dir).join(ENGINE_MARKER)).ok().map(|s| s.trim().to_string())
    }

    /// Marks an extracted engine with its release.
    pub fn write_tag(&self, sdcpp_dir: &Path) -> std::io::Result<()> {
        match self.engine_tag {
            Some(t) => std::fs::write(zip_dir(self, sdcpp_dir).join(ENGINE_MARKER), t),
            None => Ok(()),
        }
    }
}

/// The engine release the pins are checked against (`ENGINE_TAG`); the box's engine itself is a
/// pinned stock build, named in the fresh-box doc — it is not updated from here.
pub fn engine_tag() -> &'static str {
    ENGINE_TAG
}
pub const UPSCALER_NAME: &str = "4xNomos2_hq_mosr";
/// Folder of the upscaler, relative to the stable-diffusion.cpp folder.
pub const UPSCALERS_SUBDIR: &str = "models/upscalers";

/// 4xNomos2_hq_mosr by Philip Hofmann (CC BY 4.0), a MoSR model: enlarges results and runs
/// Upscale. Replaced the ESRGAN models on 2026-10-12: more natural skin, hair and text, and
/// ~8x faster than UltraSharp on an RTX 5070 Ti. Bundled with the app (copied into place at
/// start, see `installer::install_bundled`); downloaded from our mirror, then the author's
/// release, only when that copy is missing.
pub const UPSCALER: Download = Download {
    id: "upscaler",
    label: "4xNomos2 MoSR upscaler",
    kind: FileKind::Model,
    url: "https://github.com/GabrieleLozupone/kokon/releases/download/models-1/4xNomos2_hq_mosr.pth",
    fallback_urls: &["https://github.com/Phhofm/models/releases/download/4xNomos2_hq_mosr/4xNomos2_hq_mosr.pth"],
    page: "https://github.com/Phhofm/models/releases/tag/4xNomos2_hq_mosr",
    size: 17_213_494,
    sha256: "c60dbfc7e6f7d27e03517d1bec3f3cbd16e8cd4288eefd1358952f73f8497ddc",
    dest: "models/upscalers/4xNomos2_hq_mosr.pth",
    provides: &[],
    engine_tag: None,
};

/// Adobe TrustMark, variant P (MIT code and weights): the invisible AI watermark every result
/// gets (watermark.rs; AI Act Art. 50(2), with the XMP of provenance.rs). Bundled with the app
/// like the upscaler (copied into place at start) so marking never waits for a download; Adobe's
/// model host is the fallback. Pinned by size and SHA-256 (downloaded 2026-11-04).
pub const TRUSTMARK_P_ENCODER: Download = Download {
    id: "trustmark_p_encoder",
    label: "TrustMark P encoder (AI watermark)",
    kind: FileKind::Model,
    url: "https://cai-watermark.adobe.net/watermarking/trustmark-models/encoder_P.onnx",
    fallback_urls: &[],
    page: "https://github.com/adobe/trustmark",
    size: 17_312_208,
    sha256: "053441c9c9f05fc158ccba71c610d9d58fcd2c82d1912bf0ffcee988cf2f74c8",
    dest: "models/watermark/encoder_P.onnx",
    provides: &[],
    engine_tag: None,
};

/// The TrustMark P decoder: reads the watermark (skip an image already marked, "Check an image").
pub const TRUSTMARK_P_DECODER: Download = Download {
    id: "trustmark_p_decoder",
    label: "TrustMark P decoder (AI watermark check)",
    kind: FileKind::Model,
    url: "https://cai-watermark.adobe.net/watermarking/trustmark-models/decoder_P.onnx",
    fallback_urls: &[],
    page: "https://github.com/adobe/trustmark",
    size: 47_400_467,
    sha256: "be6d7c33f8a7b376f179e75f3f7c58ff816a9ac7bb6d37fd0a729a635f624c35",
    dest: "models/watermark/decoder_P.onnx",
    provides: &[],
    engine_tag: None,
};

const ESRGAN_MODELS: &str = "https://huggingface.co/uwg/upscaler/tree/main/ESRGAN";

/// 4x_NMKD-Superscale-SP_178000_G by Nmkd (ESRGAN, WTFPL): the default upscaler since
/// 2026-10-12. Sharper and more natural than UltraSharp on most renders; can draw a faint grid
/// on JPEG blocks and dark areas (`AppConfig::upscale_blend` tones it down).
pub const SUPERSCALE: Download = Download {
    id: "upscaler_superscale",
    label: "Superscale-SP upscaler",
    kind: FileKind::Model,
    url: "https://huggingface.co/uwg/upscaler/resolve/main/ESRGAN/4x_NMKD-Superscale-SP_178000_G.pth",
    fallback_urls: &[],
    page: ESRGAN_MODELS,
    size: 66_958_607,
    sha256: "1d1b0078fe71446e0469d8d4df59e96baa80d83cda600d68237d655830821bcc",
    dest: "models/upscalers/4x_NMKD-Superscale-SP_178000_G.pth",
    provides: &[],
    engine_tag: None,
};

/// 4x-UltraSharp by Kim2091 (ESRGAN, CC BY-NC-SA 4.0: non-commercial): the default before
/// 2026-10-12, now an optional download (very clean, but smooths skin into an AI look and can
/// invent lines).
pub const ULTRASHARP: Download = Download {
    id: "upscaler_ultrasharp",
    label: "UltraSharp upscaler",
    kind: FileKind::Model,
    url: "https://huggingface.co/uwg/upscaler/resolve/main/ESRGAN/4x-UltraSharp.pth",
    fallback_urls: &[],
    page: ESRGAN_MODELS,
    size: 66_961_958,
    sha256: "a5812231fc936b42af08a5edba784195495d303d5b3248c24489ef0c4021fe01",
    dest: "models/upscalers/4x-UltraSharp.pth",
    provides: &[],
    engine_tag: None,
};

/// Every upscaler model Kokon can install, in the order Settings lists them.
pub const UPSCALER_DOWNLOADS: &[Download] = &[SUPERSCALE, UPSCALER, ULTRASHARP];

/// The download of the upscaler named `name` (file stem).
pub fn upscaler_download(name: &str) -> Option<Download> {
    UPSCALER_DOWNLOADS.iter().copied().find(|d| d.dest.rsplit('/').next().and_then(|f| f.rsplit_once('.')).is_some_and(|(stem, _)| stem == name))
}

/// Whether the engine box reports `name` among its upscaler models (its
/// `--hires-upscalers-dir`, as its capabilities list it); `None` = the engine has not
/// been queried yet. The engine box is where the file must live; this box's own copy is
/// irrelevant to it (task-038).
pub fn upscaler_on_engine(caps: Option<&crate::sdcpp::Capabilities>, name: &str) -> Option<bool> {
    caps.map(|c| c.upscalers.iter().any(|u| u.model && u.name == name))
}

pub fn zip_dir(d: &Download, sdcpp_dir: &Path) -> PathBuf {
    d.path(sdcpp_dir).parent().map(Path::to_path_buf).unwrap_or_else(|| sdcpp_dir.to_path_buf())
}

/// Message shown when a download keeps failing (moved link, changed file, server down).
pub fn manual_download_hint(d: &Download, sdcpp_dir: &Path) -> String {
    let target = if d.is_zip() { format!("extract it into {}", zip_dir(d, sdcpp_dir).display()) } else { format!("save it as {}", d.path(sdcpp_dir).display()) };
    format!(
        "Download {} by hand from {} and {target}, then press Check again. Please also open an issue at {ISSUES_URL} so the link can be fixed.",
        d.label, d.page
    )
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_checks() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!UPSCALER.is_present(dir.path()));
        std::fs::create_dir_all(dir.path().join(UPSCALERS_SUBDIR)).unwrap();
        std::fs::write(UPSCALER.path(dir.path()), b"short").unwrap();
        assert!(!UPSCALER.is_present(dir.path()), "a partial file is not installed");
        let f = UPSCALER.path(dir.path());
        std::fs::File::create(&f).unwrap().set_len(UPSCALER.size).unwrap();
        assert!(UPSCALER.is_present(dir.path()));
        let hint = manual_download_hint(&UPSCALER, dir.path());
        assert!(hint.contains(UPSCALER.dest) && hint.contains(ISSUES_URL));
    }

    #[test]
    fn upscaler_on_engine_reads_the_engine_caps() {
        use crate::sdcpp::{CapUpscaler, Capabilities};
        let mut caps = Capabilities::default();
        caps.upscalers = vec![
            CapUpscaler { name: "Lanczos".into(), model: false, image_upscale: false },
            CapUpscaler { name: "4xNomos2_hq_mosr".into(), model: true, image_upscale: true },
        ];
        assert_eq!(upscaler_on_engine(None, "4xNomos2_hq_mosr"), None, "the engine has not been queried");
        assert_eq!(upscaler_on_engine(Some(&caps), "4xNomos2_hq_mosr"), Some(true), "the engine box has it");
        assert_eq!(upscaler_on_engine(Some(&caps), "4x-UltraSharp"), Some(false), "the engine box lacks it");
        assert_eq!(upscaler_on_engine(Some(&caps), "Lanczos"), Some(false), "a built-in filter is not an engine-box model");
    }
}
