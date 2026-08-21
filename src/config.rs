use crate::error::{HybridSearchError, Result};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone)]
pub struct Config {
    pub chatgpt2api_api_url: Option<String>,
    pub chatgpt2api_api_key: Option<String>,
    pub tavily_api_url: String,
    pub tavily_api_key: Option<String>,
    pub firecrawl_api_url: String,
    pub firecrawl_api_key: Option<String>,
    pub tinyfish_search_api_url: String,
    pub tinyfish_fetch_api_url: String,
    pub tinyfish_api_key: Option<String>,
    pub exa_api_url: String,
    pub exa_api_key: Option<String>,
    pub github_token: Option<String>,
    pub timeout: Duration,
    pub default_extra_sources: usize,
    pub fallback_sources: usize,
    pub cache_size: usize,
    pub fetch_max_chars: Option<usize>,
    pub response_max_chars: usize,
    pub enrich_concurrency: usize,
    pub enrich_max_chars: usize,
    pub max_inline_sources: usize,
    pub github_max_comments: usize,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_map(std::env::vars())
    }

    pub fn from_map<I, K, V>(values: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let values: HashMap<String, String> = values
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        let chat_url = optional(&values, "CHATGPT2API_API_URL").map(normalize_chat_endpoint);
        let chat_key = optional(&values, "CHATGPT2API_API_KEY");
        let tavily_key = optional(&values, "TAVILY_API_KEY");
        let firecrawl_key = optional(&values, "FIRECRAWL_API_KEY");
        let tinyfish_key = optional(&values, "TINYFISH_API_KEY");
        let exa_key = optional(&values, "EXA_API_KEY");

        let chat_complete = chat_url.is_some() && chat_key.is_some();
        if chat_url.is_some() != chat_key.is_some() {
            tracing::warn!(
                "ChatGPT2API is disabled because CHATGPT2API_API_URL and CHATGPT2API_API_KEY must both be set"
            );
        }
        if !chat_complete
            && tavily_key.is_none()
            && firecrawl_key.is_none()
            && tinyfish_key.is_none()
            && exa_key.is_none()
        {
            return Err(HybridSearchError::MissingConfig(
                "configure CHATGPT2API_API_URL + CHATGPT2API_API_KEY, TAVILY_API_KEY, FIRECRAWL_API_KEY, TINYFISH_API_KEY, or EXA_API_KEY"
                    .to_string(),
            ));
        }

        Ok(Self {
            chatgpt2api_api_url: if chat_complete { chat_url } else { None },
            chatgpt2api_api_key: if chat_complete { chat_key } else { None },
            tavily_api_url: value(&values, "TAVILY_API_URL", "https://api.tavily.com"),
            tavily_api_key: tavily_key,
            firecrawl_api_url: value(&values, "FIRECRAWL_API_URL", "https://api.firecrawl.dev/v1"),
            firecrawl_api_key: firecrawl_key,
            tinyfish_search_api_url: value(
                &values,
                "TINYFISH_SEARCH_API_URL",
                "https://api.search.tinyfish.ai",
            ),
            tinyfish_fetch_api_url: value(
                &values,
                "TINYFISH_FETCH_API_URL",
                "https://api.fetch.tinyfish.ai",
            ),
            tinyfish_api_key: tinyfish_key,
            exa_api_url: value(&values, "EXA_API_URL", "https://api.exa.ai"),
            exa_api_key: exa_key,
            github_token: optional(&values, "GITHUB_TOKEN"),
            timeout: Duration::from_secs(positive_u64(
                &values,
                "HYBRID_SEARCH_TIMEOUT_SECONDS",
                300,
            )),
            default_extra_sources: usize_value(&values, "HYBRID_SEARCH_EXTRA_SOURCES", 3),
            fallback_sources: usize_value(&values, "HYBRID_SEARCH_FALLBACK_SOURCES", 5),
            cache_size: positive_usize(&values, "HYBRID_SEARCH_CACHE_SIZE", 256),
            fetch_max_chars: optional_positive_usize(&values, "HYBRID_SEARCH_FETCH_MAX_CHARS"),
            response_max_chars: positive_usize(&values, "HYBRID_SEARCH_RESPONSE_MAX_CHARS", 45_000),
            enrich_concurrency: positive_usize(&values, "HYBRID_SEARCH_ENRICH_CONCURRENCY", 3)
                .clamp(1, 8),
            enrich_max_chars: positive_usize(&values, "HYBRID_SEARCH_ENRICH_MAX_CHARS", 15_000),
            max_inline_sources: usize_value(&values, "HYBRID_SEARCH_MAX_INLINE_SOURCES", 5),
            github_max_comments: positive_usize(&values, "HYBRID_SEARCH_GITHUB_MAX_COMMENTS", 30),
        })
    }

    pub fn configured_providers(&self) -> Vec<&'static str> {
        let mut providers = Vec::new();
        if self.chatgpt2api_api_url.is_some() && self.chatgpt2api_api_key.is_some() {
            providers.push("chatgpt2api");
        }
        if self.tavily_api_key.is_some() {
            providers.push("tavily");
        }
        if self.firecrawl_api_key.is_some() {
            providers.push("firecrawl");
        }
        if self.tinyfish_api_key.is_some() {
            providers.push("tinyfish");
        }
        if self.exa_api_key.is_some() {
            providers.push("exa");
        }
        providers
    }
}

fn optional(values: &HashMap<String, String>, key: &str) -> Option<String> {
    values
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn value(values: &HashMap<String, String>, key: &str, default: &str) -> String {
    optional(values, key)
        .unwrap_or_else(|| default.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn positive_u64(values: &HashMap<String, String>, key: &str, default: u64) -> u64 {
    optional(values, key)
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn usize_value(values: &HashMap<String, String>, key: &str, default: usize) -> usize {
    optional(values, key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn positive_usize(values: &HashMap<String, String>, key: &str, default: usize) -> usize {
    usize_value(values, key, default).max(1)
}

fn optional_positive_usize(values: &HashMap<String, String>, key: &str) -> Option<usize> {
    optional(values, key)
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
}

fn normalize_chat_endpoint(value: String) -> String {
    let value = value.trim_end_matches('/');
    if value.ends_with("/v1/search") {
        value.to_string()
    } else if value.ends_with("/v1") {
        format!("{value}/search")
    } else {
        format!("{value}/v1/search")
    }
}
