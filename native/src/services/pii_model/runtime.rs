//! Runtime wrapper for the optional AstrLink Guard PII model.
//!
//! ONNX Runtime is loaded dynamically from the plugin data dir; no native
//! dependency is linked into the plugin. A single session is kept while in
//! use and dropped after an idle period so its memory is released.

use super::decoder::{char_offsets_to_byte_offsets, decode_window, merge_windows, ModelSpan};
use super::download;
use super::manifest;
use crate::error::AppError;
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokenizers::Tokenizer;

const WINDOW: usize = 510; // 512 minus <s> and </s>
const STRIDE: usize = 128;
const PAD_ID: u32 = 1;
const IDLE_AFTER: Duration = Duration::from_secs(10 * 60);
const FAILURE_LIMIT: u32 = 3;
const COOLDOWN: Duration = Duration::from_secs(60);
const MAX_TEXTS_PER_REQUEST: usize = 64;
const MIN_TEXT_CHARS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Missing,
    Partial,
    Ready,
    Corrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Stopped,
    Loading,
    Ready,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PiiModelStatusView {
    pub supported: bool,
    pub file_state: FileState,
    pub runtime_state: RuntimeState,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub device: String,
    pub load_ms: u64,
    pub model_size: u64,
    pub ort_size: u64,
    pub last_error: Option<String>,
}

impl Default for PiiModelStatusView {
    fn default() -> Self {
        let artifacts = manifest::all_artifacts();
        Self {
            supported: manifest::ort_asset().is_some(),
            file_state: FileState::Missing,
            runtime_state: RuntimeState::Stopped,
            downloaded_bytes: 0,
            total_bytes: artifacts.iter().map(|item| item.size).sum(),
            device: String::new(),
            load_ms: 0,
            model_size: manifest::MODEL_FILES.iter().map(|item| item.size).sum(),
            ort_size: manifest::ort_asset().map(|item| item.size).unwrap_or(0),
            last_error: None,
        }
    }
}

struct Loaded {
    session: ort::session::Session,
    tokenizer: Tokenizer,
    labels: Vec<String>,
    load_ms: u64,
}

pub struct PiiModelManager {
    paths: RwLock<Option<AppPaths>>,
    loaded: Mutex<Option<Loaded>>,
    threshold: RwLock<f32>,
    last_used: Mutex<Instant>,
    failures: AtomicU32,
    cooldown_until: Mutex<Option<Instant>>,
    enabled: AtomicBool,
    /// Hashing 40 MB on every request would be wasteful; cache for 30 seconds.
    verified_at: Mutex<Option<Instant>>,
    verified_files: Mutex<std::collections::HashMap<String, bool>>,
}

impl Default for PiiModelManager {
    fn default() -> Self {
        Self {
            paths: RwLock::new(None),
            loaded: Mutex::new(None),
            threshold: RwLock::new(0.8),
            last_used: Mutex::new(Instant::now()),
            failures: AtomicU32::new(0),
            cooldown_until: Mutex::new(None),
            enabled: AtomicBool::new(false),
            verified_at: Mutex::new(None),
            verified_files: Mutex::new(std::collections::HashMap::new()),
        }
    }
}

static MANAGER: OnceLock<Arc<PiiModelManager>> = OnceLock::new();

pub fn pii_model_runtime() -> Arc<PiiModelManager> {
    MANAGER
        .get_or_init(|| Arc::new(PiiModelManager::default()))
        .clone()
}

impl PiiModelManager {
    pub fn configure(&self, paths: AppPaths) {
        if let Ok(mut current) = self.paths.write() {
            *current = Some(paths);
        }
    }

    pub fn invalidate_verification(&self) {
        if let Ok(mut guard) = self.verified_at.lock() {
            *guard = None;
        }
        self.shutdown();
    }

    fn installed(&self, paths: &AppPaths) -> std::collections::HashMap<String, bool> {
        let fresh = self
            .verified_at
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .is_some_and(|at| at.elapsed() < Duration::from_secs(30));
        if fresh {
            if let Ok(guard) = self.verified_files.lock() {
                return guard.clone();
            }
        }
        let result = download::all_installed(paths);
        if let Ok(mut guard) = self.verified_files.lock() {
            *guard = result.clone();
        }
        if let Ok(mut guard) = self.verified_at.lock() {
            *guard = Some(Instant::now());
        }
        result
    }

    pub fn set_enabled(&self, enabled: bool, threshold_percent: u32) {
        self.enabled.store(enabled, Ordering::Relaxed);
        let threshold = (threshold_percent as f32 / 100.0).clamp(0.5, 0.99);
        if let Ok(mut current) = self.threshold.write() {
            *current = threshold;
        }
        if !enabled {
            self.shutdown();
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn threshold(&self) -> f32 {
        self.threshold.read().map(|value| *value).unwrap_or(0.8)
    }

    pub fn shutdown(&self) {
        if let Ok(mut loaded) = self.loaded.lock() {
            *loaded = None;
        }
    }

    fn in_cooldown(&self) -> bool {
        self.cooldown_until
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .is_some_and(|until| Instant::now() < until)
    }

    fn note_failure(&self) {
        let count = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= FAILURE_LIMIT {
            if let Ok(mut cooldown) = self.cooldown_until.lock() {
                *cooldown = Some(Instant::now() + COOLDOWN);
            }
            self.failures.store(0, Ordering::Relaxed);
        }
    }

    pub fn status(&self) -> PiiModelStatusView {
        let mut view = PiiModelStatusView::default();
        let paths_guard = match self.paths.read() {
            Ok(guard) => guard,
            Err(_) => return view,
        };
        let Some(paths) = paths_guard.as_ref() else {
            return view;
        };
        let installed = self.installed(paths);
        let ready = installed.values().all(|ok| *ok);
        let downloaded: u64 = manifest::all_artifacts()
            .iter()
            .filter(|item| installed.get(item.name).copied().unwrap_or(false))
            .map(|item| item.size)
            .sum();
        view.downloaded_bytes = downloaded;
        view.file_state = if ready {
            FileState::Ready
        } else if downloaded > 0 {
            FileState::Partial
        } else {
            FileState::Missing
        };
        let runtime = match self.loaded.lock() {
            Ok(guard) => guard,
            Err(_) => return view,
        };
        if let Some(loaded) = runtime.as_ref() {
            view.runtime_state = RuntimeState::Ready;
            view.device = "cpu".to_string();
            view.load_ms = loaded.load_ms;
        } else if self.in_cooldown() {
            view.runtime_state = RuntimeState::Degraded;
            view.last_error = Some("模型连续加载失败，暂时冷却中".to_string());
        }
        view
    }

    fn ensure_ready(&self) -> Result<(), String> {
        if self.in_cooldown() {
            return Err("本地 PII 模型连续失败，稍后会自动重试".to_string());
        }
        if self.loaded.lock().map_err(|e| e.to_string())?.is_some() {
            return Ok(());
        }
        let paths = self
            .paths
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "模型管理器未初始化".to_string())?;
        if !manifest::ort_asset().is_some_and(|asset| {
            paths.onnxruntime_dir.join(asset.name).is_file()
        }) {
            return Err("ONNX Runtime 未下载".to_string());
        }
        let installed = self.installed(&paths);
        if !installed.values().all(|ok| *ok) {
            return Err("本地 PII 模型文件缺失或未通过校验".to_string());
        }

        let started = Instant::now();
        let library = paths
            .onnxruntime_dir
            .join(manifest::ort_library_file_name());
        let env = ort::init_from(library.to_string_lossy().to_string());
        env.commit()
            .map_err(|error| format!("ONNX Runtime 加载失败: {error}"))?;
        let model = paths.astrlink_model_dir.join("model_int8.onnx");
        let session = ort::session::Session::builder()
            .map_err(|error| format!("创建会话失败: {error}"))?
            .commit_from_file(&model)
            .map_err(|error| format!("加载模型失败: {error}"))?;
        let tokenizer_path = paths.astrlink_model_dir.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|error| format!("加载分词器失败: {error}"))?;
        let labels_path = paths.astrlink_model_dir.join("labels.json");
        let labels_value: serde_json::Value = serde_json::from_reader(
            std::fs::File::open(&labels_path).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("读取标签失败: {error}"))?;
        let labels: Vec<String> = labels_value
            .get("id2label")
            .and_then(serde_json::Value::as_object)
            .map(|object| {
                let mut pairs: Vec<(usize, String)> = object
                    .iter()
                    .filter_map(|(key, value)| {
                        Some((key.parse::<usize>().ok()?, value.as_str()?.to_string()))
                    })
                    .collect();
                pairs.sort_by_key(|(index, _)| *index);
                pairs.into_iter().map(|(_, value)| value).collect()
            })
            .ok_or_else(|| "labels.json 缺少 id2label".to_string())?;
        if labels.is_empty() {
            return Err("labels.json 的 id2label 为空".to_string());
        }

        let mut guard = self.loaded.lock().map_err(|e| e.to_string())?;
        *guard = Some(Loaded {
            session,
            tokenizer,
            labels,
            load_ms: started.elapsed().as_millis() as u64,
        });
        self.failures.store(0, Ordering::Relaxed);
        Ok(())
    }

    /// Detect PII spans in one text. Offsets are byte offsets of `text`.
    pub fn detect(&self, text: &str) -> Result<Vec<ModelSpan>, String> {
        if !self.enabled.load(Ordering::Relaxed) {
            return Ok(Vec::new());
        }
        if text.chars().count() < MIN_TEXT_CHARS {
            return Ok(Vec::new());
        }
        self.ensure_ready()?;
        if let Ok(mut last) = self.last_used.lock() {
            *last = Instant::now();
        }

        let mut guard = self.loaded.lock().map_err(|e| e.to_string())?;
        let Some(loaded) = guard.as_mut() else {
            self.note_failure();
            return Err("模型未就绪".to_string());
        };
        let encoding = loaded
            .tokenizer
            .encode(text, false)
            .map_err(|error| format!("分词失败: {error}"))?;
        let ids = encoding.get_ids();
        let offsets = encoding.get_offsets();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let threshold = self.threshold();
        let label_count = loaded.labels.len();
        let mut all_spans = Vec::new();
        let mut start = 0usize;
        while start < ids.len() {
            let end = (start + WINDOW).min(ids.len());
            let window_ids = &ids[start..end];
            let window_offsets = &offsets[start..end];
            let n = window_ids.len();
            let shape = [1usize, n];
            let input_ids = Vec::<i64>::from_iter(window_ids.iter().map(|id| *id as i64));
            let attention = vec![1i64; n];
            let token_types = vec![0i64; n];
            let outputs = loaded
                .session
                .run(ort::inputs! {
                    "input_ids" => ort::value::Tensor::from_array((shape, input_ids))
                        .map_err(|error| format!("构造 input_ids 失败: {error}"))?,
                    "attention_mask" => ort::value::Tensor::from_array((shape, attention))
                        .map_err(|error| format!("构造 attention_mask 失败: {error}"))?,
                    "token_type_ids" => ort::value::Tensor::from_array((shape, token_types))
                        .map_err(|error| format!("构造 token_type_ids 失败: {error}"))?,
                })
                .map_err(|error| {
                    self.note_failure();
                    format!("模型推理失败: {error}")
                })?;
            let output = outputs
                .get("logits")
                .ok_or_else(|| "模型输出缺少 logits".to_string())?;
            let (_, logits) = output
                .try_extract_tensor::<f32>()
                .map_err(|error| format!("读取 logits 失败: {error}"))?;
            let char_offsets: Vec<(usize, usize)> = window_offsets
                .iter()
                .map(|offset| (offset.0, offset.1))
                .collect();
            all_spans.extend(decode_window(
                logits,
                n,
                &char_offsets,
                &loaded.labels,
                threshold,
            ));
            if end == ids.len() {
                break;
            }
            start += STRIDE;
        }
        drop(guard);

        let char_spans = merge_windows(all_spans);
        Ok(char_offsets_to_byte_offsets(text, &char_spans))
    }

    /// Detect spans for several texts, enforcing a per-request cap.
    pub fn detect_all(&self, texts: &[&str]) -> Result<Vec<Vec<ModelSpan>>, String> {
        if texts.len() > MAX_TEXTS_PER_REQUEST {
            return Err(format!(
                "请求包含 {} 个文本字段，超过本地模型单次上限 {MAX_TEXTS_PER_REQUEST}",
                texts.len()
            ));
        }
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            out.push(self.detect(text)?);
        }
        Ok(out)
    }

    /// Drop the loaded session after 10 minutes idle.
    pub async fn idle_watchdog(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let idle = self
                .last_used
                .lock()
                .map(|guard| guard.elapsed())
                .unwrap_or_default();
            if idle >= IDLE_AFTER && self.loaded.lock().map(|guard| guard.is_some()).unwrap_or(false) {
                self.shutdown();
            }
        }
    }

    pub fn warm(&self) -> Result<PiiModelStatusView, AppError> {
        self.ensure_ready().map_err(|message| AppError::Validation {
            code: "validation.pii_model_unavailable",
            message,
            details: None,
            recoverable: true,
        })?;
        Ok(self.status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_status_reports_supported_platform_and_total_size() {
        let status = PiiModelStatusView::default();
        assert_eq!(status.supported, manifest::ort_asset().is_some());
        assert!(status.total_bytes > 0);
        assert_eq!(status.file_state, FileState::Missing);
    }

    #[test]
    fn disabled_manager_does_not_load_anything() {
        let manager = PiiModelManager::default();
        assert!(!manager.enabled());
        assert!(manager.detect("张三住在北京市朝阳区").expect("disabled").is_empty());
    }
}
