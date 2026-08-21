use crate::error::{HybridSearchError, Result};
use reqwest::{RequestBuilder, StatusCode};
use serde_json::Value;

pub struct HttpFailure {
    pub status: Option<StatusCode>,
    pub error: HybridSearchError,
}

pub async fn send_json(request: RequestBuilder, label: &str) -> Result<Value> {
    send_json_with_status(request, label)
        .await
        .map_err(|failure| failure.error)
}

pub async fn send_json_with_status(
    request: RequestBuilder,
    label: &str,
) -> std::result::Result<Value, HttpFailure> {
    let response = request.send().await.map_err(|error| HttpFailure {
        status: None,
        error: if error.is_timeout() {
            HybridSearchError::Timeout(format!("{label} request"))
        } else {
            HybridSearchError::Provider(format!("{label} request failed: {error}"))
        },
    })?;
    let status = response.status();
    let body = response.bytes().await.map_err(|error| HttpFailure {
        status: None,
        error: HybridSearchError::Provider(format!("{label} response read failed: {error}")),
    })?;

    if !status.is_success() {
        return Err(HttpFailure {
            status: Some(status),
            error: HybridSearchError::Provider(format!(
                "{label} returned HTTP {status}: {}",
                String::from_utf8_lossy(&body)
            )),
        });
    }

    serde_json::from_slice(&body).map_err(|error| HttpFailure {
        status: Some(status),
        error: HybridSearchError::Parse(format!("invalid {label} JSON: {error}")),
    })
}

pub async fn get_json(request: RequestBuilder, label: &str) -> Result<Value> {
    send_json(request, label).await
}
