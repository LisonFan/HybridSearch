use crate::error::{HybridSearchError, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct DiagnosticLogger {
    path: Option<Arc<PathBuf>>,
}

#[derive(Serialize)]
struct DiagnosticEvent {
    timestamp: String,
    event: String,
    payload: Value,
}

impl DiagnosticLogger {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path: path.map(Arc::new),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref().map(PathBuf::as_path)
    }

    pub fn event(&self, event: &str, payload: Value) {
        let Some(path) = &self.path else {
            return;
        };
        let event = DiagnosticEvent {
            timestamp: Utc::now().to_rfc3339(),
            event: event.to_string(),
            payload: redact_json_value(payload),
        };
        if let Err(error) = write_jsonl_event(path, &event) {
            tracing::warn!(%error, "failed to write diagnostic log");
        }
    }
}

fn write_jsonl_event(path: &Path, event: &DiagnosticEvent) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| {
            HybridSearchError::Provider(format!(
                "failed to create diagnostic log directory: {error}"
            ))
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            HybridSearchError::Provider(format!("failed to open diagnostic log: {error}"))
        })?;
    let line = serde_json::to_string(event).map_err(|error| {
        HybridSearchError::Parse(format!("failed to serialize diagnostic log: {error}"))
    })?;
    writeln!(file, "{line}").map_err(|error| {
        HybridSearchError::Provider(format!("failed to write diagnostic log: {error}"))
    })
}

fn redact_json_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(redact_object(map)),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_json_value).collect()),
        other => other,
    }
}

fn redact_object(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| {
            if is_secret_key(&key) {
                (key, json!("***"))
            } else {
                (key, redact_json_value(value))
            }
        })
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("authorization")
        || key.contains("api_key")
        || key.contains("apikey")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("cookie")
}
