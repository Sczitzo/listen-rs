//! Local model catalog, on-demand update checking, and in-app installer for
//! Ollama AI cleanup models and optional whisper.cpp speech-to-text models.

use crate::cleanup;
use crate::formatting;
use crate::storage::{self, Settings, SpeechChoice, Style};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const PARAKEET_ID: &str = "parakeet-unified-en-0.6b";
const WHISPER_FALLBACK_TAG: &str = "b5907";
const WHISPER_ASSET_NAME: &str = "whisper-bin-x64.zip";
const MAX_RUNTIME_ZIP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RUNTIME_UNCOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RUNTIME_ARCHIVE_ENTRIES: usize = 64;
const MAX_SPEECH_MODEL_BYTES: u64 = 1500 * 1024 * 1024;
const MAX_REDIRECT_HOPS: usize = 6;
const OLLAMA_PULL_STALL_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Copy)]
pub struct SpeechCatalogEntry {
    pub id: &'static str,
    pub filename: &'static str,
    pub name: &'static str,
    pub tag: &'static str,
    pub size_mb: u32,
    pub ram_note: &'static str,
    pub note: &'static str,
}

#[derive(Clone, Copy)]
pub struct CleanupCatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub tier: &'static str,
    pub size_gb: &'static str,
    pub vram_note: &'static str,
    pub note: &'static str,
}

pub const SPEECH_CATALOG: &[SpeechCatalogEntry] = &[
    SpeechCatalogEntry {
        id: PARAKEET_ID,
        filename: "",
        name: "Parakeet Unified English 0.6B",
        tag: "Built-in · Fastest English",
        size_mb: 640,
        ram_note: "~0.7 GB RAM · 0 GB VRAM (CPU)",
        note: "English only. Fast and leaves your GPU completely free.",
    },
    SpeechCatalogEntry {
        id: "whisper:ggml-large-v3-turbo-q5_0.bin",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        name: "Whisper Large v3 Turbo (Q5_0)",
        tag: "Best accuracy · Multilingual",
        size_mb: 574,
        ram_note: "~0.8 GB RAM · 0 GB VRAM (CPU)",
        note: "Near Large-v3 accuracy with vocabulary hints and multilingual support.",
    },
    SpeechCatalogEntry {
        id: "whisper:ggml-medium.en-q5_0.bin",
        filename: "ggml-medium.en-q5_0.bin",
        name: "Whisper Medium English (Q5_0)",
        tag: "Balanced · English",
        size_mb: 539,
        ram_note: "~0.7 GB RAM · 0 GB VRAM (CPU)",
        note: "Strong English-only Whisper model with vocabulary hints.",
    },
    SpeechCatalogEntry {
        id: "whisper:ggml-small.en-q5_1.bin",
        filename: "ggml-small.en-q5_1.bin",
        name: "Whisper Small English (Q5_1)",
        tag: "Fast · Light",
        size_mb: 190,
        ram_note: "~0.3 GB RAM · 0 GB VRAM (CPU)",
        note: "Lightweight English Whisper model with vocabulary hints.",
    },
];

pub const CLEANUP_CATALOG: &[CleanupCatalogEntry] = &[
    CleanupCatalogEntry {
        id: "qwen3.5:2b",
        name: "Qwen 3.5 2B",
        tier: "Light · ~2.2 GB VRAM",
        size_gb: "2.0 GB",
        vram_note: "~2.2 GB VRAM",
        note: "Fastest formatting with minimal GPU memory footprint.",
    },
    CleanupCatalogEntry {
        id: "qwen3.5:4b",
        name: "Qwen 3.5 4B",
        tier: "Balanced · ~3.5 GB VRAM",
        size_gb: "3.4 GB",
        vram_note: "~3.5 GB VRAM",
        note: "Balanced everyday formatting while leaving plenty of GPU headroom.",
    },
    CleanupCatalogEntry {
        id: "gemma3:4b",
        name: "Gemma 3 4B",
        tier: "Balanced · ~3.5 GB VRAM",
        size_gb: "3.3 GB",
        vram_note: "~3.5 GB VRAM",
        note: "Alternative 4B prose editor sized for 8 GB GPUs.",
    },
    CleanupCatalogEntry {
        id: "qwen2.5:7b",
        name: "Qwen 2.5 7B",
        tier: "Medium · ~5.0 GB VRAM",
        size_gb: "4.7 GB",
        vram_note: "~5.0 GB VRAM",
        note: "Classic steady formatting baseline.",
    },
    CleanupCatalogEntry {
        id: "qwen3.5:9b",
        name: "Qwen 3.5 9B",
        tier: "Heavy · ~6.5 GB VRAM",
        size_gb: "6.6 GB",
        vram_note: "~6.5 GB VRAM",
        note: "Larger model; uses most of an 8 GB GPU.",
    },
];

pub fn speech_models_dir() -> PathBuf {
    storage::data_dir().join("models").join("speech")
}

pub fn whisper_runtime_dir() -> PathBuf {
    storage::data_dir().join("runtimes").join("whisper-cpp")
}

pub fn find_speech_entry(id: &str) -> Option<&'static SpeechCatalogEntry> {
    SPEECH_CATALOG.iter().find(|entry| entry.id == id)
}

pub fn find_cleanup_entry(id: &str) -> Option<&'static CleanupCatalogEntry> {
    CLEANUP_CATALOG.iter().find(|entry| entry.id == id)
}

pub fn find_whisper_cli_in(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join("whisper-cli.exe");
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let nested = path.join("whisper-cli.exe");
            if nested.is_file() {
                return Some(nested);
            }
            if let Ok(sub) = std::fs::read_dir(&path) {
                for sub_entry in sub.flatten() {
                    let candidate = sub_entry.path().join("whisper-cli.exe");
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

pub fn find_whisper_cli(settings: &Settings) -> Option<PathBuf> {
    if let Some(managed) = find_whisper_cli_in(&whisper_runtime_dir()) {
        return Some(managed);
    }
    if !settings.whisper_cli.is_empty() {
        let configured = PathBuf::from(&settings.whisper_cli);
        if configured.is_file() {
            return Some(configured);
        }
    }
    None
}

pub fn managed_whisper_model_path(entry: &SpeechCatalogEntry) -> Option<PathBuf> {
    if entry.filename.is_empty() {
        return None;
    }
    Some(speech_models_dir().join(entry.filename))
}

fn is_installed_whisper_file(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.len() >= 1_000_000)
}

pub fn installed_managed_whisper_models(dir: &Path) -> Vec<&'static SpeechCatalogEntry> {
    SPEECH_CATALOG
        .iter()
        .filter(|entry| {
            !entry.filename.is_empty() && is_installed_whisper_file(&dir.join(entry.filename))
        })
        .collect()
}

pub fn active_speech_id(settings: &Settings) -> String {
    if settings.speech_engine == SpeechChoice::WhisperCli {
        let file = Path::new(&settings.whisper_model)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if let Some(entry) = SPEECH_CATALOG
            .iter()
            .find(|e| !e.filename.is_empty() && e.filename.eq_ignore_ascii_case(file))
        {
            return entry.id.to_owned();
        }
        return "whisper-cpp".into();
    }
    PARAKEET_ID.into()
}

pub fn parakeet_installed(settings: &Settings) -> bool {
    let dir = storage::effective_transcribe_dir(&settings.transcribe_dir);
    dir.join("transcribe.dll").is_file()
        && storage::effective_speech_model(&settings.speech_model).is_file()
}

#[derive(Clone, Default)]
pub struct InstallState {
    pub run_id: u64,
    pub active: bool,
    pub kind: String,
    pub id: String,
    pub label: String,
    pub status: String,
    pub downloaded: u64,
    pub total: u64,
    pub percent: Option<u8>,
    pub done: bool,
    pub error: String,
}

impl InstallState {
    pub fn to_json(&self) -> Value {
        json!({
            "run_id": self.run_id,
            "active": self.active,
            "kind": self.kind,
            "id": self.id,
            "label": self.label,
            "status": self.status,
            "downloaded": self.downloaded,
            "total": self.total,
            "percent": self.percent,
            "done": self.done,
            "error": self.error,
        })
    }
}

fn lock_unpoisoned<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct InstallManager {
    state: Mutex<InstallState>,
    cancel: AtomicBool,
    latest_whisper_tag: Mutex<Option<String>>,
}

impl InstallManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(InstallState::default()),
            cancel: AtomicBool::new(false),
            latest_whisper_tag: Mutex::new(None),
        }
    }

    pub fn status(&self) -> Value {
        lock_unpoisoned(&self.state).to_json()
    }

    pub fn is_active(&self) -> bool {
        lock_unpoisoned(&self.state).active
    }

    pub fn cancel(&self) -> bool {
        self.cancel.store(true, Ordering::Relaxed);
        let mut st = lock_unpoisoned(&self.state);
        if st.active {
            st.status = "Canceling…".into();
            return true;
        }
        false
    }

    pub fn set_latest_whisper_tag(&self, tag: String) {
        *lock_unpoisoned(&self.latest_whisper_tag) = Some(tag);
    }

    pub fn whisper_tag(&self) -> String {
        lock_unpoisoned(&self.latest_whisper_tag)
            .clone()
            .unwrap_or_else(|| WHISPER_FALLBACK_TAG.into())
    }

    pub fn start(
        self: &Arc<Self>,
        kind: &str,
        id: &str,
        ollama_url: String,
        preview: bool,
    ) -> Value {
        if preview {
            return json!({"ok": false, "message": "Downloads are disabled in sample mode."});
        }
        let label = match kind {
            "cleanup" => match find_cleanup_entry(id) {
                Some(entry) => entry.name.to_owned(),
                None => return json!({"ok": false, "message": "Unknown formatting model."}),
            },
            "speech" => match find_speech_entry(id) {
                Some(entry) if !entry.filename.is_empty() => entry.name.to_owned(),
                _ => {
                    return json!({"ok": false, "message": "That speech model cannot be downloaded."})
                }
            },
            _ => return json!({"ok": false, "message": "Unknown model type."}),
        };

        let run_id = {
            let mut st = lock_unpoisoned(&self.state);
            if st.active {
                return json!({"ok": false, "message": format!("Already installing {}. Wait or cancel it first.", st.label)});
            }
            let next_id = st.run_id.wrapping_add(1).max(1);
            self.cancel.store(false, Ordering::Relaxed);
            *st = InstallState {
                run_id: next_id,
                active: true,
                kind: kind.to_owned(),
                id: id.to_owned(),
                label: label.clone(),
                status: format!("Starting {label}…"),
                downloaded: 0,
                total: 0,
                percent: None,
                done: false,
                error: String::new(),
            };
            next_id
        };

        let mgr = Arc::clone(self);
        let kind_owned = kind.to_owned();
        let id_owned = id.to_owned();
        let spawned = std::thread::Builder::new()
            .name("model-install".into())
            .spawn(move || {
                let result = if kind_owned == "cleanup" {
                    mgr.run_ollama_pull(&ollama_url, &id_owned)
                } else {
                    mgr.run_speech_install(&id_owned)
                };
                let mut st = lock_unpoisoned(&mgr.state);
                st.active = false;
                match result {
                    Ok(msg) => {
                        st.done = true;
                        st.percent = Some(100);
                        st.status = msg;
                        st.error.clear();
                    }
                    Err(err) => {
                        st.done = false;
                        st.status = err.clone();
                        st.error = err;
                    }
                }
            });
        if spawned.is_err() {
            let mut st = lock_unpoisoned(&self.state);
            st.active = false;
            st.error = "Could not start the download worker.".into();
            return json!({"ok": false, "message": st.error});
        }
        json!({"ok": true, "run_id": run_id, "message": format!("Downloading {label}…")})
    }

    fn update_progress(&self, status: impl Into<String>, downloaded: u64, total: u64) {
        if self.cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut st = lock_unpoisoned(&self.state);
        st.status = status.into();
        st.downloaded = downloaded;
        st.total = total;
        st.percent = (total > 0).then(|| ((downloaded.saturating_mul(100)) / total).min(100) as u8);
    }

    fn run_ollama_pull(&self, ollama_url: &str, model: &str) -> Result<String, String> {
        let base = match ollama_url.trim().trim_end_matches('/') {
            "http://127.0.0.1:11434" | "http://localhost:11434" => "http://127.0.0.1:11434",
            _ => return Err("Ollama must run locally at http://127.0.0.1:11434".into()),
        };
        self.update_progress(format!("Connecting to Ollama to pull {model}…"), 0, 0);
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .try_proxy_from_env(false)
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(3))
            .build();
        let resp = agent
            .post(&format!("{base}/api/pull"))
            .send_json(json!({"model": model, "stream": true}))
            .map_err(|err| match err {
                ureq::Error::Status(code, r) => {
                    let body = r.into_string().unwrap_or_default();
                    let detail = body.trim();
                    if detail.is_empty() {
                        format!("Ollama returned HTTP {code}.")
                    } else {
                        format!("Ollama returned HTTP {code}: {}", detail.chars().take(160).collect::<String>())
                    }
                }
                ureq::Error::Transport(_) => "Could not reach Ollama. Start Ollama and try again.".into(),
            })?;
        let mut reader = BufReader::new(resp.into_reader());
        let mut line = String::new();
        let mut succeeded = false;
        let mut last_activity = Instant::now();
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err("Download canceled.".into());
            }
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    last_activity = Instant::now();
                }
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                    if self.cancel.load(Ordering::Relaxed) {
                        return Err("Download canceled.".into());
                    }
                    if last_activity.elapsed() > OLLAMA_PULL_STALL_TIMEOUT {
                        return Err("Ollama download stalled for too long.".into());
                    }
                    continue;
                }
                Err(e) => return Err(format!("Ollama download interrupted: {e}")),
            }
            let Ok(chunk) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if let Some(err) = chunk["error"].as_str() {
                return Err(format!("Ollama error: {err}"));
            }
            let status = chunk["status"].as_str().unwrap_or("Downloading…");
            let total = chunk["total"].as_u64().unwrap_or(0);
            let completed = chunk["completed"].as_u64().unwrap_or(0);
            if status == "success" {
                succeeded = true;
            }
            let pretty = if status.starts_with("pulling ") && total > 0 {
                format!("Downloading {model}…")
            } else if status == "verifying sha256 digest" {
                format!("Verifying {model}…")
            } else {
                status.to_owned()
            };
            self.update_progress(pretty, completed, total);
        }
        if succeeded {
            Ok(format!("Installed {model}. Click Use to switch to it."))
        } else {
            Err("Ollama closed the connection before finishing the install.".into())
        }
    }

    fn run_speech_install(&self, id: &str) -> Result<String, String> {
        let entry = find_speech_entry(id).ok_or_else(|| "Unknown speech model.".to_string())?;
        let runtime_dir = whisper_runtime_dir();
        if let Some(parent) = runtime_dir.parent() {
            sweep_stale_artifacts(parent);
        }
        if find_whisper_cli_in(&runtime_dir).is_none() {
            self.install_whisper_runtime(&runtime_dir)?;
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err("Download canceled.".into());
        }
        let models_dir = speech_models_dir();
        std::fs::create_dir_all(&models_dir)
            .map_err(|e| format!("Could not create models folder: {e}"))?;
        sweep_stale_artifacts(&models_dir);
        let dest = models_dir.join(entry.filename);
        let url = format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            entry.filename
        );
        self.download_https_file(
            &url,
            &dest,
            &format!("Downloading {}…", entry.name),
            MAX_SPEECH_MODEL_BYTES,
        )?;
        if !is_installed_whisper_file(&dest) {
            let _ = std::fs::remove_file(&dest);
            return Err("Downloaded speech model file was incomplete.".into());
        }
        Ok(format!("Installed {}. Click Use to switch to it.", entry.name))
    }

    fn install_whisper_runtime(&self, runtime_dir: &Path) -> Result<(), String> {
        let tag = self.whisper_tag();
        if !is_safe_release_tag(&tag) {
            return Err("Invalid whisper.cpp release tag.".into());
        }
        let parent = runtime_dir
            .parent()
            .ok_or_else(|| "Invalid runtime directory.".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create runtime folder: {e}"))?;
        let zip_path = parent.join(format!("whisper-runtime-{}.zip", std::process::id()));
        let url = format!(
            "https://github.com/ggml-org/whisper.cpp/releases/download/{tag}/{WHISPER_ASSET_NAME}"
        );
        self.download_https_file(
            &url,
            &zip_path,
            &format!("Downloading whisper.cpp runtime ({tag})…"),
            MAX_RUNTIME_ZIP_BYTES,
        )?;
        let extract_result = self.extract_whisper_zip(&zip_path, runtime_dir);
        let _ = std::fs::remove_file(&zip_path);
        extract_result
    }

    fn extract_whisper_zip(&self, zip_path: &Path, runtime_dir: &Path) -> Result<(), String> {
        self.update_progress("Inspecting whisper.cpp archive…", 0, 0);
        let listing = run_system_tar(&["-tvf", &zip_path.to_string_lossy()])
            .map_err(|e| format!("Could not inspect runtime archive: {e}"))?;
        validate_tar_verbose_listing(&listing)?;

        let staging = runtime_dir.with_extension(format!("staging-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)
            .map_err(|e| format!("Could not create staging folder: {e}"))?;
        self.update_progress("Extracting whisper.cpp runtime…", 0, 0);
        let extracted = run_system_tar(&[
            "-xf",
            &zip_path.to_string_lossy(),
            "-C",
            &staging.to_string_lossy(),
        ]);
        if let Err(err) = extracted {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("Could not extract whisper.cpp runtime: {err}"));
        }
        if verify_extracted_tree(&staging, MAX_RUNTIME_UNCOMPRESSED_BYTES).is_err()
            || find_whisper_cli_in(&staging).is_none()
        {
            let _ = std::fs::remove_dir_all(&staging);
            return Err("Runtime archive failed post-extraction safety verification.".into());
        }
        if runtime_dir.exists() {
            let _ = std::fs::remove_dir_all(runtime_dir);
        }
        std::fs::rename(&staging, runtime_dir).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            format!("Could not finish installing whisper.cpp runtime: {e}")
        })?;
        Ok(())
    }

    fn download_https_file(
        &self,
        initial_url: &str,
        dest: &Path,
        label: &str,
        max_bytes: u64,
    ) -> Result<(), String> {
        self.update_progress(label, 0, 0);
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(5))
            .build();

        let mut current_url = initial_url.to_owned();
        let resp = {
            let mut final_resp = None;
            for _ in 0..=MAX_REDIRECT_HOPS {
                if self.cancel.load(Ordering::Relaxed) {
                    return Err("Download canceled.".into());
                }
                if !is_allowed_download_url(&current_url) {
                    return Err("Download URL was rejected by the allowlist.".into());
                }
                let r = agent
                    .get(&current_url)
                    .set("User-Agent", "listen-rs")
                    .call()
                    .map_err(|e| format!("Download failed: {e}"))?;
                if matches!(r.status(), 301 | 302 | 303 | 307 | 308) {
                    let loc = r
                        .header("Location")
                        .ok_or_else(|| "Redirect response missing Location header.".to_string())?;
                    current_url = resolve_redirect_url(&current_url, loc)
                        .ok_or_else(|| "Invalid redirect Location header.".to_string())?;
                    continue;
                }
                final_resp = Some(r);
                break;
            }
            final_resp.ok_or_else(|| "Too many redirects while downloading file.".to_string())?
        };

        let total = resp
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        if total > max_bytes {
            return Err("File size exceeds safety limit.".into());
        }

        let partial = dest.with_extension(format!("{}.partial", std::process::id()));
        let write_result = (|| -> Result<(), String> {
            let mut out = std::fs::File::create(&partial)
                .map_err(|e| format!("Could not write temporary file: {e}"))?;
            let mut reader = resp.into_reader();
            let mut buf = [0u8; 64 * 1024];
            let mut downloaded = 0u64;
            let mut last_activity = Instant::now();
            loop {
                if self.cancel.load(Ordering::Relaxed) {
                    return Err("Download canceled.".into());
                }
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        last_activity = Instant::now();
                        n
                    }
                    Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                        if self.cancel.load(Ordering::Relaxed) {
                            return Err("Download canceled.".into());
                        }
                        if last_activity.elapsed() > Duration::from_secs(45) {
                            return Err("Download timed out.".into());
                        }
                        continue;
                    }
                    Err(e) => return Err(format!("Download interrupted: {e}")),
                };
                downloaded = downloaded.saturating_add(n as u64);
                if downloaded > max_bytes {
                    return Err("Downloaded file exceeded safety limit.".into());
                }
                out.write_all(&buf[..n])
                    .map_err(|e| format!("Could not save downloaded bytes: {e}"))?;
                self.update_progress(label, downloaded, total);
            }
            out.flush()
                .map_err(|e| format!("Could not flush download: {e}"))?;
            if total > 0 && downloaded != total {
                return Err("Download ended early before all bytes arrived.".into());
            }
            Ok(())
        })();

        if let Err(err) = write_result {
            let _ = std::fs::remove_file(&partial);
            return Err(err);
        }
        if dest.exists() {
            let _ = std::fs::remove_file(dest);
        }
        std::fs::rename(&partial, dest).map_err(|e| {
            let _ = std::fs::remove_file(&partial);
            format!("Could not finalize downloaded file: {e}")
        })?;
        Ok(())
    }
}

fn sweep_stale_artifacts(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".partial") || (name.starts_with("whisper-runtime-") && name.ends_with(".zip")) {
            let _ = std::fs::remove_file(entry.path());
        } else if name.contains(".staging-") && entry.path().is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn system_tar_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let path = PathBuf::from(root).join("System32").join("tar.exe");
            if path.is_file() {
                return path;
            }
        }
        PathBuf::from(r"C:\Windows\System32\tar.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("tar")
    }
}

fn run_system_tar(args: &[&str]) -> Result<String, String> {
    let mut cmd = std::process::Command::new(system_tar_path());
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn validate_tar_verbose_listing(listing: &str) -> Result<(), String> {
    let mut count = 0usize;
    let mut total_size = 0u64;
    for raw_line in listing.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        count += 1;
        if count > MAX_RUNTIME_ARCHIVE_ENTRIES {
            return Err("Archive contains too many entries.".into());
        }
        let first = line.as_bytes()[0];
        if first != b'-' && first != b'd' {
            return Err("Archive contains a disallowed entry type (symlink/special file).".into());
        }
        // bsdtar -tvf format: `<mode> <links> <owner> <group> <size> <mon> <day> <time/year> <name>`
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            return Err("Unrecognized archive listing line.".into());
        }
        let size = parts[4]
            .parse::<u64>()
            .map_err(|_| "Unrecognized archive entry size.".to_string())?;
        total_size = total_size.saturating_add(size);
        if total_size > MAX_RUNTIME_UNCOMPRESSED_BYTES {
            return Err("Archive uncompressed size exceeds safety limit.".into());
        }
        let name = parts[8..].join(" ");
        if name.contains("->") || !is_safe_archive_entry(&name) {
            return Err("Archive contains an unsafe path.".into());
        }
    }
    if count == 0 {
        return Err("Archive is empty.".into());
    }
    Ok(())
}

fn verify_extracted_tree(root: &Path, max_bytes: u64) -> Result<(), String> {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let meta = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if meta.file_type().is_symlink() {
                return Err("Extracted archive contained a symlink.".into());
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
                if total > max_bytes {
                    return Err("Extracted archive exceeded size cap.".into());
                }
            }
        }
    }
    Ok(())
}

pub fn is_safe_release_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 32
        && tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

pub fn is_safe_archive_entry(entry: &str) -> bool {
    let clean = entry.trim();
    if clean.is_empty()
        || clean.starts_with('/')
        || clean.starts_with('\\')
        || clean.contains(':')
        || clean.contains('\0')
    {
        return false;
    }
    !clean.split(['/', '\\']).any(|part| part == "..")
}

pub fn resolve_redirect_url(current_url: &str, location: &str) -> Option<String> {
    let loc = location.trim();
    if loc.is_empty() || loc.bytes().any(|b| b.is_ascii_control() || b.is_ascii_whitespace()) {
        return None;
    }
    if loc.starts_with("https://") || loc.starts_with("http://") {
        return Some(loc.to_owned());
    }
    if loc.starts_with('/') && !loc.starts_with("//") {
        let rest = current_url.strip_prefix("https://")?;
        let authority = rest.split(['/', '?', '#']).next()?;
        return Some(format!("https://{authority}{loc}"));
    }
    None
}

pub fn is_allowed_download_url(url: &str) -> bool {
    if url.bytes().any(|b| b.is_ascii_control() || b.is_ascii_whitespace()) || url.contains('\\') {
        return false;
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let host = match authority.split_once(':') {
        Some((h, "443")) => h,
        Some(_) => return false,
        None => authority,
    };
    let host = host.to_ascii_lowercase();
    if host.is_empty()
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        return false;
    }
    host == "github.com"
        || host.ends_with(".githubusercontent.com")
        || host == "huggingface.co"
        || host.ends_with(".huggingface.co")
        || host == "hf.co"
        || host.ends_with(".hf.co")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkRecord {
    pub model: String,
    pub passed: u32,
    pub cases: u32,
    pub median_seconds: f64,
    pub recorded: String,
    pub style: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SavedBenchmarks {
    pub results: BTreeMap<String, BenchmarkRecord>,
}

pub fn benchmarks_path() -> PathBuf {
    storage::data_dir().join("benchmarks.json")
}

pub fn load_benchmarks(preview: bool) -> SavedBenchmarks {
    if preview {
        return SavedBenchmarks::default();
    }
    load_benchmarks_from(&benchmarks_path())
}

pub fn load_benchmarks_from(path: &Path) -> SavedBenchmarks {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_benchmarks_to(path: &Path, data: &SavedBenchmarks) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
    storage::atomic_write(path, &bytes).map_err(|e| e.to_string())
}

struct BenchCase {
    raw: &'static str,
    must_contain: &'static [&'static str],
    banned_words: &'static [&'static str],
    banned_substrings: &'static [&'static str],
    check_safe: bool,
    check_list: bool,
    require_paragraph: bool,
}

const BENCHMARK_CASES: &[BenchCase] = &[
    BenchCase {
        raw: "um so I want to ship the uh settings fix today",
        must_contain: &["ship", "settings", "fix", "today"],
        banned_words: &["um", "uh"],
        banned_substrings: &[],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
    BenchCase {
        raw: "we should should test this right now",
        must_contain: &["should", "test", "right now"],
        banned_words: &[],
        banned_substrings: &["should should"],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
    BenchCase {
        raw: "run cargo test with --locked and --offline",
        must_contain: &["cargo test", "--locked", "--offline"],
        banned_words: &[],
        banned_substrings: &[],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
    BenchCase {
        raw: r"open C:\work\app.py and check line 42",
        must_contain: &[r"C:\work\app.py", "42"],
        banned_words: &[],
        banned_substrings: &["C:/work/app.py"],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
    BenchCase {
        raw: "do not delete the 5 backup files",
        must_contain: &["not", "delete", "5", "backup"],
        banned_words: &[],
        banned_substrings: &[],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
    BenchCase {
        raw: "Make a numbered list: first, check the logs, second, restart the app, third, test paste.",
        must_contain: &["check the logs", "restart the app", "test paste"],
        banned_words: &[],
        banned_substrings: &[],
        check_safe: true,
        check_list: true,
        require_paragraph: false,
    },
    BenchCase {
        raw: "the build passed new paragraph now let's test the UI",
        must_contain: &["build passed", "test the UI"],
        banned_words: &[],
        banned_substrings: &["new paragraph"],
        check_safe: true,
        check_list: false,
        require_paragraph: true,
    },
    BenchCase {
        raw: "set the port to 8080 scratch that set the port to 3000",
        must_contain: &["set", "port", "3000"],
        banned_words: &["8080"],
        banned_substrings: &["scratch that"],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
    BenchCase {
        raw: "check if x <= 10 and keep --dry-run",
        must_contain: &["<=", "10", "--dry-run"],
        banned_words: &[],
        banned_substrings: &["--dry_run"],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
    BenchCase {
        raw: "um, basically, like, we need to make paste instant instead of, uh, typing letter by letter",
        must_contain: &["paste", "instant", "letter"],
        banned_words: &["um", "uh"],
        banned_substrings: &[],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
    BenchCase {
        raw: "good morning can you review the pull request before friday",
        must_contain: &["morning", "review", "pull request"],
        banned_words: &[],
        banned_substrings: &[],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
    BenchCase {
        raw: "ship it before noon",
        must_contain: &["ship", "noon"],
        banned_words: &[],
        banned_substrings: &[],
        check_safe: true,
        check_list: false,
        require_paragraph: false,
    },
];

fn has_ai_preamble(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    ["sure", "certainly", "here's", "here is", "here are", "as an ai", "i can help", "i cannot help"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn eval_benchmark_case(
    case: &BenchCase,
    raw: &str,
    prepared: &formatting::PreparedDictation,
    out: &str,
    style: Style,
) -> bool {
    let normalized = out.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty()
        || trimmed.len() > raw.len().saturating_mul(3).max(200)
        || has_ai_preamble(trimmed)
    {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    for needle in case.must_contain {
        if !lower.contains(&needle.to_ascii_lowercase()) {
            return false;
        }
    }
    for banned in case.banned_substrings {
        if lower.contains(&banned.to_ascii_lowercase()) {
            return false;
        }
    }
    if !case.banned_words.is_empty() {
        let tokens: Vec<String> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect();
        for banned in case.banned_words {
            if tokens.iter().any(|tok| tok == banned) {
                return false;
            }
        }
    }
    if case.require_paragraph && !trimmed.contains("\n\n") {
        return false;
    }
    if case.check_list {
        if let Some(local_list) = prepared.local_list.as_deref() {
            if !formatting::keeps_list_lines_for_style(local_list, trimmed, "", style, &[]) {
                return false;
            }
        }
    }
    if case.check_safe && !prepared.is_safe_output_for_style(trimmed, style, &[]) {
        return false;
    }
    true
}

fn unload_ollama_model(ollama_url: &str, model: &str) {
    let base = match ollama_url.trim().trim_end_matches('/') {
        "http://127.0.0.1:11434" | "http://localhost:11434" => "http://127.0.0.1:11434",
        _ => return,
    };
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .try_proxy_from_env(false)
        .timeout(Duration::from_secs(3))
        .build();
    let _ = agent
        .post(&format!("{base}/api/chat"))
        .send_json(json!({ "model": model, "messages": [], "keep_alive": 0 }));
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

#[derive(Clone, Default)]
pub struct BenchmarkState {
    pub run_id: u64,
    pub active: bool,
    pub current_model: String,
    pub model_index: usize,
    pub model_total: usize,
    pub case_index: usize,
    pub case_total: usize,
    pub status: String,
    pub done: bool,
    pub error: String,
}

impl BenchmarkState {
    pub fn to_json(&self) -> Value {
        let total_steps = self.model_total.saturating_mul(self.case_total);
        let done_steps = self
            .model_index
            .saturating_sub(1)
            .saturating_mul(self.case_total)
            .saturating_add(self.case_index);
        let percent = (total_steps > 0).then(|| ((done_steps * 100) / total_steps).min(100) as u8);
        json!({
            "run_id": self.run_id,
            "active": self.active,
            "current_model": self.current_model,
            "model_index": self.model_index,
            "model_total": self.model_total,
            "case_index": self.case_index,
            "case_total": self.case_total,
            "percent": percent,
            "status": self.status,
            "done": self.done,
            "error": self.error,
        })
    }
}

pub struct BenchmarkManager {
    state: Mutex<BenchmarkState>,
    cancel: AtomicBool,
}

impl BenchmarkManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(BenchmarkState::default()),
            cancel: AtomicBool::new(false),
        }
    }

    pub fn status(&self) -> Value {
        lock_unpoisoned(&self.state).to_json()
    }

    pub fn is_active(&self) -> bool {
        lock_unpoisoned(&self.state).active
    }

    pub fn cancel(&self) -> bool {
        self.cancel.store(true, Ordering::Relaxed);
        let mut st = lock_unpoisoned(&self.state);
        if st.active {
            st.status = "Canceling benchmark…".into();
            return true;
        }
        false
    }

    pub fn start(
        self: &Arc<Self>,
        settings: Settings,
        target_model: &str,
        preview: bool,
    ) -> Value {
        if preview {
            return json!({"ok": false, "message": "Benchmarks are disabled in sample mode."});
        }
        let target = target_model.trim().to_owned();
        let run_id = {
            let mut st = lock_unpoisoned(&self.state);
            if st.active {
                return json!({"ok": false, "message": "A benchmark is already running. Wait or cancel it first."});
            }
            let next_id = st.run_id.wrapping_add(1).max(1);
            self.cancel.store(false, Ordering::Relaxed);
            *st = BenchmarkState {
                run_id: next_id,
                active: true,
                current_model: target.clone(),
                model_index: 0,
                model_total: 0,
                case_index: 0,
                case_total: BENCHMARK_CASES.len(),
                status: "Checking installed Ollama models…".into(),
                done: false,
                error: String::new(),
            };
            next_id
        };

        let mgr = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("model-benchmark".into())
            .spawn(move || {
                let result = mgr.run(settings, target);
                let mut st = lock_unpoisoned(&mgr.state);
                st.active = false;
                match result {
                    Ok(msg) => {
                        st.done = true;
                        st.status = msg;
                        st.error.clear();
                    }
                    Err(err) => {
                        st.done = false;
                        st.status = err.clone();
                        st.error = err;
                    }
                }
            });
        if spawned.is_err() {
            let mut st = lock_unpoisoned(&self.state);
            st.active = false;
            st.error = "Could not start the benchmark worker.".into();
            return json!({"ok": false, "message": st.error});
        }
        json!({"ok": true, "run_id": run_id, "message": "Benchmark started."})
    }

    fn set_status(&self, model: &str, mi: usize, mt: usize, ci: usize, msg: String) {
        if self.cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut st = lock_unpoisoned(&self.state);
        st.current_model = model.to_owned();
        st.model_index = mi;
        st.model_total = mt;
        st.case_index = ci;
        st.case_total = BENCHMARK_CASES.len();
        st.status = msg;
    }

    fn run(&self, settings: Settings, target_model: String) -> Result<String, String> {
        let installed = cleanup::ollama_models(&settings.ollama_url)
            .map_err(|_| "Could not reach Ollama. Start Ollama before running benchmarks.".to_string())?;
        if installed.is_empty() {
            return Err("No local models are installed in Ollama.".into());
        }
        let models_to_test: Vec<String> = if target_model.is_empty() {
            installed
        } else if installed.iter().any(|m| m == &target_model) {
            vec![target_model]
        } else {
            return Err(format!("{target_model} is not installed in Ollama."));
        };

        let style_label = if settings.style == Style::Clean { "clean" } else { "tighten" };
        let recorded = storage::now_stamp().0.split_whitespace().next().unwrap_or("today").to_owned();
        let path = benchmarks_path();
        let mut saved = load_benchmarks_from(&path);
        let total_models = models_to_test.len();

        for (m_idx, model_name) in models_to_test.iter().enumerate() {
            if self.cancel.load(Ordering::Relaxed) {
                return Err("Benchmark canceled.".into());
            }
            let mi = m_idx + 1;
            self.set_status(
                model_name,
                mi,
                total_models,
                0,
                format!("Warming {model_name} ({mi}/{total_models})…"),
            );
            cleanup::ollama_warm(&settings.ollama_url, model_name);
            if self.cancel.load(Ordering::Relaxed) {
                if !settings.cleanup || settings.local_model != *model_name {
                    unload_ollama_model(&settings.ollama_url, model_name);
                }
                return Err("Benchmark canceled.".into());
            }

            let mut run_settings = settings.clone();
            run_settings.local_model = model_name.clone();
            run_settings.vocab.clear();

            let mut passed = 0u32;
            let mut times = Vec::with_capacity(BENCHMARK_CASES.len());
            let mut consecutive_errors = 0u32;

            for (c_idx, case) in BENCHMARK_CASES.iter().enumerate() {
                if self.cancel.load(Ordering::Relaxed) {
                    if !settings.cleanup || settings.local_model != *model_name {
                        unload_ollama_model(&settings.ollama_url, model_name);
                    }
                    return Err("Benchmark canceled.".into());
                }
                self.set_status(
                    model_name,
                    mi,
                    total_models,
                    c_idx,
                    format!(
                        "Testing {model_name} ({mi}/{total_models}) · sample {}/{}…",
                        c_idx + 1,
                        BENCHMARK_CASES.len()
                    ),
                );
                let prepared = formatting::prepare_dictation(case.raw, "");
                let t0 = Instant::now();
                let res = cleanup::clean_dictation(&run_settings, prepared.model_input());
                let elapsed = t0.elapsed().as_secs_f64();
                match res {
                    Ok(out) => {
                        consecutive_errors = 0;
                        times.push(elapsed);
                        if eval_benchmark_case(case, case.raw, &prepared, &out, run_settings.style) {
                            passed += 1;
                        }
                    }
                    Err(_) => {
                        consecutive_errors += 1;
                        if consecutive_errors >= 3 {
                            if !settings.cleanup || settings.local_model != *model_name {
                                unload_ollama_model(&settings.ollama_url, model_name);
                            }
                            return Err(format!("Ollama stopped responding while testing {model_name}. Previous benchmark scores were kept."));
                        }
                    }
                }
            }

            if !settings.cleanup || settings.local_model != *model_name {
                unload_ollama_model(&settings.ollama_url, model_name);
            }
            if self.cancel.load(Ordering::Relaxed) {
                return Err("Benchmark canceled.".into());
            }
            if times.is_empty() {
                return Err(format!("Could not get any responses from {model_name}. Previous benchmark scores were kept."));
            }

            self.set_status(
                model_name,
                mi,
                total_models,
                BENCHMARK_CASES.len(),
                format!("Saved results for {model_name} ({mi}/{total_models})."),
            );
            let rec = BenchmarkRecord {
                model: model_name.clone(),
                passed,
                cases: BENCHMARK_CASES.len() as u32,
                median_seconds: (median(times) * 100.0).round() / 100.0,
                recorded: recorded.clone(),
                style: style_label.into(),
            };
            saved.results.insert(model_name.clone(), rec);
            save_benchmarks_to(&path, &saved)
                .map_err(|e| format!("Benchmark finished, but results could not be saved: {e}"))?;
        }

        if total_models == 1 {
            Ok(format!("Finished benchmarking {}.", models_to_test[0]))
        } else {
            Ok(format!("Finished benchmarking {total_models} local models."))
        }
    }
}

pub fn catalog_snapshot(
    settings: &Settings,
    installed_ollama: &[String],
    ollama_ok: bool,
    install_status: Value,
    benchmark_status: Value,
    saved_benchmarks: &SavedBenchmarks,
    latest_whisper_tag: Option<&str>,
    online_checked: bool,
    check_note: &str,
) -> Value {
    let active_speech = active_speech_id(settings);
    let parakeet_ok = parakeet_installed(settings);
    let has_whisper_cli = find_whisper_cli(settings).is_some();
    let models_dir = speech_models_dir();
    let mut speech_items: Vec<Value> = SPEECH_CATALOG
        .iter()
        .map(|entry| {
            let installed = if entry.id == PARAKEET_ID {
                parakeet_ok
            } else {
                has_whisper_cli && is_installed_whisper_file(&models_dir.join(entry.filename))
            };
            json!({
                "id": entry.id,
                "name": entry.name,
                "tag": entry.tag,
                "size_mb": entry.size_mb,
                "ram_note": entry.ram_note,
                "note": entry.note,
                "installed": installed,
                "active": active_speech == entry.id,
                "downloadable": !entry.filename.is_empty(),
            })
        })
        .collect();
    if active_speech == "whisper-cpp" && has_whisper_cli && Path::new(&settings.whisper_model).is_file() {
        let custom_name = Path::new(&settings.whisper_model)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Configured model");
        speech_items.push(json!({
            "id": "whisper-cpp",
            "name": format!("Whisper.cpp ({custom_name})"),
            "tag": "Custom configured",
            "size_mb": 0,
            "ram_note": "Local whisper.cpp CLI",
            "note": "Custom whisper.cpp model configured in settings.",
            "installed": true,
            "active": true,
            "downloadable": false,
        }));
    }

    let format_bench = |model_id: &str| -> Option<Value> {
        saved_benchmarks.results.get(model_id).map(|b| {
            let pct = if b.cases > 0 { (b.passed * 100 + b.cases / 2) / b.cases } else { 0 };
            json!({
                "passed": b.passed,
                "cases": b.cases,
                "percent": pct,
                "median_seconds": b.median_seconds,
                "recorded": b.recorded,
                "style": b.style,
                "label": format!("{}/{} ({pct}%) · {:.2} s", b.passed, b.cases, b.median_seconds),
            })
        })
    };

    let mut cleanup_items: Vec<Value> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    // Curated catalog models (installed or downloadable)
    for entry in CLEANUP_CATALOG {
        seen_ids.insert(entry.id.to_owned());
        let installed = installed_ollama.iter().any(|m| m == entry.id);
        cleanup_items.push(json!({
            "id": entry.id,
            "name": entry.name,
            "tier": entry.tier,
            "size_gb": entry.size_gb,
            "vram_note": entry.vram_note,
            "benchmark": format_bench(entry.id),
            "note": entry.note,
            "installed": installed,
            "active": settings.local_model == entry.id,
            "downloadable": true,
        }));
    }

    // Any other local Ollama models already installed on this machine (e.g. llama3.1:8b, llama3.2:latest)
    for name in installed_ollama {
        if seen_ids.insert(name.clone()) {
            cleanup_items.push(json!({
                "id": name,
                "name": name,
                "tier": "Installed in Ollama",
                "size_gb": "",
                "vram_note": "Local Ollama model",
                "benchmark": format_bench(name),
                "note": "Detected in your local Ollama library.",
                "installed": true,
                "active": settings.local_model == *name,
                "downloadable": false,
            }));
        }
    }

    // If the saved model is not currently installed (or Ollama is offline), keep it visible so the user sees what is saved.
    if !settings.local_model.is_empty() && seen_ids.insert(settings.local_model.clone()) {
        cleanup_items.push(json!({
            "id": settings.local_model,
            "name": settings.local_model,
            "tier": if ollama_ok { "Saved · not installed" } else { "Saved · Ollama offline" },
            "size_gb": "",
            "vram_note": "",
            "benchmark": format_bench(&settings.local_model),
            "note": if ollama_ok { "This saved model isn't installed in Ollama right now." } else { "Start Ollama to verify this saved model." },
            "installed": false,
            "active": true,
            "downloadable": false,
        }));
    }

    json!({
        "speech": speech_items,
        "cleanup": cleanup_items,
        "whisper_runtime_installed": has_whisper_cli,
        "whisper_runtime_latest": latest_whisper_tag.unwrap_or(WHISPER_FALLBACK_TAG),
        "ollama_available": ollama_ok,
        "install": install_status,
        "benchmark": benchmark_status,
        "benchmarks": saved_benchmarks.results,
        "online_checked": online_checked,
        "check_note": check_note,
    })
}

pub fn check_updates(
    settings: &Settings,
    preview: bool,
    installer: &InstallManager,
    benchmarker: &BenchmarkManager,
) -> (Value, Option<Vec<String>>, bool) {
    let saved_benchmarks = load_benchmarks(preview);
    if preview {
        let snap = catalog_snapshot(
            settings,
            &[],
            false,
            installer.status(),
            benchmarker.status(),
            &saved_benchmarks,
            Some(WHISPER_FALLBACK_TAG),
            false,
            "Sample mode skips online model checks.",
        );
        return (snap, None, false);
    }

    let ollama_res = cleanup::ollama_models(&settings.ollama_url);
    let (installed_ollama, ollama_ok, ollama_models_update) = match ollama_res {
        Ok(mut list) => {
            list.sort();
            list.dedup();
            (list.clone(), true, Some(list))
        }
        Err(_) => (Vec::new(), false, Some(Vec::new())),
    };

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .timeout_connect(Duration::from_millis(2500))
        .timeout_read(Duration::from_millis(3000))
        .build();

    let mut latest_tag = installer.whisper_tag();
    let mut online_notes = Vec::new();
    let mut online_ok = false;

    if let Ok(resp) = agent
        .get("https://api.github.com/repos/ggml-org/whisper.cpp/releases?per_page=10")
        .set("User-Agent", "listen-rs")
        .call()
    {
        if let Ok(Value::Array(releases)) = resp.into_json::<Value>() {
            online_ok = true;
            for rel in releases {
                if rel["draft"].as_bool() == Some(true) || rel["prerelease"].as_bool() == Some(true)
                {
                    continue;
                }
                let Some(tag) = rel["tag_name"].as_str() else { continue };
                let has_x64 = rel["assets"].as_array().is_some_and(|assets| {
                    assets.iter().any(|a| a["name"].as_str() == Some(WHISPER_ASSET_NAME))
                });
                if has_x64 && is_safe_release_tag(tag) {
                    latest_tag = tag.to_owned();
                    installer.set_latest_whisper_tag(latest_tag.clone());
                    break;
                }
            }
        }
    } else {
        online_notes.push("Could not reach GitHub for whisper.cpp release info (using known release).");
    }

    let note = if online_ok {
        format!("Checked whisper.cpp ({latest_tag}) and local Ollama models.")
    } else if !online_notes.is_empty() {
        online_notes.join(" ")
    } else {
        "Checked local models.".into()
    };

    let snap = catalog_snapshot(
        settings,
        &installed_ollama,
        ollama_ok,
        installer.status(),
        benchmarker.status(),
        &saved_benchmarks,
        Some(&latest_tag),
        online_ok,
        &note,
    );
    (snap, ollama_models_update, ollama_ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_and_archive_safety_reject_unsafe_inputs() {
        assert!(is_allowed_download_url(
            "https://github.com/ggml-org/whisper.cpp/releases/download/b5907/whisper-bin-x64.zip"
        ));
        assert!(is_allowed_download_url(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin"
        ));
        assert!(is_allowed_download_url(
            "https://cdn-lfs-us-1.hf.co:443/repos/abc/model.bin"
        ));
        assert!(!is_allowed_download_url("http://github.com/ggml-org/whisper.cpp"));
        assert!(!is_allowed_download_url("https://evil.example.com/model.bin"));
        assert!(!is_allowed_download_url("https://evil.com\\.hf.co/payload"));
        assert!(!is_allowed_download_url("https://user@github.com/a/b"));
        assert!(!is_allowed_download_url("https://github.com:8443/a/b"));
        assert!(!is_allowed_download_url("https://.hf.co/a/b"));

        assert_eq!(
            resolve_redirect_url("https://huggingface.co/a/b", "/cdn/model.bin").as_deref(),
            Some("https://huggingface.co/cdn/model.bin")
        );
        assert!(resolve_redirect_url("https://huggingface.co/a/b", "//evil.com/x").is_none());

        assert!(is_safe_archive_entry("Release/whisper-cli.exe"));
        assert!(!is_safe_archive_entry("../whisper-cli.exe"));
        assert!(!is_safe_archive_entry("/etc/passwd"));
        assert!(!is_safe_archive_entry("C:\\Windows\\system32\\cmd.exe"));
        assert!(is_safe_release_tag("b5907"));
        assert!(!is_safe_release_tag("b5907/../../evil"));

        let good_listing = "drwxr-xr-x 0 0 0 0 Nov 24 00:00 Release/\n-rwxr-xr-x 0 0 0 1024000 Nov 24 00:00 Release/whisper-cli.exe\n";
        assert!(validate_tar_verbose_listing(good_listing).is_ok());
        let symlink_listing = "lrwxrwxrwx 0 0 0 0 Nov 24 00:00 out -> C:\\Windows\n";
        assert!(validate_tar_verbose_listing(symlink_listing).is_err());
        let bomb_listing = "-rwxr-xr-x 0 0 0 9999999999 Nov 24 00:00 huge.bin\n";
        assert!(validate_tar_verbose_listing(bomb_listing).is_err());
    }

    #[test]
    fn benchmark_evaluator_and_storage_round_trip() {
        let prep0 = formatting::prepare_dictation(BENCHMARK_CASES[0].raw, "");
        assert!(eval_benchmark_case(
            &BENCHMARK_CASES[0],
            BENCHMARK_CASES[0].raw,
            &prep0,
            "So I want to ship the settings fix today.",
            Style::Clean,
        ));
        assert!(!eval_benchmark_case(
            &BENCHMARK_CASES[0],
            BENCHMARK_CASES[0].raw,
            &prep0,
            "Um, so I want to ship the, uh, settings fix today.",
            Style::Clean,
        ));

        let prep6 = formatting::prepare_dictation(BENCHMARK_CASES[6].raw, "");
        assert!(BENCHMARK_CASES[6].check_safe);
        assert!(eval_benchmark_case(
            &BENCHMARK_CASES[6],
            BENCHMARK_CASES[6].raw,
            &prep6,
            "The build passed.\n\nNow let's test the UI.",
            Style::Clean,
        ));
        assert!(!eval_benchmark_case(
            &BENCHMARK_CASES[6],
            BENCHMARK_CASES[6].raw,
            &prep6,
            "The build passed. Now let's test the UI.",
            Style::Clean,
        ));

        let prep7 = formatting::prepare_dictation(BENCHMARK_CASES[7].raw, "");
        assert!(BENCHMARK_CASES[7].check_safe);
        assert!(eval_benchmark_case(
            &BENCHMARK_CASES[7],
            BENCHMARK_CASES[7].raw,
            &prep7,
            "Set the port to 3000.",
            Style::Clean,
        ));
        assert!(!eval_benchmark_case(
            &BENCHMARK_CASES[7],
            BENCHMARK_CASES[7].raw,
            &prep7,
            "Set the port to 9000.",
            Style::Clean,
        ));
        assert!(!eval_benchmark_case(
            &BENCHMARK_CASES[7],
            BENCHMARK_CASES[7].raw,
            &prep7,
            "Sure! Here is your text: set the port to 3000.",
            Style::Clean,
        ));

        let prep9 = formatting::prepare_dictation(BENCHMARK_CASES[9].raw, "");
        assert!(eval_benchmark_case(
            &BENCHMARK_CASES[9],
            BENCHMARK_CASES[9].raw,
            &prep9,
            "We need to make paste instant instead of typing letter by letter.",
            Style::Clean,
        ));
        let list_raw = BENCHMARK_CASES[5].raw;
        let list_prepared = formatting::prepare_dictation(list_raw, "");
        assert!(
            list_prepared.local_list.is_some(),
            "case 5 should trigger spoken_list"
        );
        assert!(eval_benchmark_case(
            &BENCHMARK_CASES[5],
            list_raw,
            &list_prepared,
            list_prepared.model_input(),
            Style::Clean,
        ));
        assert_eq!(median(vec![0.9, 0.3, 0.5]), 0.5);
        assert_eq!(median(vec![0.8, 0.2, 0.4, 0.6]), 0.5);

        let dir = std::env::temp_dir().join(format!("listen-bench-test-{}", std::process::id()));
        let path = dir.join("benchmarks.json");
        let _ = std::fs::remove_dir_all(&dir);
        let mut data = SavedBenchmarks::default();
        data.results.insert(
            "qwen3.5:4b".into(),
            BenchmarkRecord {
                model: "qwen3.5:4b".into(),
                passed: 11,
                cases: 12,
                median_seconds: 0.41,
                recorded: "2026-11-25".into(),
                style: "tighten".into(),
            },
        );
        save_benchmarks_to(&path, &data).unwrap();
        let loaded = load_benchmarks_from(&path);
        assert_eq!(loaded.results["qwen3.5:4b"].passed, 11);
        assert_eq!(loaded.results["qwen3.5:4b"].cases, 12);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn benchmark_evaluator_respects_cleanup_style() {
        let prep0 = formatting::prepare_dictation(BENCHMARK_CASES[0].raw, "");
        let tightened = "I want to ship the settings fix today.";
        assert!(
            eval_benchmark_case(
                &BENCHMARK_CASES[0],
                BENCHMARK_CASES[0].raw,
                &prep0,
                tightened,
                Style::Tighten,
            ),
            "Style::Tighten should accept dropping filler 'so' in benchmark case 0"
        );
        assert!(
            !eval_benchmark_case(
                &BENCHMARK_CASES[0],
                BENCHMARK_CASES[0].raw,
                &prep0,
                tightened,
                Style::Clean,
            ),
            "Style::Clean should remain strict and reject dropping 'so'"
        );
    }

    #[test]
    fn catalog_includes_custom_installed_ollama_models_and_omits_nag_banner() {
        let settings = Settings { local_model: "llama3.1:8b".into(), ..Default::default() };
        let installed = vec!["qwen3.5:4b".into(), "llama3.1:8b".into()];
        let snap = catalog_snapshot(
            &settings,
            &installed,
            true,
            json!({"active": false}),
            json!({"active": false}),
            &SavedBenchmarks::default(),
            Some("b5907"),
            false,
            "",
        );
        assert!(snap.get("recommendation").is_none());
        let cleanup = snap["cleanup"].as_array().unwrap();
        assert!(cleanup.iter().any(|m| m["id"] == "llama3.1:8b" && m["installed"] == true && m["active"] == true));
        assert!(cleanup.iter().any(|m| m["id"] == "qwen3.5:4b" && m["installed"] == true));
    }

    #[test]
    fn installer_and_benchmarker_reject_unknown_models_and_preview_mode() {
        let mgr = Arc::new(InstallManager::new());
        assert_eq!(
            mgr.start("cleanup", "unlisted:99b", "http://127.0.0.1:11434".into(), false)["ok"],
            false
        );
        assert_eq!(
            mgr.start("speech", "whisper:../evil.bin", "http://127.0.0.1:11434".into(), false)["ok"],
            false
        );
        assert_eq!(
            mgr.start("cleanup", "qwen3.5:4b", "http://127.0.0.1:11434".into(), true)["ok"],
            false
        );
        let bm = Arc::new(BenchmarkManager::new());
        assert_eq!(bm.start(Settings::default(), "", true)["ok"], false);
    }

    #[test]
    fn installed_managed_whisper_models_ignores_tiny_or_unknown_files() {
        let dir = std::env::temp_dir().join(format!("listen-whisper-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ggml-small.en-q5_1.bin"), vec![0u8; 1024]).unwrap();
        assert!(installed_managed_whisper_models(&dir).is_empty());
        std::fs::write(dir.join("ggml-small.en-q5_1.bin"), vec![0u8; 1_000_005]).unwrap();
        std::fs::write(dir.join("unknown.bin"), vec![0u8; 1_000_005]).unwrap();
        let found = installed_managed_whisper_models(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "whisper:ggml-small.en-q5_1.bin");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
