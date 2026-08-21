use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    pub recency_days: Option<u32>,
    pub include_domains: Vec<String>,
    pub exclude_domains: Vec<String>,
}

impl SearchFilters {
    pub fn is_empty(&self) -> bool {
        self.recency_days.is_none()
            && self.include_domains.is_empty()
            && self.exclude_domains.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    pub url: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl Source {
    pub fn new(url: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            provider: provider.into(),
            title: None,
            description: None,
            published_date: None,
            content: None,
        }
    }

    pub fn with_title(mut self, value: impl Into<String>) -> Self {
        let value = value.into();
        if !value.trim().is_empty() {
            self.title = Some(value);
        }
        self
    }

    pub fn with_description(mut self, value: impl Into<String>) -> Self {
        let value = value.into();
        if !value.trim().is_empty() {
            self.description = Some(value);
        }
        self
    }

    pub fn with_published_date(mut self, value: impl Into<String>) -> Self {
        let value = value.into();
        if !value.trim().is_empty() {
            self.published_date = Some(value);
        }
        self
    }
}

#[derive(Debug, Clone)]
pub struct FetchedPage {
    pub content: String,
    pub title: Option<String>,
    pub published_date: Option<String>,
}

pub fn merge_sources(primary: Vec<Source>, secondary: Vec<Source>) -> Vec<Source> {
    let mut seen = HashSet::new();
    primary
        .into_iter()
        .chain(secondary)
        .filter(|source| {
            let url = source.url.trim();
            !url.is_empty() && seen.insert(url.to_string())
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ResponseFormat {
    Concise,
    #[default]
    Detailed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchProvider {
    Chatgpt2api,
    Tavily,
    Firecrawl,
    Tinyfish,
    Exa,
}

impl SearchProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chatgpt2api => "chatgpt2api",
            Self::Tavily => "tavily",
            Self::Firecrawl => "firecrawl",
            Self::Tinyfish => "tinyfish",
            Self::Exa => "exa",
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WebSearchInput {
    /// Search query.
    pub query: String,
    /// Run only this provider and do not fall back to another provider.
    #[serde(default)]
    pub provider: Option<SearchProvider>,
    /// Number of supplemental sources. With a selected source provider, this is its result limit.
    #[serde(default)]
    pub extra_sources: Option<usize>,
    /// Include extracted page content in the first sources.
    #[serde(default)]
    pub include_content: Option<bool>,
    /// concise omits inline page content; detailed includes it.
    #[serde(default)]
    pub response_format: Option<ResponseFormat>,
    /// Prefer results published within the last N days.
    #[serde(default)]
    pub recency_days: Option<u32>,
    /// Restrict supported source providers to these domains.
    #[serde(default)]
    pub include_domains: Vec<String>,
    /// Exclude these domains from supported source providers.
    #[serde(default)]
    pub exclude_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebSearchOutput {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    pub sources_count: usize,
    pub sources: Vec<Source>,
    pub search_provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supplemental_provider: Option<String>,
    pub fallback_used: bool,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetSourcesInput {
    pub session_id: String,
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetSourcesOutput {
    pub session_id: String,
    pub sources_count: usize,
    pub sources: Vec<Source>,
    pub total_sources: usize,
    pub offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WebFetchInput {
    pub url: String,
    #[serde(default)]
    pub max_chars: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebFetchOutput {
    pub url: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    pub original_length: usize,
    pub truncated: bool,
    pub source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WebMapInput {
    pub url: String,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebMapOutput {
    pub sources_count: usize,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProviderHealth {
    pub provider: String,
    pub enabled: bool,
    pub configured: bool,
    pub endpoints: Vec<String>,
    pub credential: String,
    pub reachable: bool,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DoctorOutput {
    pub status: String,
    pub configuration_source: String,
    pub provider_order: Vec<String>,
    pub source_provider_order: Vec<String>,
    pub providers: Vec<ProviderHealth>,
    pub github_token_configured: bool,
    pub timeout_seconds: u64,
    pub cache_size: usize,
    pub response_max_chars: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_log_path: Option<String>,
}
