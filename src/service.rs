use crate::cache::SourceCache;
use crate::config::Config;
use crate::error::{HybridSearchError, Result};
use crate::logging::DiagnosticLogger;
use crate::model::{
    DoctorOutput, FetchedPage, GetSourcesOutput, ProviderHealth, ResponseFormat, SearchFilters,
    SearchProvider, Source, WebFetchOutput, WebMapOutput, WebSearchInput, WebSearchOutput,
    merge_sources,
};
use crate::providers::{
    Chatgpt2apiProvider, ExaProvider, FirecrawlProvider, KeenableProvider, SharedSourceProvider,
    TavilyProvider, TinyfishProvider, build_http_client,
};
use futures::{StreamExt, stream};
use serde_json::json;
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
    logger: DiagnosticLogger,
}

struct ChainResult {
    sources: Vec<Source>,
    provider: Option<String>,
    notes: Vec<String>,
}

struct ProbeResult {
    provider: String,
    reachable: bool,
    status: String,
    detail: String,
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
        for provider in &config.source_providers {
            match provider.as_str() {
                "tavily" => {
                    let Some(key) = &config.tavily_api_key else {
                        continue;
                    };
                    source_providers.push(Arc::new(TavilyProvider::new(
                        client.clone(),
                        config.tavily_api_url.clone(),
                        key.clone(),
                    )));
                }
                "firecrawl" => {
                    let Some(key) = &config.firecrawl_api_key else {
                        continue;
                    };
                    source_providers.push(Arc::new(FirecrawlProvider::new(
                        client.clone(),
                        config.firecrawl_api_url.clone(),
                        key.clone(),
                    )));
                }
                "tinyfish" => {
                    let Some(key) = &config.tinyfish_api_key else {
                        continue;
                    };
                    source_providers.push(Arc::new(TinyfishProvider::new(
                        client.clone(),
                        config.tinyfish_search_api_url.clone(),
                        config.tinyfish_fetch_api_url.clone(),
                        key.clone(),
                    )));
                }
                "exa" => {
                    let Some(key) = &config.exa_api_key else {
                        continue;
                    };
                    source_providers.push(Arc::new(ExaProvider::new(
                        client.clone(),
                        config.exa_api_url.clone(),
                        key.clone(),
                    )));
                }
                "keenable" => {
                    let Some(key) = &config.keenable_api_key else {
                        continue;
                    };
                    source_providers.push(Arc::new(KeenableProvider::new(
                        client.clone(),
                        config.keenable_api_url.clone(),
                        key.clone(),
                    )));
                }
                _ => {}
            }
        }

        let logger = DiagnosticLogger::new(config.debug_log_path.clone());
        logger.event(
            "service.started",
            json!({
                "provider_order": config.effective_provider_order(),
                "source_provider_order": config.source_providers,
                "configured_providers": config.configured_providers(),
                "timeout_seconds": config.timeout.as_secs(),
            }),
        );

        Ok(Self {
            inner: Arc::new(ServiceInner {
                cache: Mutex::new(SourceCache::new(config.cache_size)),
                config,
                client,
                chatgpt2api,
                source_providers,
                logger,
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
        let requested_provider = input.provider;
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
        self.inner.logger.event(
            "web_search.started",
            json!({
                "query_length": query.chars().count(),
                "provider": requested_provider.map(SearchProvider::as_str),
                "response_format": input.response_format,
                "recency_days": filters.recency_days,
                "include_domains_count": filters.include_domains.len(),
                "exclude_domains_count": filters.exclude_domains.len(),
            }),
        );
        let mut notes = Vec::new();
        let mut answer = None;
        let mut primary_sources = Vec::new();
        let mut primary_usable = false;
        let requested_extra = input
            .extra_sources
            .unwrap_or(self.inner.config.default_extra_sources);
        let (sources, search_provider, supplemental_provider, fallback_used) =
            if let Some(selected) = requested_provider {
                match selected {
                    SearchProvider::Chatgpt2api => {
                        let provider = self.inner.chatgpt2api.as_ref().ok_or_else(|| {
                            HybridSearchError::MissingConfig(
                                "CHATGPT2API_API_URL + CHATGPT2API_API_KEY".to_string(),
                            )
                        })?;
                        let result =
                            match timeout_at(deadline, provider.search(query, &filters)).await {
                                Ok(Ok(result)) if !result.sources.is_empty() => result,
                                Ok(Ok(_)) => {
                                    return Err(HybridSearchError::Provider(
                                        "chatgpt2api returned no sources".to_string(),
                                    ));
                                }
                                Ok(Err(error)) => {
                                    self.log_provider_failure("chatgpt2api", &error);
                                    return Err(error);
                                }
                                Err(_) => {
                                    return Err(HybridSearchError::Timeout(
                                        "ChatGPT2API search request".to_string(),
                                    ));
                                }
                            };
                        answer = result.answer;
                        (
                            merge_sources(Vec::new(), result.sources),
                            selected.as_str().to_string(),
                            None,
                            false,
                        )
                    }
                    _ => {
                        let name = selected.as_str();
                        if !self.inner.config.source_provider_enabled(name) {
                            return Err(HybridSearchError::InvalidParams(format!(
                                "provider '{name}' is disabled by HYBRID_SEARCH_SOURCE_PROVIDERS"
                            )));
                        }
                        let provider = self
                            .inner
                            .source_providers
                            .iter()
                            .find(|provider| provider.name() == name)
                            .ok_or_else(|| {
                                HybridSearchError::MissingConfig(
                                    provider_key_name(name).to_string(),
                                )
                            })?;
                        if !filters.is_empty() && !provider.supports_filters() {
                            return Err(HybridSearchError::InvalidParams(format!(
                                "provider '{name}' cannot enforce domain or recency filters"
                            )));
                        }
                        let max_results = input
                            .extra_sources
                            .unwrap_or(self.inner.config.fallback_sources);
                        let mut selected_sources = match timeout_at(
                            deadline,
                            provider.search(query, max_results, &filters),
                        )
                        .await
                        {
                            Ok(Ok(sources)) => merge_sources(Vec::new(), sources),
                            Ok(Err(error)) => {
                                self.log_provider_failure(name, &error);
                                return Err(error);
                            }
                            Err(_) => {
                                return Err(HybridSearchError::Timeout(format!(
                                    "{name} search request"
                                )));
                            }
                        };
                        selected_sources.truncate(max_results);
                        if selected_sources.is_empty() {
                            return Err(HybridSearchError::Provider(format!(
                                "{name} returned no sources"
                            )));
                        }
                        (selected_sources, name.to_string(), None, false)
                    }
                }
            } else {
                if let Some(provider) = &self.inner.chatgpt2api {
                    match timeout_at(deadline, provider.search(query, &filters)).await {
                        Ok(Ok(result)) if !result.sources.is_empty() => {
                            answer = result.answer;
                            primary_sources = merge_sources(Vec::new(), result.sources);
                            primary_usable = true;
                        }
                        Ok(Ok(_)) => notes.push("chatgpt2api returned no sources".to_string()),
                        Ok(Err(error)) => {
                            self.log_provider_failure("chatgpt2api", &error);
                            notes.push(format!("chatgpt2api: {error}"));
                        }
                        Err(_) => notes.push("chatgpt2api: request deadline reached".to_string()),
                    }
                }

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
                    let detail = if notes.is_empty() {
                        "no usable sources".to_string()
                    } else {
                        self.inner.config.redact_text(&notes.join("; "))
                    };
                    return Err(HybridSearchError::Provider(format!(
                        "all configured search providers failed: {detail}"
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
                (
                    merge_sources(primary_sources, chain.sources),
                    search_provider,
                    supplemental_provider,
                    fallback_used,
                )
            };
        let mut sources = sources;
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
            answer.as_deref(),
            &mut response_sources,
            self.inner.config.response_max_chars,
            &session_id,
            0,
        );
        let recovery_hint = truncated.then(|| {
            format!(
                "The response was trimmed to fit the configured budget. Use get_sources(session_id=\"{session_id}\") for the cached source list or web_fetch(url) for full page content."
            )
        });
        self.inner.logger.event(
            "web_search.completed",
            json!({
                "provider": search_provider,
                "supplemental_provider": supplemental_provider,
                "sources_count": sources_count,
                "fallback_used": fallback_used,
                "truncated": truncated,
            }),
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
            recovery_hint,
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
        let truncated = apply_response_budget(
            None,
            &mut sources,
            self.inner.config.response_max_chars,
            session_id,
            start,
        );
        let next_offset = (start + sources.len() < total_sources).then_some(start + sources.len());
        let recovery_hint = truncated.then(|| match next_offset {
            Some(next_offset) => format!(
                "This page was trimmed to fit the configured budget. Use web_fetch(url) for full page content and continue with get_sources(session_id=\"{session_id}\", offset={next_offset})."
            ),
            None => {
                "This page was trimmed to fit the configured budget. Use web_fetch(url) for full page content."
                    .to_string()
            }
        });
        Ok(GetSourcesOutput {
            session_id: session_id.to_string(),
            sources_count: sources.len(),
            sources,
            total_sources,
            offset: start,
            next_offset,
            truncated,
            recovery_hint,
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
        let recovery_hint = truncated.then(|| {
            "Call web_fetch again with a larger max_chars value, or omit max_chars when no HYBRID_SEARCH_FETCH_MAX_CHARS default is configured."
                .to_string()
        });
        Ok(WebFetchOutput {
            url: parsed.to_string(),
            content,
            title: page.title,
            published_date: page.published_date,
            original_length,
            truncated,
            source_type,
            fallback_reason,
            recovery_hint,
        })
    }

    pub async fn web_map(&self, url: &str, max_results: usize) -> Result<WebMapOutput> {
        if !self.inner.config.source_provider_enabled("tavily") {
            return Err(HybridSearchError::InvalidParams(
                "Tavily is disabled by HYBRID_SEARCH_SOURCE_PROVIDERS".to_string(),
            ));
        }
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
                probe_result("chatgpt2api", result)
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
                probe_result(name, result)
            }));
        }
        let mut results = Vec::new();
        for probe in probes {
            match probe.await {
                Ok(result) => results.push(result),
                Err(error) => results.push(ProbeResult {
                    provider: "unknown".to_string(),
                    reachable: false,
                    status: "internal_error".to_string(),
                    detail: error.to_string(),
                }),
            }
        }

        let mut providers = Vec::new();
        for name in [
            "chatgpt2api",
            "tavily",
            "firecrawl",
            "tinyfish",
            "exa",
            "keenable",
        ] {
            let configured = self.inner.config.provider_configured(name);
            let enabled = if name == "chatgpt2api" {
                configured
            } else if self.inner.config.source_providers_explicit {
                self.inner.config.source_provider_enabled(name)
            } else {
                configured
            };
            let probe = results.iter().find(|result| result.provider == name);
            let (reachable, status, detail) = if !configured {
                (
                    false,
                    "not_configured".to_string(),
                    format!("{} is not configured", provider_key_name(name)),
                )
            } else if !enabled {
                (
                    false,
                    "disabled".to_string(),
                    "disabled by HYBRID_SEARCH_SOURCE_PROVIDERS".to_string(),
                )
            } else if let Some(probe) = probe {
                (
                    probe.reachable,
                    probe.status.clone(),
                    self.inner.config.redact_text(&probe.detail),
                )
            } else {
                (
                    false,
                    "not_probed".to_string(),
                    "provider was not probed".to_string(),
                )
            };
            providers.push(ProviderHealth {
                provider: name.to_string(),
                enabled,
                configured,
                endpoints: provider_endpoints(&self.inner.config, name),
                credential: if configured { "set" } else { "unset" }.to_string(),
                reachable,
                status,
                detail,
            });
        }

        let degraded = providers
            .iter()
            .any(|provider| provider.enabled && (!provider.configured || !provider.reachable));
        let output = DoctorOutput {
            status: if degraded { "degraded" } else { "ok" }.to_string(),
            configuration_source: "environment + built-in defaults".to_string(),
            provider_order: self
                .inner
                .config
                .effective_provider_order()
                .into_iter()
                .map(str::to_string)
                .collect(),
            source_provider_order: self.inner.config.source_providers.clone(),
            providers,
            github_token_configured: self.inner.config.github_token.is_some(),
            timeout_seconds: self.inner.config.timeout.as_secs(),
            cache_size: self.inner.config.cache_size,
            response_max_chars: self.inner.config.response_max_chars,
            debug_log_path: self
                .inner
                .logger
                .path()
                .map(|path| path.display().to_string()),
        };
        self.inner.logger.event("doctor.completed", json!(&output));
        output
    }

    fn log_provider_failure(&self, provider: &str, error: &HybridSearchError) {
        let status = error_status(error);
        self.inner.logger.event(
            "provider.failed",
            json!({
                "provider": provider,
                "status": status,
                "detail": format!("{provider} request failed: {status}"),
            }),
        );
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
                        self.inner.logger.event(
                            "provider.search_completed",
                            json!({
                                "provider": provider.name(),
                                "sources_count": sources.len(),
                            }),
                        );
                        return ChainResult {
                            sources,
                            provider: Some(provider.name().to_string()),
                            notes,
                        };
                    }
                    self.inner.logger.event(
                        "provider.search_completed",
                        json!({
                            "provider": provider.name(),
                            "sources_count": 0,
                        }),
                    );
                    notes.push(format!("{} returned no sources", provider.name()));
                }
                Ok(Err(error)) => {
                    self.log_provider_failure(provider.name(), &error);
                    notes.push(format!("{}: {error}", provider.name()));
                }
                Err(_) => {
                    self.inner.logger.event(
                        "provider.failed",
                        json!({
                            "provider": provider.name(),
                            "status": "timeout",
                            "detail": "request deadline reached",
                        }),
                    );
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
            crate::sources::fetch(
                &self.inner.client,
                url,
                self.inner.config.github_token.as_deref(),
                self.inner.config.github_max_comments,
                self.inner.config.source_max_answers,
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
                    "specialist content extraction".to_string(),
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
                Ok(Ok(page)) if !page.content.trim().is_empty() => {
                    self.inner.logger.event(
                        "provider.fetch_completed",
                        json!({
                            "provider": provider.name(),
                            "content_length": page.content.chars().count(),
                        }),
                    );
                    return Ok(page);
                }
                Ok(Ok(_)) => notes.push(format!("{} returned empty content", provider.name())),
                Ok(Err(error)) => {
                    self.log_provider_failure(provider.name(), &error);
                    notes.push(format!("{}: {error}", provider.name()));
                }
                Err(_) => {
                    self.inner.logger.event(
                        "provider.failed",
                        json!({
                            "provider": provider.name(),
                            "status": "timeout",
                            "detail": "request deadline reached",
                        }),
                    );
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
    answer: Option<&str>,
    sources: &mut Vec<Source>,
    budget: usize,
    session_id: &str,
    source_offset: usize,
) -> bool {
    let answer_chars = answer
        .map(str::chars)
        .map(Iterator::count)
        .unwrap_or_default();
    let mut total = answer_chars + sources.iter().map(source_weight).sum::<usize>();
    if total <= budget {
        return false;
    }

    for index in (0..sources.len()).rev() {
        if total <= budget {
            break;
        }
        let content_length = sources[index]
            .content
            .as_deref()
            .map(str::chars)
            .map(Iterator::count)
            .unwrap_or_default();
        if content_length == 0 {
            continue;
        }
        let url = sources[index].url.clone();
        let source_index = source_offset + index;
        let note = |message: &str| {
            format!(
                "_[{message}: response budget reached — full text via web_fetch(\"{url}\") or get_sources(session_id=\"{session_id}\", offset={source_index}, limit=1)]_"
            )
        };
        let omitted = note("inline content omitted");
        let omitted_length = omitted.chars().count();
        if content_length <= omitted_length {
            continue;
        }
        let overshoot = total - budget;
        let truncated = note("truncated");
        let note_length = truncated.chars().count() + 2;
        if content_length > overshoot + note_length {
            let keep = content_length - overshoot - note_length;
            let prefix: String = sources[index]
                .content
                .as_deref()
                .unwrap_or_default()
                .chars()
                .take(keep)
                .collect();
            sources[index].content = Some(format!("{prefix}\n\n{truncated}"));
            total -= overshoot;
        } else {
            sources[index].content = Some(omitted);
            total = total - content_length + omitted_length;
        }
    }

    while total > budget && sources.len() > 1 {
        if let Some(source) = sources.pop() {
            total = total.saturating_sub(source_weight(&source));
        }
    }

    true
}

fn source_weight(source: &Source) -> usize {
    serde_json::to_string(source)
        .map(|value| value.chars().count())
        .unwrap_or_default()
}

fn probe_result<T>(
    provider: &str,
    result: std::result::Result<Result<T>, tokio::time::error::Elapsed>,
) -> ProbeResult {
    match result {
        Ok(Ok(_)) => ProbeResult {
            provider: provider.to_string(),
            reachable: true,
            status: "ok".to_string(),
            detail: "reachable".to_string(),
        },
        Ok(Err(error)) => ProbeResult {
            provider: provider.to_string(),
            reachable: false,
            status: error_status(&error).to_string(),
            detail: error.to_string(),
        },
        Err(_) => ProbeResult {
            provider: provider.to_string(),
            reachable: false,
            status: "timeout".to_string(),
            detail: "probe timed out".to_string(),
        },
    }
}

fn error_status(error: &HybridSearchError) -> &'static str {
    match error {
        HybridSearchError::MissingConfig(_) => "not_configured",
        HybridSearchError::InvalidParams(_) => "invalid_request",
        HybridSearchError::Parse(_) => "invalid_response",
        HybridSearchError::Timeout(_) => "timeout",
        HybridSearchError::NotFound(_) => "not_found",
        HybridSearchError::Provider(detail) => {
            let detail = detail.to_ascii_lowercase();
            if detail.contains("http 401") || detail.contains("http 403") {
                "authentication"
            } else if detail.contains("http 429")
                || detail.contains("http 432")
                || detail.contains("http 433")
            {
                "rate_limited"
            } else if detail.contains("request failed")
                || detail.contains("connect")
                || detail.contains("dns")
            {
                "network"
            } else {
                "provider_error"
            }
        }
    }
}

fn provider_key_name(provider: &str) -> &'static str {
    match provider {
        "chatgpt2api" => "CHATGPT2API_API_URL + CHATGPT2API_API_KEY",
        "tavily" => "TAVILY_API_KEY",
        "firecrawl" => "FIRECRAWL_API_KEY",
        "tinyfish" => "TINYFISH_API_KEY",
        "exa" => "EXA_API_KEY",
        "keenable" => "KEENABLE_API_KEY",
        _ => "provider credentials",
    }
}

fn provider_endpoints(config: &Config, provider: &str) -> Vec<String> {
    let endpoints: Vec<&str> = match provider {
        "chatgpt2api" => config
            .chatgpt2api_api_url
            .iter()
            .map(String::as_str)
            .collect(),
        "tavily" => vec![&config.tavily_api_url],
        "firecrawl" => vec![&config.firecrawl_api_url],
        "tinyfish" => vec![
            &config.tinyfish_search_api_url,
            &config.tinyfish_fetch_api_url,
        ],
        "exa" => vec![&config.exa_api_url],
        "keenable" => vec![&config.keenable_api_url],
        _ => Vec::new(),
    };
    endpoints
        .into_iter()
        .map(|endpoint| redact_endpoint(config, endpoint))
        .collect()
}

fn redact_endpoint(config: &Config, endpoint: &str) -> String {
    let Ok(mut url) = Url::parse(endpoint) else {
        return config.redact_text(endpoint);
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    config.redact_text(url.as_str())
}
