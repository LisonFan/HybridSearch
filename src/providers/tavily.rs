use crate::error::{HybridSearchError, Result};
use crate::model::{FetchedPage, SearchFilters, Source};
use crate::providers::SourceProvider;
use crate::providers::http::send_json_with_status;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct KeyRing {
    keys: Vec<String>,
    cursor: AtomicUsize,
}

impl KeyRing {
    fn new(value: &str) -> Self {
        let mut keys: Vec<String> = value
            .split(',')
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(str::to_string)
            .collect();
        if keys.is_empty() {
            keys.push(value.to_string());
        }
        Self {
            keys,
            cursor: AtomicUsize::new(0),
        }
    }
}

#[derive(Clone)]
pub struct TavilyProvider {
    client: Client,
    api_url: String,
    keys: Arc<KeyRing>,
}

impl TavilyProvider {
    pub fn new(client: Client, api_url: String, api_key: String) -> Self {
        Self {
            client,
            api_url: api_url.trim_end_matches('/').to_string(),
            keys: Arc::new(KeyRing::new(&api_key)),
        }
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let endpoint = format!("{}/{}", self.api_url, path.trim_start_matches('/'));
        let start = self.keys.cursor.fetch_add(1, Ordering::Relaxed) % self.keys.keys.len();
        for offset in 0..self.keys.keys.len() {
            let key = &self.keys.keys[(start + offset) % self.keys.keys.len()];
            match send_json_with_status(
                self.client.post(&endpoint).bearer_auth(key).json(body),
                "Tavily",
            )
            .await
            {
                Ok(value) => return Ok(value),
                Err(failure)
                    if failure.status.is_some_and(|status| {
                        matches!(status.as_u16(), 401 | 403 | 429 | 432 | 433)
                    }) && offset + 1 < self.keys.keys.len() =>
                {
                    tracing::warn!("Tavily key was rejected or rate limited; rotating key");
                }
                Err(failure) => return Err(failure.error),
            }
        }
        Err(HybridSearchError::Provider(
            "Tavily request failed".to_string(),
        ))
    }
}

#[async_trait]
impl SourceProvider for TavilyProvider {
    fn name(&self) -> &'static str {
        "tavily"
    }

    fn supports_filters(&self) -> bool {
        true
    }

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        let mut body = json!({
            "query": query,
            "max_results": max_results,
            "include_answer": false
        });
        let object = body.as_object_mut().expect("JSON object");
        if let Some(days) = filters.recency_days {
            object.insert("days".to_string(), json!(days));
            object.insert("topic".to_string(), json!("news"));
        }
        if !filters.include_domains.is_empty() {
            object.insert(
                "include_domains".to_string(),
                json!(filters.include_domains),
            );
        }
        if !filters.exclude_domains.is_empty() {
            object.insert(
                "exclude_domains".to_string(),
                json!(filters.exclude_domains),
            );
        }
        let response = self.post("search", &body).await?;
        Ok(normalize_results(&response))
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        let response = self
            .post("extract", &json!({ "urls": [url], "format": "markdown" }))
            .await?;
        let result = response
            .get("results")
            .and_then(Value::as_array)
            .and_then(|items| items.first());
        let content = result
            .and_then(|item| item.get("raw_content").or_else(|| item.get("content")))
            .and_then(Value::as_str)
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| {
                HybridSearchError::Provider("Tavily extract returned empty content".to_string())
            })?;
        Ok(FetchedPage {
            content: content.to_string(),
            title: result
                .and_then(|item| item.get("title"))
                .and_then(Value::as_str)
                .map(str::to_string),
            published_date: None,
        })
    }

    async fn map(&self, url: &str, max_results: usize) -> Result<Vec<Source>> {
        let response = self
            .post(
                "map",
                &json!({ "url": url, "max_depth": 1, "limit": max_results }),
            )
            .await?;
        let mut sources = normalize_results(&response);
        sources.truncate(max_results);
        Ok(sources)
    }
}

fn normalize_results(response: &Value) -> Vec<Source> {
    response
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if let Some(url) = item.as_str() {
                return Some(Source::new(url, "tavily"));
            }
            if item
                .get("score")
                .and_then(Value::as_f64)
                .is_some_and(|score| score < 0.1)
            {
                return None;
            }
            let mut source = Source::new(item.get("url")?.as_str()?, "tavily");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(description) = item
                .get("content")
                .or_else(|| item.get("description"))
                .and_then(Value::as_str)
            {
                source = source.with_description(description);
            }
            if let Some(date) = item.get("published_date").and_then(Value::as_str) {
                source = source.with_published_date(date);
            }
            Some(source)
        })
        .collect()
}
