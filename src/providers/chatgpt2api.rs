use crate::error::Result;
use crate::model::{SearchFilters, Source};
use crate::providers::http::send_json;
use reqwest::Client;
use serde_json::{Value, json};

#[derive(Debug)]
pub struct ChatSearchResult {
    pub answer: Option<String>,
    pub sources: Vec<Source>,
}

#[derive(Clone)]
pub struct Chatgpt2apiProvider {
    client: Client,
    endpoint: String,
    api_key: String,
}

impl Chatgpt2apiProvider {
    pub fn new(client: Client, endpoint: String, api_key: String) -> Self {
        Self {
            client,
            endpoint,
            api_key,
        }
    }

    pub async fn search(&self, query: &str, filters: &SearchFilters) -> Result<ChatSearchResult> {
        let prompt = query_with_filters(query, filters);
        let response = send_json(
            self.client
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&json!({ "prompt": prompt })),
            "ChatGPT2API",
        )
        .await?;
        Ok(parse_response(&response))
    }

    pub async fn probe(&self) -> Result<()> {
        let endpoint = self.endpoint.trim_end_matches("/search").to_string() + "/models";
        send_json(
            self.client.get(endpoint).bearer_auth(&self.api_key),
            "ChatGPT2API",
        )
        .await?;
        Ok(())
    }
}

fn query_with_filters(query: &str, filters: &SearchFilters) -> String {
    if filters.is_empty() {
        return query.to_string();
    }
    let mut prompt = query.to_string();
    prompt.push_str("\n\nSearch constraints:");
    if let Some(days) = filters.recency_days {
        prompt.push_str(&format!(
            "\n- Prefer information published within the last {days} days."
        ));
    }
    if !filters.include_domains.is_empty() {
        prompt.push_str(&format!(
            "\n- Prefer only these domains: {}.",
            filters.include_domains.join(", ")
        ));
    }
    if !filters.exclude_domains.is_empty() {
        prompt.push_str(&format!(
            "\n- Exclude these domains: {}.",
            filters.exclude_domains.join(", ")
        ));
    }
    prompt
}

fn parse_response(response: &Value) -> ChatSearchResult {
    let answer = response
        .get("answer")
        .and_then(Value::as_str)
        .map(clean_search_text)
        .filter(|value| !value.trim().is_empty());
    let sources = response
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let url = item.get("url")?.as_str()?.trim();
            if url.is_empty() {
                return None;
            }
            let mut source = Source::new(url, "chatgpt2api");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(snippet) = item.get("snippet").and_then(Value::as_str) {
                source = source.with_description(snippet);
            }
            Some(source)
        })
        .collect();
    ChatSearchResult { answer, sources }
}

fn clean_search_text(text: &str) -> String {
    let mut output = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find('\u{e200}') {
        output.push_str(&remaining[..start]);
        let annotation = &remaining[start + '\u{e200}'.len_utf8()..];
        let Some(end) = annotation.find('\u{e201}') else {
            break;
        };
        let parts: Vec<&str> = annotation[..end].split('\u{e202}').collect();
        let kind = parts.first().copied().unwrap_or_default();
        let readable = if kind == "url" {
            let label = parts.get(1).copied().unwrap_or_default();
            let url = parts.get(2).copied().unwrap_or_default();
            match (label.is_empty(), url.is_empty()) {
                (false, false) => format!("{label} ({url})"),
                (false, true) => label.to_string(),
                (true, false) => url.to_string(),
                (true, true) => String::new(),
            }
        } else {
            parts
                .iter()
                .skip(1)
                .find(|part| {
                    let value = part.trim();
                    !value.is_empty()
                        && !value.starts_with("turn")
                        && !value.starts_with("source")
                        && !value.chars().all(|character| character.is_ascii_digit())
                })
                .copied()
                .unwrap_or_default()
                .to_string()
        };
        output.push_str(&readable);
        remaining = &annotation[end + '\u{e201}'.len_utf8()..];
    }
    output.push_str(remaining);
    output.trim().to_string()
}
