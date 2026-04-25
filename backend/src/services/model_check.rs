/// Model-file integrity verification, run once at startup.
/// Sprint 1: no models downloaded — degraded is the expected state.
///
/// Streams each file and computes SHA-256. Records pass/fail in ReadinessState.
/// /healthz still returns 200 in degraded mode; /readyz returns 503.
use crate::state::ModelStatus;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
use tokio::task;
use toml::Value;

const MODELS_TOML: &str = include_str!("../../assets/models.toml");

#[derive(Debug)]
struct ModelEntry {
    name: String,
    path: String,
    sha256: String,
    size_bytes: u64,
}

fn parse_models_toml() -> Vec<ModelEntry> {
    let parsed: Value = MODELS_TOML.parse().unwrap_or(Value::Table(Default::default()));
    let mut entries = Vec::new();

    if let Some(table) = parsed.as_table() {
        for (category, cat_val) in table {
            if let Some(models) = cat_val.as_table() {
                for (model_key, model_val) in models {
                    let name = format!("{category}.{model_key}");
                    let path = model_val
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let sha256 = model_val
                        .get("sha256")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let size_bytes = model_val
                        .get("size_bytes")
                        .and_then(|v| v.as_integer())
                        .unwrap_or(0) as u64;
                    entries.push(ModelEntry {
                        name,
                        path,
                        sha256,
                        size_bytes,
                    });
                }
            }
        }
    }
    entries
}

fn verify_file(model_dir: &Path, entry: &ModelEntry) -> ModelStatus {
    let full_path = model_dir.join(&entry.path);

    if !full_path.exists() {
        return ModelStatus {
            name: entry.name.clone(),
            path: entry.path.clone(),
            ok: false,
            reason: Some(format!("file not found: {}", full_path.display())),
        };
    }

    let meta = match std::fs::metadata(&full_path) {
        Ok(m) => m,
        Err(e) => {
            return ModelStatus {
                name: entry.name.clone(),
                path: entry.path.clone(),
                ok: false,
                reason: Some(format!("metadata error: {e}")),
            }
        }
    };

    if meta.len() != entry.size_bytes {
        return ModelStatus {
            name: entry.name.clone(),
            path: entry.path.clone(),
            ok: false,
            reason: Some(format!(
                "size mismatch: expected {} bytes, got {}",
                entry.size_bytes,
                meta.len()
            )),
        };
    }

    // Stream SHA-256
    let mut file = match std::fs::File::open(&full_path) {
        Ok(f) => f,
        Err(e) => {
            return ModelStatus {
                name: entry.name.clone(),
                path: entry.path.clone(),
                ok: false,
                reason: Some(format!("open error: {e}")),
            }
        }
    };

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) => {
                return ModelStatus {
                    name: entry.name.clone(),
                    path: entry.path.clone(),
                    ok: false,
                    reason: Some(format!("read error: {e}")),
                }
            }
        }
    }
    let actual = hex::encode(hasher.finalize());

    if actual != entry.sha256 {
        tracing::error!(
            model = %entry.name,
            expected = %entry.sha256,
            actual = %actual,
            "model integrity check failed"
        );
        ModelStatus {
            name: entry.name.clone(),
            path: entry.path.clone(),
            ok: false,
            reason: Some(format!("sha256 mismatch: got {actual}")),
        }
    } else {
        ModelStatus {
            name: entry.name.clone(),
            path: entry.path.clone(),
            ok: true,
            reason: None,
        }
    }
}

/// Run integrity checks for all models listed in assets/models.toml.
/// Runs in a blocking thread to avoid starving the async runtime during startup.
pub async fn run_integrity_checks(model_dir: std::path::PathBuf) -> Vec<ModelStatus> {
    task::spawn_blocking(move || {
        let entries = parse_models_toml();
        entries
            .iter()
            .map(|e| verify_file(&model_dir, e))
            .collect()
    })
    .await
    .unwrap_or_else(|e| {
        tracing::error!("model integrity check task panicked: {e}");
        vec![]
    })
}
