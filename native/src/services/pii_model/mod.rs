//! Optional local PII model: pinned downloads + dynamic ONNX Runtime.

pub mod decoder;
pub mod download;
pub mod manifest;
pub mod runtime;

pub use decoder::ModelSpan;
pub use download::{pii_download, DownloadProgress, DownloadState, PiiDownloadManager};
pub use runtime::{pii_model_runtime, FileState, PiiModelStatusView, RuntimeState};
