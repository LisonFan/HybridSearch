use crate::error::{HybridSearchError, Result};
use crate::model::{FetchedPage, SearchFilters, Source};
use crate::providers::SourceProvider;
use crate::providers::http::{get_json, send_json};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct KeenableProvider {
    client: Client,
    api_url: String,
    api_key: String,
}

impl KeenableProvider {
    pub fn new(client: Client, api_url: String, api_key: String) -> Self {
        Self {
            client,
            api_url: api_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        send_json(
            self.client
                .post(format!("{}/{}", self.api_url, path.trim_start_matches('/')))
                .header("X-API-Key", &self.api_key)
                .json(body),
            "Keenable",
        )
        .await
    }

    async fn get(&self, path: &str, url: &str) -> Result<Value> {
        get_json(
            self.client
                .get(format!("{}/{}", self.api_url, path.trim_start_matches('/')))
                .header("X-API-Key", &self.api_key)
                .query(&[("url", url), ("live", "true")]),
            "Keenable",
        )
        .await
    }
}

#[async_trait]
impl SourceProvider for KeenableProvider {
    fn name(&self) -> &'static str {
        "keenable"
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
        if !filters.exclude_domains.is_empty() {
            return Err(HybridSearchError::InvalidParams(
                "Keenable does not support exclude_domains".to_string(),
            ));
        }
        if filters.include_domains.len() > 1 {
            return Err(HybridSearchError::InvalidParams(
                "Keenable supports at most one include_domains entry".to_string(),
            ));
        }

        let mut body = json!({
            "query": query,
            "max_results": max_results.clamp(1, 50)
        });
        let object = body.as_object_mut().expect("JSON object");
        if let Some(site) = filters.include_domains.first() {
            object.insert("site".to_string(), json!(site));
        }
        if let Some(days) = filters.recency_days {
            object.insert("published_after".to_string(), json!(format!("{days}d")));
        }

        let response = self.post("v1/search", &body).await?;
        Ok(normalize_results(&response))
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        let response = self.get("v1/fetch", url).await?;
        let content = response
            .get("content")
            .and_then(Value::as_str)
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| {
                HybridSearchError::Provider("Keenable fetch returned empty content".to_string())
            })?;
        let published_date = response.get("published_at").and_then(|value| {
            value.as_str().map(str::to_string).or_else(|| {
                value
                    .as_i64()
                    .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
                    .map(|date| date.to_rfc3339())
            })
        });

        Ok(FetchedPage {
            content: content.to_string(),
            title: response
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string),
            published_date,
        })
    }
}

fn normalize_results(response: &Value) -> Vec<Source> {
    response
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let mut source = Source::new(item.get("url")?.as_str()?, "keenable");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(description) = item
                .get("snippet")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .or_else(|| item.get("description").and_then(Value::as_str))
            {
                source = source.with_description(description);
            }
            if let Some(date) = item.get("published_at").and_then(Value::as_str) {
                source = source.with_published_date(date);
            }
            Some(source)
        })
        .collect()
}
