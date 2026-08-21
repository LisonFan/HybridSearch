pub mod arxiv;
pub mod github;
pub mod stackexchange;
pub mod wikipedia;

use crate::error::{HybridSearchError, Result};
use reqwest::Client;
use url::Url;

pub struct SpecialistPage {
    pub content: String,
    pub source_type: &'static str,
}

pub async fn fetch(
    client: &Client,
    url: &Url,
    github_token: Option<&str>,
    github_max_comments: usize,
    source_max_answers: usize,
) -> Result<Option<SpecialistPage>> {
    if let Some(page) = github::fetch(client, url, github_token, github_max_comments).await? {
        return Ok(Some(page));
    }
    if stackexchange::matches(url) {
        return stackexchange::fetch(client, url, source_max_answers)
            .await
            .map(Some);
    }
    if arxiv::matches(url) {
        return arxiv::fetch(client, url).await.map(Some);
    }
    if wikipedia::matches(url) {
        return wikipedia::fetch(client, url).await.map(Some);
    }
    Ok(None)
}

pub(crate) async fn get_json(client: &Client, url: &str, label: &str) -> Result<serde_json::Value> {
    let bytes = get_bytes(client, url, label).await?;
    serde_json::from_slice(&bytes)
        .map_err(|error| HybridSearchError::Parse(format!("invalid {label} JSON: {error}")))
}

pub(crate) async fn get_text(client: &Client, url: &str, label: &str) -> Result<String> {
    let bytes = get_bytes(client, url, label).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn get_bytes(client: &Client, url: &str, label: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            "HybridSearch/0.1 (https://github.com/LisonFan/HybridSearch)",
        )
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                HybridSearchError::Timeout(format!("{label} request"))
            } else {
                HybridSearchError::Provider(format!("{label} request failed: {error}"))
            }
        })?;
    let status = response.status();
    let body = response.bytes().await.map_err(|error| {
        HybridSearchError::Provider(format!("{label} response read failed: {error}"))
    })?;
    if !status.is_success() {
        return Err(HybridSearchError::Provider(format!(
            "{label} returned HTTP {status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    Ok(body.to_vec())
}
