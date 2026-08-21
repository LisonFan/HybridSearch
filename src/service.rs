use crate::cache::SourceCache;
use crate::config::Config;
use crate::error::{HybridSearchError, Result};
use crate::model::{
    DoctorOutput, FetchedPage, GetSourcesOutput, ProviderHealth, ResponseFormat, SearchFilters,
    Source, WebFetchOutput, WebMapOutput, WebSearchInput, WebSearchOutput, merge_sources,
};
use crate::providers::{
    Chatgpt2apiProvider, ExaProvider, FirecrawlProvider, SharedSourceProvider, TavilyProvider,
    TinyfishProvider, build_http_client,
};
use futures::{StreamExt, stream};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Instant, timeout_at};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct SearchService {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    config: Config,
    client: reqwest::Client,
    chatgpt2api: Option<Chatgpt2apiProvider>,
    source_providers: Vec<SharedSourceProvider>,
    cache: Mutex<SourceCache>,
}

struct ChainResult {
    sources: Vec<Source>,
    provider: Option<String>,
    notes: Vec<String>,
}

impl SearchService {
    pub fn new(config: Config) -> Result<Self> {
        let client = build_http_client(config.timeout)?;
        let chatgpt2api = config
            .chatgpt2api_api_url
            .clone()
            .zip(config.chatgpt2api_api_key.clone())
            .map(|(endpoint, key)| Chatgpt2apiProvider::new(client.clone(), endpoint, key));
        let mut source_providers: Vec<SharedSourceProvider> = Vec::new();
        if let Some(key) = &config.tavily_api_key {
            source_providers.push(Arc::new(TavilyProvider::new(
                client.clone(),
                config.tavily_api_url.clone(),
                key.clone(),
            )));
        }
        if let Some(key) = &config.firecrawl_api_key {
            source_providers.push(Arc::new(FirecrawlProvider::new(
                client.clone(),
                config.firecrawl_api_url.clone(),
                key.clone(),
            )));
        }
        if let Some(key) = &config.tinyfish_api_key {
            source_providers.push(Arc::new(TinyfishProvider::new(
                client.clone(),
                config.tinyfish_search_api_url.clone(),
                config.tinyfish_fetch_api_url.clone(),
                key.clone(),
            )));
        }
        if let Some(key) = &config.exa_api_key {
            source_providers.push(Arc::new(ExaProvider::new(
                client.clone(),
                config.exa_api_url.clone(),
                key.clone(),
            )));
        }

        Ok(Self {
            inner: Arc::new(ServiceInner {
                cache: Mutex::new(SourceCache::new(config.cache_size)),
                config,
                client,
                chatgpt2api,
                source_providers,
            }),
        })
    }

    pub async fn web_search(&self, input: WebSearchInput) -> Result<WebSearchOutput> {
        let query = input.query.trim();
        if query.is_empty() {
            return Err(HybridSearchError::InvalidParams(
                "web_search.query must not be empty".to_string(),
            ));
        }
        if input.recency_days == Some(0) {
            return Err(HybridSearchError::InvalidParams(
                "web_search.recency_days must be greater than zero".to_string(),
            ));
        }
        let filters = SearchFilters {
            recency_days: input.recency_days,
            include_domains: normalize_domains(input.include_domains),
            exclude_domains: normalize_domains(input.exclude_domains),
        };
        let include_content = match input.response_format {
            Some(ResponseFormat::Concise) => false,
            Some(ResponseFormat::Detailed) => true,
            None => input.include_content.unwrap_or(true),
        };
        let deadline = Instant::now() + self.inner.config.timeout;
        let mut notes = Vec::new();
        let mut answer = None;
        let mut primary_sources = Vec::new();
        let mut primary_usable = false;

        if let Some(provider) = &self.inner.chatgpt2api {
            match timeout_at(deadline, provider.search(query, &filters)).await {
                Ok(Ok(result)) if !result.sources.is_empty() => {
                    answer = result.answer;
                    primary_sources = merge_sources(Vec::new(), result.sources);
                    primary_usable = true;
                }
                Ok(Ok(_)) => notes.push("chatgpt2api returned no sources".to_string()),
                Ok(Err(error)) => notes.push(format!("chatgpt2api: {error}")),
                Err(_) => notes.push("chatgpt2api: request deadline reached".to_string()),
            }
        }

        let requested_extra = input
            .extra_sources
            .unwrap_or(self.inner.config.default_extra_sources);
        let source_count = if primary_usable {
            requested_extra
        } else {
            self.inner.config.fallback_sources
        };
        let chain = if source_count == 0 {
            ChainResult {
                sources: Vec::new(),
                provider: None,
                notes: Vec::new(),
            }
        } else {
            self.search_source_chain(query, source_count, &filters, deadline)
                .await
        };
        notes.extend(chain.notes);

        if !primary_usable && chain.sources.is_empty() {
            return Err(HybridSearchError::Provider(format!(
                "all configured search providers failed: {}",
                if notes.is_empty() {
                    "no usable sources".to_string()
                } else {
                    notes.join("; ")
                }
            )));
        }

        let search_provider = if primary_usable {
            "chatgpt2api".to_string()
        } else {
            chain
                .provider
                .clone()
                .unwrap_or_else(|| "source_chain".to_string())
        };
        let supplemental_provider = primary_usable.then_some(chain.provider).flatten();
        let fallback_used = self.inner.chatgpt2api.is_some() && !primary_usable;
        let mut sources = merge_sources(primary_sources, chain.sources);
        if include_content {
            sources = self.enrich_sources(sources, deadline).await;
        }

        let session_id = Uuid::new_v4().to_string();
        let full_sources = Arc::new(sources);
        let sources_count = full_sources.len();
        self.inner
            .cache
            .lock()
            .await
            .insert(session_id.clone(), full_sources.clone());
        let mut response_sources = (*full_sources).clone();
        let truncated = apply_response_budget(
            &mut answer,
            &mut response_sources,
            self.inner.config.response_max_chars,
        );

        Ok(WebSearchOutput {
            session_id,
            answer,
            sources_count,
            sources: response_sources,
            search_provider,
            supplemental_provider,
            fallback_used,
            truncated,
        })
    }

    pub async fn get_sources(
        &self,
        session_id: &str,
        offset: usize,
        limit: Option<usize>,
    ) -> Result<GetSourcesOutput> {
        let cached = self
            .inner
            .cache
            .lock()
            .await
            .get(session_id)
            .ok_or_else(|| HybridSearchError::NotFound(format!("session_id={session_id}")))?;
        let total_sources = cached.len();
        let start = offset.min(total_sources);
        let end = limit
            .map(|limit| start.saturating_add(limit))
            .unwrap_or(total_sources)
            .min(total_sources);
        let mut sources = cached[start..end].to_vec();
        let mut no_answer = None;
        let truncated = apply_response_budget(
            &mut no_answer,
            &mut sources,
            self.inner.config.response_max_chars,
        );
        let next_offset = (start + sources.len() < total_sources).then_some(start + sources.len());
        Ok(GetSourcesOutput {
            session_id: session_id.to_string(),
            sources_count: sources.len(),
            sources,
            total_sources,
            offset: start,
            next_offset,
            truncated,
        })
    }

    pub async fn web_fetch(&self, url: &str, max_chars: Option<usize>) -> Result<WebFetchOutput> {
        let parsed = Url::parse(url)
            .map_err(|error| HybridSearchError::InvalidParams(format!("invalid URL: {error}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(HybridSearchError::InvalidParams(
                "web_fetch.url must use http or https".to_string(),
            ));
        }
        let deadline = Instant::now() + self.inner.config.timeout;
        let (page, source_type, fallback_reason) = self.fetch_page(&parsed, deadline).await?;
        let original_length = page.content.chars().count();
        let limit = max_chars.or(self.inner.config.fetch_max_chars);
        let (content, truncated) = truncate_text(page.content, limit);
        Ok(WebFetchOutput {
            url: parsed.to_string(),
            content,
            title: page.title,
            published_date: page.published_date,
            original_length,
            truncated,
            source_type,
            fallback_reason,
        })
    }

    pub async fn web_map(&self, url: &str, max_results: usize) -> Result<WebMapOutput> {
        let provider = self
            .inner
            .source_providers
            .iter()
            .find(|provider| provider.name() == "tavily")
            .ok_or_else(|| HybridSearchError::MissingConfig("TAVILY_API_KEY".to_string()))?;
        let deadline = Instant::now() + self.inner.config.timeout;
        let sources = timeout_at(deadline, provider.map(url, max_results.max(1)))
            .await
            .map_err(|_| HybridSearchError::Timeout("Tavily Map request".to_string()))??;
        Ok(WebMapOutput {
            sources_count: sources.len(),
            sources,
        })
    }

    pub async fn doctor(&self) -> DoctorOutput {
        let mut probes = Vec::new();
        if let Some(provider) = self.inner.chatgpt2api.clone() {
            let timeout = self.inner.config.timeout;
            probes.push(tokio::spawn(async move {
                let result = tokio::time::timeout(timeout, provider.probe()).await;
                health("chatgpt2api", result)
            }));
        }
        for provider in &self.inner.source_providers {
            let provider = provider.clone();
            let timeout = self.inner.config.timeout;
            probes.push(tokio::spawn(async move {
                let name = provider.name();
                let filters = SearchFilters::default();
                let result = tokio::time::timeout(
                    timeout,
                    provider.search("Rust programming language", 1, &filters),
                )
                .await
                .map(|result| result.map(|_| ()));
                health(name, result)
            }));
        }
        let mut providers = Vec::new();
        for probe in probes {
            match probe.await {
                Ok(health) => providers.push(health),
                Err(error) => providers.push(ProviderHealth {
                    provider: "unknown".to_string(),
                    reachable: false,
                    detail: error.to_string(),
                }),
            }
        }
        let all_reachable = providers.iter().all(|provider| provider.reachable);
        DoctorOutput {
            status: if all_reachable { "ok" } else { "degraded" }.to_string(),
            provider_order: self
                .inner
                .config
                .configured_providers()
                .into_iter()
                .map(str::to_string)
                .collect(),
            providers,
            github_token_configured: self.inner.config.github_token.is_some(),
            timeout_seconds: self.inner.config.timeout.as_secs(),
        }
    }

    async fn search_source_chain(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
        deadline: Instant,
    ) -> ChainResult {
        let mut notes = Vec::new();
        for provider in &self.inner.source_providers {
            if !filters.is_empty() && !provider.supports_filters() {
                notes.push(format!(
                    "{} skipped because it cannot enforce domain or recency filters",
                    provider.name()
                ));
                continue;
            }
            match timeout_at(deadline, provider.search(query, max_results, filters)).await {
                Ok(Ok(sources)) => {
                    let mut sources = merge_sources(Vec::new(), sources);
                    sources.truncate(max_results);
                    if !sources.is_empty() {
                        return ChainResult {
                            sources,
                            provider: Some(provider.name().to_string()),
                            notes,
                        };
                    }
                    notes.push(format!("{} returned no sources", provider.name()));
                }
                Ok(Err(error)) => notes.push(format!("{}: {error}", provider.name())),
                Err(_) => {
                    notes.push(format!("{}: request deadline reached", provider.name()));
                    break;
                }
            }
        }
        ChainResult {
            sources: Vec::new(),
            provider: None,
            notes,
        }
    }

    async fn enrich_sources(&self, sources: Vec<Source>, deadline: Instant) -> Vec<Source> {
        let inline_limit = self.inner.config.max_inline_sources.min(sources.len());
        let concurrency = self.inner.config.enrich_concurrency;
        let max_chars = self.inner.config.enrich_max_chars;
        let service = self.clone();
        let mut enriched: Vec<(usize, Source)> = stream::iter(sources.into_iter().enumerate())
            .map(move |(index, mut source)| {
                let service = service.clone();
                async move {
                    if index < inline_limit
                        && let Ok(url) = Url::parse(&source.url)
                        && let Ok((page, _, _)) = service.fetch_page(&url, deadline).await
                    {
                        source.content = Some(truncate_text(page.content, Some(max_chars)).0);
                    }
                    (index, source)
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;
        enriched.sort_by_key(|(index, _)| *index);
        enriched.into_iter().map(|(_, source)| source).collect()
    }

    async fn fetch_page(
        &self,
        url: &Url,
        deadline: Instant,
    ) -> Result<(FetchedPage, String, Option<String>)> {
        let mut specialist_failure = None;
        match timeout_at(
            deadline,
            crate::sources::github::fetch(
                &self.inner.client,
                url,
                self.inner.config.github_token.as_deref(),
                self.inner.config.github_max_comments,
            ),
        )
        .await
        {
            Ok(Ok(Some(page))) => {
                return Ok((
                    FetchedPage {
                        content: page.content,
                        title: None,
                        published_date: None,
                    },
                    page.source_type.to_string(),
                    None,
                ));
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => specialist_failure = Some(error.to_string()),
            Err(_) => {
                return Err(HybridSearchError::Timeout(
                    "GitHub content extraction".to_string(),
                ));
            }
        }

        let page = self.fetch_generic(url.as_str(), deadline).await?;
        Ok((page, "generic".to_string(), specialist_failure))
    }

    async fn fetch_generic(&self, url: &str, deadline: Instant) -> Result<FetchedPage> {
        let mut notes = Vec::new();
        for provider in &self.inner.source_providers {
            match timeout_at(deadline, provider.fetch(url)).await {
                Ok(Ok(page)) if !page.content.trim().is_empty() => return Ok(page),
                Ok(Ok(_)) => notes.push(format!("{} returned empty content", provider.name())),
                Ok(Err(error)) => notes.push(format!("{}: {error}", provider.name())),
                Err(_) => {
                    notes.push(format!("{}: request deadline reached", provider.name()));
                    break;
                }
            }
        }
        Err(HybridSearchError::Provider(format!(
            "all configured fetch providers failed: {}",
            notes.join("; ")
        )))
    }
}

fn normalize_domains(domains: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for domain in domains {
        let domain = domain.trim().to_ascii_lowercase();
        if !domain.is_empty() && !output.contains(&domain) {
            output.push(domain);
        }
    }
    output
}

fn truncate_text(content: String, limit: Option<usize>) -> (String, bool) {
    let Some(limit) = limit else {
        return (content, false);
    };
    if content.chars().count() <= limit {
        return (content, false);
    }
    (content.chars().take(limit).collect(), true)
}

fn apply_response_budget(
    answer: &mut Option<String>,
    sources: &mut Vec<Source>,
    budget: usize,
) -> bool {
    let mut truncated = false;
    while response_size(answer.as_deref(), sources) > budget {
        if let Some(source) = sources
            .iter_mut()
            .rev()
            .find(|source| source.content.is_some())
        {
            source.content = None;
            truncated = true;
        } else if sources.pop().is_some() {
            truncated = true;
        } else if let Some(content) = answer {
            *content = content.chars().take(budget).collect();
            truncated = true;
        } else {
            break;
        }
    }
    truncated
}

fn response_size(answer: Option<&str>, sources: &[Source]) -> usize {
    answer
        .map(|value| value.chars().count())
        .unwrap_or_default()
        + sources
            .iter()
            .map(|source| {
                serde_json::to_string(source)
                    .map(|value| value.chars().count())
                    .unwrap_or_default()
            })
            .sum::<usize>()
}

fn health<T>(
    provider: &str,
    result: std::result::Result<Result<T>, tokio::time::error::Elapsed>,
) -> ProviderHealth {
    match result {
        Ok(Ok(_)) => ProviderHealth {
            provider: provider.to_string(),
            reachable: true,
            detail: "reachable".to_string(),
        },
        Ok(Err(error)) => ProviderHealth {
            provider: provider.to_string(),
            reachable: false,
            detail: error.to_string(),
        },
        Err(_) => ProviderHealth {
            provider: provider.to_string(),
            reachable: false,
            detail: "probe timed out".to_string(),
        },
    }
}
