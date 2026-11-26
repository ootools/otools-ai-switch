//! Pinned manifest for the optional local PII model.
//!
//! Nothing here is shipped in the plugin package: both the model and ONNX
//! Runtime are downloaded into the plugin's private data directory on demand.
//! Every artifact carries a pinned size and SHA-256. A mirror only replaces
//! the transport; it never replaces the trust anchor.

/// Immutable HuggingFace revision (v0.1.3 == main at time of writing).
pub const HF_REV: &str = "7216b6a3b897ef903d4b1ba367ae6fb95b0fa5e4";
pub const HF_BASE: &str = "https://huggingface.co/QuantumNous/astrlink-guard/resolve";
pub const ORT_VERSION: &str = "1.22.0";
pub const ORT_RELEASE_BASE: &str = "https://github.com/microsoft/onnxruntime/releases/download/v1.22.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Artifact {
    /// Filename on disk.
    pub name: &'static str,
    /// Path relative to the download root, used with a mirror.
    pub rel: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub kind: ArtifactKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Model,
    OnnxRuntime,
}

/// Files required for AstrLink Guard int8 inference. `model_int8.onnx_data`
/// is ONNX external data and must sit next to the `.onnx` file.
pub const MODEL_FILES: &[Artifact] = &[
    Artifact {
        name: "model_int8.onnx",
        rel: "int8/model_int8.onnx",
        size: 330_781,
        sha256: "0a070d3113615e33ba3b84d6bd486f663a377c04c4b056e7381732179f51832a",
        kind: ArtifactKind::Model,
    },
    Artifact {
        name: "model_int8.onnx_data",
        rel: "int8/model_int8.onnx_data",
        size: 37_124_608,
        sha256: "0ba34216ceead01e0ae2dfd8e46d52b67e7aee80b04f722d9b758613a91c0067",
        kind: ArtifactKind::Model,
    },
    Artifact {
        name: "tokenizer.json",
        rel: "int8/tokenizer.json",
        size: 1_502_881,
        sha256: "0cf3b235ef125658015a6c8026a5fe6f28088bf825101a990ec18a10200feef4",
        kind: ArtifactKind::Model,
    },
    Artifact {
        name: "labels.json",
        rel: "int8/labels.json",
        size: 768,
        sha256: "1a81b6cdcedbc8481b956109df303a84fbbef84cc2229bf3a26c80237dc1f120",
        kind: ArtifactKind::Model,
    },
    Artifact {
        name: "config.json",
        rel: "int8/config.json",
        size: 1_804,
        sha256: "2c20003590d642e273c03c4c7c82d9849fa75f64bf29a71ec0a0c9a20ba4a150",
        kind: ArtifactKind::Model,
    },
];

/// CPU-only ONNX Runtime 1.22 package for the current platform. Microsoft does
/// not publish checksums for these assets, so they were computed from the
/// official release files and pinned here. Mirror bytes must match.
pub fn ort_asset() -> Option<Artifact> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some(Artifact {
            name: "onnxruntime-win-x64-1.22.0.zip",
            rel: "onnxruntime-win-x64-1.22.0.zip",
            size: 72_368_545,
            sha256: "174c616efc0271194488642a72f1a514e01487da4dfe84c49296d66e40ebe0da",
            kind: ArtifactKind::OnnxRuntime,
        })
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(Artifact {
            name: "onnxruntime-linux-x64-1.22.0.tgz",
            rel: "onnxruntime-linux-x64-1.22.0.tgz",
            size: 7_798_730,
            sha256: "8344d55f93d5bc5021ce342db50f62079daf39aaafb5d311a451846228be49b3",
            kind: ArtifactKind::OnnxRuntime,
        })
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(Artifact {
            name: "onnxruntime-osx-arm64-1.22.0.tgz",
            rel: "onnxruntime-osx-arm64-1.22.0.tgz",
            size: 25_943_843,
            sha256: "cab6dcbd77e7ec775390e7b73a8939d45fec3379b017c7cb74f5b204c1a1cc07",
            kind: ArtifactKind::OnnxRuntime,
        })
    } else {
        None
    }
}

pub fn all_artifacts() -> Vec<Artifact> {
    let mut items: Vec<Artifact> = MODEL_FILES.iter().copied().collect();
    if let Some(asset) = ort_asset() {
        items.push(asset);
    }
    items
}

/// Official URL for an artifact, or a mirror base + canonical relative path.
pub fn resolve_url(artifact: &Artifact, mirror: Option<&str>) -> String {
    if let Some(base) = mirror.map(str::trim).filter(|value| !value.is_empty()) {
        return format!("{}/{}", base.trim_end_matches('/'), artifact.rel);
    }
    match artifact.kind {
        ArtifactKind::Model => format!("{HF_BASE}/{HF_REV}/{}", artifact.rel),
        ArtifactKind::OnnxRuntime => format!("{ORT_RELEASE_BASE}/{}", artifact.name),
    }
}

/// Dynamic library filename ONNX Runtime exposes on this platform.
pub fn ort_library_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_and_mirror_urls_are_distinct_and_stable() {
        let model = &MODEL_FILES[0];
        let official = resolve_url(model, None);
        assert_eq!(
            official,
            format!("{HF_BASE}/{HF_REV}/int8/model_int8.onnx")
        );
        assert_eq!(
            resolve_url(model, Some("https://mirror.example/ort/")),
            "https://mirror.example/ort/int8/model_int8.onnx"
        );
    }

    #[test]
    fn every_model_file_has_a_pinned_hash_and_size() {
        assert_eq!(MODEL_FILES.len(), 5);
        for file in MODEL_FILES {
            assert!(file.size > 0);
            assert_eq!(file.sha256.len(), 64);
        }
    }
}
