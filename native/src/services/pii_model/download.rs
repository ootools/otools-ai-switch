//! On-demand download manager for the optional local PII model and its ONNX
//! Runtime. Files are written to the plugin-private data dir, verified against
//! pinned size + SHA-256, and only then made visible to the runtime.

use super::manifest::{self, Artifact, ArtifactKind};
use crate::error::AppError;
use crate::paths::AppPaths;
use crate::web::event_bridge::EventEmitter;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const DOWNLOAD_PROGRESS_EVENT: &str = "pii-model-download-progress";
pub const DOWNLOAD_STATUS_EVENT: &str = "pii-model-download-status";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Idle,
    Downloading,
    Verifying,
    Extracting,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub state: DownloadState,
    /// Artifact currently being processed, empty when idle.
    pub artifact: String,
    pub completed: u64,
    pub total: u64,
    pub percent: f64,
    pub error: Option<String>,
}

impl Default for DownloadProgress {
    fn default() -> Self {
        Self {
            state: DownloadState::Idle,
            artifact: String::new(),
            completed: 0,
            total: 0,
            percent: 0.0,
            error: None,
        }
    }
}

struct Inner {
    paths: Option<AppPaths>,
    emitter: EventEmitter,
    progress: DownloadProgress,
    cancel: Arc<AtomicBool>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            paths: None,
            emitter: EventEmitter::Noop,
            progress: DownloadProgress::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }
}

pub struct PiiDownloadManager {
    inner: Mutex<Inner>,
}

impl Default for PiiDownloadManager {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }
}

static MANAGER: OnceLock<PiiDownloadManager> = OnceLock::new();

pub fn pii_download() -> &'static PiiDownloadManager {
    MANAGER.get_or_init(PiiDownloadManager::default)
}

impl PiiDownloadManager {
    /// Called once from runtime init and from command handlers, so downloads
    /// started from any surface keep the same paths and event emitter.
    pub fn configure(&self, paths: AppPaths, emitter: EventEmitter) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.paths = Some(paths);
        inner.emitter = emitter;
    }

    pub fn status(&self) -> DownloadProgress {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .progress
            .clone()
    }

    fn emit(&self, progress: &DownloadProgress) {
        if let Ok(inner) = self.inner.lock() {
            inner.emitter.emit(DOWNLOAD_PROGRESS_EVENT, progress);
        }
    }

    pub fn cancel(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = inner.handle.take() {
            handle.abort();
        }
        inner.progress.state = DownloadState::Cancelled;
        let progress = inner.progress.clone();
        inner.emitter.emit(DOWNLOAD_STATUS_EVENT, &progress);
    }

    /// Delete downloaded model + runtime files. Safe to call repeatedly.
    pub async fn delete(&self) -> Result<(), AppError> {
        let paths = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.paths.clone()
        };
        let Some(paths) = paths else {
            return Err(AppError::Validation {
                code: "validation.pii_model_not_configured",
                message: "Model manager is not configured".to_string(),
                details: None,
                recoverable: true,
            });
        };
        for dir in [&paths.astrlink_model_dir, &paths.onnxruntime_dir] {
            if dir.exists() {
                tokio::fs::remove_dir_all(dir).await?;
            }
        }
        tokio::fs::create_dir_all(&paths.astrlink_model_dir).await?;
        tokio::fs::create_dir_all(&paths.onnxruntime_dir).await?;
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.progress = DownloadProgress::default();
        inner.emitter.emit(DOWNLOAD_STATUS_EVENT, &inner.progress.clone());
        Ok(())
    }

    /// Start a background download. Returns immediately; progress arrives via
    /// `pii-model-download-progress` events and `status()`.
    pub fn start(&self, mirror: Option<String>) -> Result<(), AppError> {
        let (paths, emitter) = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner.handle.as_ref().is_some_and(|handle| !handle.is_finished()) {
                return Err(AppError::Validation {
                    code: "validation.pii_model_download_busy",
                    message: "A model download is already running".to_string(),
                    details: None,
                    recoverable: true,
                });
            }
            inner.cancel = Arc::new(AtomicBool::new(false));
            let Some(paths) = inner.paths.clone() else {
                return Err(AppError::Validation {
                    code: "validation.pii_model_not_configured",
                    message: "Model manager is not configured".to_string(),
                    details: None,
                    recoverable: true,
                });
            };
            inner.progress = DownloadProgress {
                state: DownloadState::Downloading,
                ..DownloadProgress::default()
            };
            (paths, inner.emitter.clone())
        };

        let cancel = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.cancel.clone()
        };
        let handle = tokio::spawn(async move {
            let result = download_all(&paths, mirror.as_deref(), &cancel, &emitter).await;
            let progress = DownloadProgress {
                state: match result {
                    Ok(()) => DownloadState::Ready,
                    Err(ref error) if error.contains("cancelled") => DownloadState::Cancelled,
                    Err(_) => DownloadState::Failed,
                },
                artifact: String::new(),
                completed: 0,
                total: 0,
                percent: 100.0,
                error: result.err(),
            };
            emitter.emit(DOWNLOAD_STATUS_EVENT, &progress);
            if let Ok(mut inner) = MANAGER.get_or_init(PiiDownloadManager::default).inner.lock() {
                inner.progress = progress;
                inner.handle = None;
            }
        });
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.handle = Some(handle);
        Ok(())
    }
}

fn target_dir(paths: &AppPaths, artifact: &Artifact) -> PathBuf {
    match artifact.kind {
        ArtifactKind::Model => paths.astrlink_model_dir.clone(),
        ArtifactKind::OnnxRuntime => paths.onnxruntime_dir.clone(),
    }
}

/// Stream a file through SHA-256 and compare it with a pinned hex digest.
/// Also returns the byte count so callers can check a size limit in the same
/// pass (plugin manifests have no pre-pinned size).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<u64, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let bytes = std::io::copy(&mut file, &mut hasher).map_err(|error| error.to_string())?;
    let digest = format!("{:x}", hasher.finalize());
    if digest != expected {
        return Err(format!(
            "SHA-256 mismatch (got {digest}, expected {expected})"
        ));
    }
    Ok(bytes)
}

/// Verify size and SHA-256 of an already-downloaded file.
pub fn verify_file(path: &Path, artifact: &Artifact) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if meta.len() != artifact.size {
        return Err(format!(
            "{}: size {} != pinned {}",
            artifact.name,
            meta.len(),
            artifact.size
        ));
    }
    verify_sha256(path, &artifact.sha256)
        .map(|_| ())
        .map_err(|error| format!("{}: {error}", artifact.name))
}

/// Check whether every required artifact is present and valid.
pub fn all_installed(paths: &AppPaths) -> HashMap<String, bool> {
    manifest::all_artifacts()
        .into_iter()
        .map(|artifact| {
            let path = target_dir(paths, &artifact).join(&artifact.name);
            let ok = path.is_file() && verify_file(&path, &artifact).is_ok();
            (artifact.name.to_string(), ok)
        })
        .collect()
}

async fn download_all(
    paths: &AppPaths,
    mirror: Option<&str>,
    cancel: &AtomicBool,
    emitter: &EventEmitter,
) -> Result<(), String> {
    for artifact in manifest::all_artifacts() {
        let dir = target_dir(paths, &artifact);
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|error| error.to_string())?;
        let final_path = dir.join(&artifact.name);
        if final_path.is_file() && verify_file(&final_path, &artifact).is_ok() {
            continue;
        }
        download_one(&artifact, &final_path, mirror, cancel, emitter).await?;
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        emitter.emit(
            DOWNLOAD_STATUS_EVENT,
            &DownloadProgress {
                state: DownloadState::Verifying,
                artifact: artifact.name.to_string(),
                completed: artifact.size,
                total: artifact.size,
                percent: 100.0,
                error: None,
            },
        );
        verify_file(&final_path, &artifact)?;
        if artifact.kind == ArtifactKind::OnnxRuntime {
            extract_runtime(&final_path, &paths.onnxruntime_dir)?;
        }
    }
    Ok(())
}

async fn download_one(
    artifact: &Artifact,
    final_path: &Path,
    mirror: Option<&str>,
    cancel: &AtomicBool,
    emitter: &EventEmitter,
) -> Result<(), String> {
    let url = manifest::resolve_url(artifact, mirror);
    // Validate the mirror before any request so a typo cannot fall through to
    // an unrelated host that happens to serve similarly named files.
    url::Url::parse(&url).map_err(|error| format!("invalid download URL: {error}"))?;
    let client = crate::services::http_client::build_outbound_http_client_with_timeouts(
        crate::services::http_client::OutboundTimeouts {
            total: None,
            connect: Some(std::time::Duration::from_secs(30)),
            read: Some(std::time::Duration::from_secs(60)),
        },
    )
    .map_err(|error| format!("could not build download client: {error}"))?;

    let part = final_path.with_extension("part");
    let mut resume_from = 0u64;
    if let Ok(meta) = tokio::fs::metadata(&part).await {
        resume_from = meta.len().min(artifact.size);
    }
    let mut request = client.get(&url);
    if resume_from > 0 {
        request = request.header("Range", format!("bytes={resume_from}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("{}: download failed: {error}", artifact.name))?;
    let status = response.status();
    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("{}: HTTP {status}", artifact.name));
    }
    let append = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !append {
        resume_from = 0;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(!append)
        .open(&part)
        .await
        .map_err(|error| error.to_string())?;
    use tokio::io::AsyncSeekExt;
    file.seek(std::io::SeekFrom::Start(resume_from))
        .await
        .map_err(|error| error.to_string())?;

    let mut hasher = Sha256::new();
    if append && resume_from > 0 {
        let bytes = tokio::fs::read(&part).await.map_err(|error| error.to_string())?;
        hasher.update(&bytes);
    }
    let total = response.content_length().unwrap_or(artifact.size.saturating_sub(resume_from)) + resume_from;
    let mut done = resume_from;
    let mut stream = response.bytes_stream();
    let mut last_emit = std::time::Instant::now();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let chunk = chunk.map_err(|error| format!("{}: stream failed: {error}", artifact.name))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|error| error.to_string())?;
        hasher.update(&chunk);
        done += chunk.len() as u64;
        if last_emit.elapsed() >= std::time::Duration::from_millis(100) {
            last_emit = std::time::Instant::now();
            emitter.emit(
                DOWNLOAD_PROGRESS_EVENT,
                &DownloadProgress {
                    state: DownloadState::Downloading,
                    artifact: artifact.name.to_string(),
                    completed: done,
                    total,
                    percent: if total == 0 { 0.0 } else { done as f64 * 100.0 / total as f64 },
                    error: None,
                },
            );
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|error| error.to_string())?;
    drop(file);

    let digest = format!("{:x}", hasher.finalize());
    let meta = tokio::fs::metadata(&part).await.map_err(|error| error.to_string())?;
    if meta.len() != artifact.size || digest != artifact.sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(format!(
            "{}: verification failed (size {}, sha {digest})",
            artifact.name,
            meta.len()
        ));
    }
    tokio::fs::rename(&part, final_path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Extract only the native runtime library from a verified archive.
fn extract_runtime(archive: &Path, target: &Path) -> Result<(), String> {
    let wanted = manifest::ort_library_file_name();
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    if archive
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".zip"))
    {
        let file = std::fs::File::open(archive).map_err(|error| error.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(|error| error.to_string())?;
            let name = entry.name().replace('\\', "/");
            let file_name = name.rsplit('/').next().unwrap_or("");
            if !file_name.to_ascii_lowercase().ends_with(".dll") {
                continue;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            std::fs::write(target.join(file_name), bytes).map_err(|error| error.to_string())?;
        }
    } else {
        let file = std::fs::File::open(archive).map_err(|error| error.to_string())?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(decoder);
        for entry in tar.entries().map_err(|error| error.to_string())? {
            let mut entry = entry.map_err(|error| error.to_string())?;
            let name = entry.path().map_err(|error| error.to_string())?.to_path_buf();
            let file_name = name
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if file_name != wanted {
                continue;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            std::fs::write(target.join(file_name), bytes).map_err(|error| error.to_string())?;
        }
    }
    drop(staging);
    if !target.join(wanted).is_file() {
        return Err(format!("archive did not contain {wanted}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_file_rejects_a_size_mismatch() {
        let artifact = &manifest::MODEL_FILES[0];
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(artifact.name);
        std::fs::write(&path, b"short").expect("write");
        assert!(verify_file(&path, artifact).is_err());
    }

    #[test]
    fn download_progress_defaults_to_idle() {
        let progress = DownloadProgress::default();
        assert_eq!(progress.state, DownloadState::Idle);
        assert_eq!(progress.percent, 0.0);
    }
}
