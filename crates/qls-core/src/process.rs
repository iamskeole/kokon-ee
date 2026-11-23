//! The inference server this service talks to. This build never launches or supervises a
//! process: the engine lives behind the gateway named by `externalEngine`, and the manager
//! adopts whatever compatible engine the gateway answers with (it stays `owned = false`,
//! so no path in this crate can spawn, restart or kill it). The gateway may have to load
//! the model on demand and evict its current tenant first; a single request to the
//! gateway does that, held until the engine is ready.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, ExternalEngine};
use crate::error::{AppError, ErrorCode};
use crate::sdcpp::{Capabilities, SdcppClient};

/// Whether a process id is alive (lease liveness: a client that sent its pid holds its
/// lease while that process runs).
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|st| st.success())
    }
    #[cfg(windows)]
    {
        // OpenProcess on the pid: access-denied still means "exists".
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
    }
}

// ---------------------------------------------------------------------------------------
// Stages the job view reports
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendStage {
    Preparing,
    EncodingImages,
    EncodingPrompt,
    Generating,
    /// Hires fix: decoding the first pass, ESRGAN, re-encoding at the target size.
    Upscaling,
    /// Hires fix: second sampling pass at the target size.
    Refining,
    Decoding,
    Finishing,
}

// ---------------------------------------------------------------------------------------
// Server manager
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerState {
    Stopped,
    Starting,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub state: ServerState,
    /// Always false in this build: the engine is external and nobody here owns it.
    pub owned: bool,
    pub pid: Option<u32>,
    pub port: u16,
    pub model: Option<String>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error: Option<AppError>,
    pub capabilities: Option<Capabilities>,
}

struct Inner {
    state: ServerState,
    owned: bool,
    pid: Option<u32>,
    port: u16,
    model: Option<String>,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    error: Option<AppError>,
    caps: Option<Capabilities>,
}

pub struct ServerManager {
    inner: Mutex<Inner>,
    start_lock: tokio::sync::Mutex<()>,
    data_dir: PathBuf,
}

impl ServerManager {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                state: ServerState::Stopped,
                owned: false,
                pid: None,
                port: 0,
                model: None,
                started_at: None,
                error: None,
                caps: None,
            }),
            start_lock: tokio::sync::Mutex::new(()),
            data_dir,
        })
    }

    pub fn status(&self) -> ServerStatus {
        let g = self.inner.lock().unwrap();
        ServerStatus {
            state: g.state,
            owned: g.owned,
            pid: g.pid,
            port: g.port,
            model: g.model.clone(),
            started_at: g.started_at,
            error: g.error.clone(),
            capabilities: g.caps.clone(),
        }
    }

    pub fn capabilities(&self) -> Option<Capabilities> {
        self.inner.lock().unwrap().caps.clone()
    }

    fn external_caps_path(&self) -> PathBuf {
        self.data_dir.join("external-caps.json")
    }

    fn load_external_caps(&self) -> BTreeMap<String, Capabilities> {
        match std::fs::read_to_string(self.external_caps_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(m) => m,
            None => BTreeMap::new(),
        }
    }

    /// What an external engine can do, last seen, per gateway model id. In external mode
    /// asking the gateway about the engine loads the engine (and evicts its current
    /// tenant), so capabilities are only learned while doing real work — remembered here
    /// so a restarted app can report them again (for instance that the prompt enhancer
    /// exists) without touching the gateway.
    fn remember_external_caps(&self, model_id: &str, caps: &Capabilities) {
        let mut map = self.load_external_caps();
        map.insert(model_id.to_string(), caps.clone());
        if let Ok(json) = serde_json::to_string_pretty(&map) {
            let _ = std::fs::write(self.external_caps_path(), json);
        }
    }

    /// After a restart: what the configured external model reported last time. The engine
    /// is not claimed to be loaded (`state` stays stopped); only its capabilities are known.
    pub fn restore_external_caps(&self, model_id: &str) {
        let Some(caps) = self.load_external_caps().get(model_id).cloned() else {
            return;
        };
        self.set_state(|g| {
            if g.state == ServerState::Stopped {
                g.caps = Some(caps);
            }
        });
    }

    /// A model switch changes which model is configured (without touching the engine): the
    /// remembered capabilities follow the new model, or are dropped if it has never run.
    pub fn switch_external_caps(&self, model_id: &str) {
        let caps = self.load_external_caps().get(model_id).cloned();
        self.set_state(|g| {
            g.caps = caps;
        });
    }

    fn set_state(&self, f: impl FnOnce(&mut Inner)) {
        let mut g = self.inner.lock().unwrap();
        f(&mut g);
    }

    /// Mark the server as gone (e.g. the gateway stopped answering).
    pub fn mark_lost(&self, err: AppError) {
        self.set_state(|g| {
            g.state = ServerState::Error;
            g.error = Some(err);
            g.caps = None;
        });
    }

    /// Ensures a compatible engine is reachable through the gateway. With no
    /// `externalEngine` configured there is nothing to do: this build does not ship an
    /// engine of its own, and the error says so plainly.
    pub async fn ensure_ready(&self, cfg: &AppConfig) -> Result<Capabilities, AppError> {
        let _guard = self.start_lock.lock().await;
        let Some(ext) = cfg.external_engine() else {
            let e = AppError::new(
                ErrorCode::ServerUnavailable,
                "This service runs against an external engine and this box has none configured.",
            )
            .suggest("Point it at a gateway of stock engines: `externalEngine` in the box's config.json (Settings → Model).");
            self.mark_lost(e.clone());
            return Err(e);
        };
        self.ensure_external(cfg, ext).await
    }

    /// The gateway's load can take a while (it may have to evict the current GPU tenant
    /// first); long enough for a cold load of a large model, matching the router's own
    /// health-check budget.
    const EXTERNAL_LOAD_TIMEOUT: Duration = Duration::from_secs(900);

    /// External engine (config `externalEngine`): this service neither launches nor
    /// supervises the inference server. A compatible engine already reachable at the
    /// gateway is adopted as-is; otherwise a single long request to the gateway triggers
    /// the load — the gateway holds the request until the engine is ready, evicting
    /// whatever it must first — and the same adoption happens on the answer. It never
    /// spawns, never opens a second port, and never kills a server running a different
    /// model.
    async fn ensure_external(
        &self,
        cfg: &AppConfig,
        ext: &ExternalEngine,
    ) -> Result<Capabilities, AppError> {
        let client = SdcppClient::new_url(cfg.engine_base_url()).with_api_key(cfg.engine_api_key());
        let compatible =
            |c: &Capabilities| c.model_matches(Path::new(&ext.model_name)) && c.supports_img_gen();
        // Quick probe: if the right engine is already up, the answer comes in a few ms and
        // the gateway is not even asked to load anything.
        if let Ok(caps) = client.capabilities(Duration::from_secs(3)).await {
            if compatible(&caps) {
                self.adopt_external(&caps, cfg);
                return Ok(caps);
            }
        }
        // The gateway holds a different model (or none) and is now loading ours: make the
        // switch visible instead of pretending the old one is still the right one.
        self.set_state(|g| {
            g.state = ServerState::Starting;
            g.owned = false;
            g.model = Some(ext.model_name.clone());
            g.error = None;
        });
        match client.capabilities(Self::EXTERNAL_LOAD_TIMEOUT).await {
            Ok(caps) if compatible(&caps) => {
                self.adopt_external(&caps, cfg);
                Ok(caps)
            }
            Ok(caps) => {
                let e = AppError::new(
                    ErrorCode::ServerUnavailable,
                    format!(
                        "The engine at {} is running a different model (\"{}\") than the requested \"{}\".",
                        cfg.engine_base_url(),
                        caps.model.name,
                        ext.model_name
                    ),
                );
                self.mark_lost(e.clone());
                Err(e)
            }
            Err(e) => {
                self.mark_lost(e.clone());
                Err(e)
            }
        }
    }

    /// Registers a reachable external engine as the service's server without owning it.
    fn adopt_external(&self, caps: &Capabilities, cfg: &AppConfig) {
        self.set_state(|g| {
            g.state = ServerState::Ready;
            g.owned = false;
            g.pid = None;
            g.port = cfg.server_port;
            g.model = Some(caps.model.name.clone());
            g.error = None;
            g.caps = Some(caps.clone());
            if g.started_at.is_none() {
                g.started_at = Some(chrono::Utc::now());
            }
        });
        if let Some(ext) = cfg.external_engine() {
            self.remember_external_caps(&ext.model, caps);
        }
        tracing::info!(
            "external engine ready at {} (model \"{}\"; not owned)",
            cfg.engine_base_url(),
            caps.model.name
        );
    }

    /// Clears the server's state. There is no process to stop here — the engine is
    /// external — so this only brings the status back to a stopped report.
    pub async fn stop(&self) {
        let _guard = self.start_lock.lock().await;
        self.set_state(|g| {
            g.state = ServerState::Stopped;
            g.owned = false;
            g.pid = None;
            g.error = None;
            g.caps = None;
            g.started_at = None;
        });
    }
}

// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake capabilities server: answers `/sdcpp/v1/capabilities` on each connection,
    /// remembering the requests.
    async fn caps_server(
        name: &str,
        conns: usize,
    ) -> (u16, tokio::sync::oneshot::Receiver<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let body = serde_json::json!({ "model": { "name": name }, "supported_modes": ["img_gen"], "text_generation": true })
            .to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let mut seen = Vec::new();
            for _ in 0..conns {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let mut req = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    let n = sock.read(&mut buf).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    req.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&req).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if req.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                seen.push(String::from_utf8_lossy(&req).into_owned());
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                if sock.write_all(head.as_bytes()).await.is_err() {
                    break;
                }
                let _ = sock.flush().await;
            }
            let _ = tx.send(seen);
        });
        (port, rx)
    }

    fn external_cfg(gateway: String, model_name: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.external_engine = Some(ExternalEngine {
            gateway,
            api_key: String::new(),
            model: "model-a".into(),
            model_name: model_name.into(),
            sample: serde_json::json!({}),
            vlm: None,
            vlm_vision: None,
            prompt_bundle: String::new(),
        });
        cfg
    }

    #[tokio::test]
    async fn no_engine_configured_is_a_plain_error() {
        let dir = tempfile::tempdir().unwrap();
        let m = ServerManager::new(dir.path().to_path_buf());
        let e = m.ensure_ready(&AppConfig::default()).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert!(e.message.contains("external engine"), "{}", e.message);
        assert_eq!(m.status().state, ServerState::Error);
    }

    #[tokio::test]
    async fn external_engine_already_running_is_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let m = ServerManager::new(dir.path().to_path_buf());
        let (port, _) = caps_server("qwen-image-2.1-turbo-Q4_K_M.gguf", 1).await;
        let cfg = external_cfg(
            format!("http://127.0.0.1:{port}"),
            "qwen-image-2.1-turbo-Q4_K_M.gguf",
        );
        let caps = m.ensure_ready(&cfg).await.unwrap();
        let st = m.status();
        assert_eq!(st.state, ServerState::Ready);
        assert!(!st.owned, "an external engine is never owned");
        assert_eq!(
            st.model.as_deref(),
            Some("qwen-image-2.1-turbo-Q4_K_M.gguf")
        );
        assert_eq!(caps.model.name, "qwen-image-2.1-turbo-Q4_K_M.gguf");
        // stop() must not touch it either
        m.stop().await;
        assert_eq!(m.status().state, ServerState::Stopped);
    }

    #[tokio::test]
    async fn external_engine_wrong_model_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let m = ServerManager::new(dir.path().to_path_buf());
        let (port, reqs) = caps_server("some-other-model.gguf", 2).await;
        let cfg = external_cfg(
            format!("http://127.0.0.1:{port}"),
            "qwen-image-2.1-turbo-Q4_K_M.gguf",
        );
        let e = m.ensure_ready(&cfg).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert!(
            e.message.contains("some-other-model.gguf"),
            "the reply names what is actually loaded: {}",
            e.message
        );
        assert_eq!(m.status().state, ServerState::Error);
        // quick probe + long ensure, both on the gateway — nothing was spawned in between
        let seen = reqs.await.unwrap();
        assert_eq!(seen.len(), 2);
        for r in &seen {
            assert!(
                r.contains("/model-a/sdcpp/v1/capabilities"),
                "every probe goes through the gateway: {}",
                r
            );
        }
    }

    #[tokio::test]
    async fn external_engine_caps_are_remembered_and_restored() {
        let dir = tempfile::tempdir().unwrap();
        let (port, reqs) = caps_server("qwen-image-2.1-turbo-Q4_K_M.gguf", 1).await;
        let cfg = external_cfg(
            format!("http://127.0.0.1:{port}"),
            "qwen-image-2.1-turbo-Q4_K_M.gguf",
        );
        let m = ServerManager::new(dir.path().to_path_buf());
        m.ensure_ready(&cfg).await.unwrap();
        assert!(m.status().capabilities.unwrap().text_generation);
        assert!(
            m.external_caps_path().exists(),
            "adopting an external engine remembers its capabilities"
        );

        // A restarted app (a fresh manager on the same data folder) reports them again
        // without asking the gateway anything.
        let m2 = ServerManager::new(dir.path().to_path_buf());
        m2.restore_external_caps("model-a");
        let st = m2.status();
        assert_eq!(
            st.state,
            ServerState::Stopped,
            "restored caps do not claim the engine is loaded"
        );
        assert!(st.capabilities.as_ref().unwrap().text_generation);
        let seen = reqs.await.unwrap();
        assert_eq!(seen.len(), 1, "the restore does not touch the gateway");
    }

    #[tokio::test]
    async fn external_engine_caps_follow_a_model_switch() {
        let dir = tempfile::tempdir().unwrap();
        let (port, _) = caps_server("qwen-image-2.1-turbo-Q4_K_M.gguf", 1).await;
        let cfg = external_cfg(
            format!("http://127.0.0.1:{port}"),
            "qwen-image-2.1-turbo-Q4_K_M.gguf",
        );
        let m = ServerManager::new(dir.path().to_path_buf());
        m.ensure_ready(&cfg).await.unwrap();
        assert!(m.status().capabilities.unwrap().text_generation);

        // The newly configured model has never run: nothing remembered about it.
        m.switch_external_caps("model-b");
        assert!(m.status().capabilities.is_none());
        // Switching back: the remembered values come back.
        m.switch_external_caps("model-a");
        assert!(m.status().capabilities.as_ref().unwrap().text_generation);
    }

    /// A fake capabilities server that answers each connection only after `delay`:
    /// the 3s quick probe times out, the long ensure waits it out.
    async fn slow_caps_server(name: &str, delay: Duration, conns: usize) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let body = serde_json::json!({ "model": { "name": name }, "supported_modes": ["img_gen"], "text_generation": true })
            .to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            for _ in 0..conns {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let mut req = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            req.extend_from_slice(&buf[..n]);
                            let text = String::from_utf8_lossy(&req).to_string();
                            if let Some(end) = text.find("\r\n\r\n") {
                                let len = text
                                    .lines()
                                    .find_map(|l| {
                                        l.to_ascii_lowercase()
                                            .strip_prefix("content-length:")
                                            .map(|v| v.trim().parse::<usize>().unwrap())
                                    })
                                    .unwrap_or(0);
                                if req.len() >= end + 4 + len {
                                    break;
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(delay).await; // a load in flight at the gateway
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                if sock.write_all(head.as_bytes()).await.is_err() {
                    break;
                }
                let _ = sock.flush().await;
            }
        });
        port
    }

    #[tokio::test]
    async fn external_engine_load_is_visible_as_starting() {
        let dir = tempfile::tempdir().unwrap();
        let m = ServerManager::new(dir.path().to_path_buf());
        let name = "qwen-image-2.1-turbo-Q4_K_M.gguf";
        let port = slow_caps_server(name, Duration::from_millis(3500), 2).await;
        let cfg = external_cfg(format!("http://127.0.0.1:{port}"), name);
        let mgr = m.clone();
        let handle = tokio::spawn(async move { mgr.ensure_ready(&cfg).await });
        // The quick probe (3s) has timed out; the long ensure is waiting on the "load".
        tokio::time::sleep(Duration::from_millis(3600)).await;
        let st = m.status();
        assert_eq!(
            st.state,
            ServerState::Starting,
            "an in-flight gateway load shows as starting"
        );
        assert_eq!(
            st.model.as_deref(),
            Some(name),
            "... and names the model being loaded"
        );
        assert!(!st.owned);
        let caps = handle.await.unwrap().unwrap();
        assert_eq!(caps.model.name, name);
        let st = m.status();
        assert_eq!(st.state, ServerState::Ready);
        assert!(!st.owned);
    }

    #[tokio::test]
    async fn external_engine_down_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let m = ServerManager::new(dir.path().to_path_buf());
        // A port nothing listens on (a live bind/release leaves it free right after).
        let dead = std::net::TcpListener::bind(("127.0.0.1", 0))
            .and_then(|l| l.local_addr())
            .unwrap()
            .port();
        let cfg = external_cfg(
            format!("http://127.0.0.1:{dead}"),
            "qwen-image-2.1-turbo-Q4_K_M.gguf",
        );
        let e = m.ensure_ready(&cfg).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert_eq!(m.status().state, ServerState::Error);
    }
}
