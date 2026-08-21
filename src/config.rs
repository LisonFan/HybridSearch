use crate::error::{HybridSearchError, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub const SOURCE_PROVIDER_NAMES: [&str; 4] = ["tavily", "firecrawl", "tinyfish", "exa"];

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
    pub source_providers: Vec<String>,
    pub source_providers_explicit: bool,
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
    pub source_max_answers: usize,
    pub debug_log_path: Option<PathBuf>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn status<T>(value: &Option<T>) -> &'static str {
            if value.is_some() { "set" } else { "unset" }
        }
        let endpoint = |value: &str| self.redact_text(&diagnostic_endpoint(value));

        formatter
            .debug_struct("Config")
            .field(
                "chatgpt2api_api_url",
                &self.chatgpt2api_api_url.as_deref().map(&endpoint),
            )
            .field("chatgpt2api_api_key", &status(&self.chatgpt2api_api_key))
            .field("tavily_api_url", &endpoint(&self.tavily_api_url))
            .field("tavily_api_key", &status(&self.tavily_api_key))
            .field("firecrawl_api_url", &endpoint(&self.firecrawl_api_url))
            .field("firecrawl_api_key", &status(&self.firecrawl_api_key))
            .field(
                "tinyfish_search_api_url",
                &endpoint(&self.tinyfish_search_api_url),
            )
            .field(
                "tinyfish_fetch_api_url",
                &endpoint(&self.tinyfish_fetch_api_url),
            )
            .field("tinyfish_api_key", &status(&self.tinyfish_api_key))
            .field("exa_api_url", &endpoint(&self.exa_api_url))
            .field("exa_api_key", &status(&self.exa_api_key))
            .field("github_token", &status(&self.github_token))
            .field("source_providers", &self.source_providers)
            .field("source_providers_explicit", &self.source_providers_explicit)
            .field("timeout", &self.timeout)
            .field("default_extra_sources", &self.default_extra_sources)
            .field("fallback_sources", &self.fallback_sources)
            .field("cache_size", &self.cache_size)
            .field("fetch_max_chars", &self.fetch_max_chars)
            .field("response_max_chars", &self.response_max_chars)
            .field("enrich_concurrency", &self.enrich_concurrency)
            .field("enrich_max_chars", &self.enrich_max_chars)
            .field("max_inline_sources", &self.max_inline_sources)
            .field("github_max_comments", &self.github_max_comments)
            .field("source_max_answers", &self.source_max_answers)
            .field("debug_log_path", &self.debug_log_path)
            .finish()
    }
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
        let configured_source_providers = optional(&values, "HYBRID_SEARCH_SOURCE_PROVIDERS");
        let source_providers_explicit = configured_source_providers.is_some();
        let source_providers = configured_source_providers
            .map(parse_source_providers)
            .transpose()?
            .unwrap_or_else(|| {
                SOURCE_PROVIDER_NAMES
                    .iter()
                    .map(|name| name.to_string())
                    .collect()
            });

        let chat_complete = chat_url.is_some() && chat_key.is_some();
        if chat_url.is_some() != chat_key.is_some() {
            tracing::warn!(
                "ChatGPT2API is disabled because CHATGPT2API_API_URL and CHATGPT2API_API_KEY must both be set"
            );
        }
        let source_configured = source_providers
            .iter()
            .any(|provider| match provider.as_str() {
                "tavily" => tavily_key.is_some(),
                "firecrawl" => firecrawl_key.is_some(),
                "tinyfish" => tinyfish_key.is_some(),
                "exa" => exa_key.is_some(),
                _ => false,
            });
        if !chat_complete && !source_configured {
            return Err(HybridSearchError::MissingConfig(
                "configure CHATGPT2API_API_URL + CHATGPT2API_API_KEY or enable a configured TAVILY_API_KEY, FIRECRAWL_API_KEY, TINYFISH_API_KEY, or EXA_API_KEY"
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
            source_providers,
            source_providers_explicit,
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
            source_max_answers: positive_usize(&values, "HYBRID_SEARCH_SOURCE_MAX_ANSWERS", 5),
            debug_log_path: optional(&values, "HYBRID_SEARCH_LOG_PATH").map(PathBuf::from),
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

    pub fn effective_provider_order(&self) -> Vec<&'static str> {
        let mut providers = Vec::new();
        if self.chatgpt2api_api_url.is_some() && self.chatgpt2api_api_key.is_some() {
            providers.push("chatgpt2api");
        }
        for provider in &self.source_providers {
            if self.provider_configured(provider) {
                providers.push(match provider.as_str() {
                    "tavily" => "tavily",
                    "firecrawl" => "firecrawl",
                    "tinyfish" => "tinyfish",
                    "exa" => "exa",
                    _ => continue,
                });
            }
        }
        providers
    }

    pub fn source_provider_enabled(&self, provider: &str) -> bool {
        self.source_providers.iter().any(|name| name == provider)
    }

    pub fn provider_configured(&self, provider: &str) -> bool {
        match provider {
            "chatgpt2api" => {
                self.chatgpt2api_api_url.is_some() && self.chatgpt2api_api_key.is_some()
            }
            "tavily" => self.tavily_api_key.is_some(),
            "firecrawl" => self.firecrawl_api_key.is_some(),
            "tinyfish" => self.tinyfish_api_key.is_some(),
            "exa" => self.exa_api_key.is_some(),
            _ => false,
        }
    }

    pub fn redact_text(&self, value: &str) -> String {
        let mut redacted = value.to_string();
        for secret in [
            self.chatgpt2api_api_key.as_deref(),
            self.tavily_api_key.as_deref(),
            self.firecrawl_api_key.as_deref(),
            self.tinyfish_api_key.as_deref(),
            self.exa_api_key.as_deref(),
            self.github_token.as_deref(),
        ]
        .into_iter()
        .flatten()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        {
            redacted = redacted.replace(secret, "***");
        }
        redacted
    }
}

fn parse_source_providers(value: String) -> Result<Vec<String>> {
    let mut providers = Vec::new();
    for provider in value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let provider = provider.to_ascii_lowercase();
        if !SOURCE_PROVIDER_NAMES.contains(&provider.as_str()) {
            return Err(HybridSearchError::InvalidParams(format!(
                "unknown provider '{provider}' in HYBRID_SEARCH_SOURCE_PROVIDERS; valid values: {}",
                SOURCE_PROVIDER_NAMES.join(", ")
            )));
        }
        if !providers.contains(&provider) {
            providers.push(provider);
        }
    }
    if providers.is_empty() {
        return Err(HybridSearchError::InvalidParams(
            "HYBRID_SEARCH_SOURCE_PROVIDERS must contain at least one provider".to_string(),
        ));
    }
    Ok(providers)
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

fn diagnostic_endpoint(value: &str) -> String {
    let Ok(mut url) = url::Url::parse(value) else {
        return "<invalid endpoint>".to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}
