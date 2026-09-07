use crate::model::{HelpOutput, HelpTopic, HelpTopicSummary};

const TOPICS: &[(HelpTopic, &str)] = &[
    (
        HelpTopic::WebSearch,
        "Search the web and optionally select one provider.",
    ),
    (
        HelpTopic::GetSources,
        "Read and paginate sources cached by web_search.",
    ),
    (
        HelpTopic::WebFetch,
        "Extract one known URL, with specialist API support.",
    ),
    (
        HelpTopic::WebMap,
        "Discover URLs under a site with Tavily Map.",
    ),
    (
        HelpTopic::Doctor,
        "Inspect provider configuration and connectivity.",
    ),
    (
        HelpTopic::Configuration,
        "Review provider order and runtime settings.",
    ),
];

pub fn help(topic: Option<HelpTopic>) -> HelpOutput {
    match topic {
        None => HelpOutput {
            topic: "hybrid-search".to_string(),
            summary: "HybridSearch is an MCP server for provider-backed web search, cached source retrieval, structured page extraction, site mapping, and provider diagnostics. Select a topic for focused help.".to_string(),
            usage: Some("help({ topic?: web_search | get_sources | web_fetch | web_map | doctor | configuration })".to_string()),
            details: Vec::new(),
            topics: topic_summaries(),
        },
        Some(HelpTopic::WebSearch) => topic_help(
            "web_search",
            "Search the web through the configured provider route and cache the complete source set.",
            "web_search({ query, provider?, extra_sources?, response_format?, include_content?, recency_days?, include_domains?, exclude_domains? })",
            &[
                "query is required. response_format=concise omits inline page content; detailed includes it. response_format takes precedence over include_content.",
                "Without provider, ChatGPT2API runs first when configured and the HYBRID_SEARCH_SOURCE_PROVIDERS chain supplies supplemental or fallback sources.",
                "provider may be chatgpt2api, tavily, firecrawl, tinyfish, exa, or keenable. It selects exactly one configured provider and disables fallback.",
                "extra_sources controls supplemental count after a usable ChatGPT2API result; with a selected source provider, it is that provider's result limit.",
                "recency_days must be positive. include_domains and exclude_domains are enforced only by providers that support those filters; Firecrawl is skipped for filtered requests. Keenable supports recency and one include domain, but not exclude domains.",
                "Use search_provider, supplemental_provider, and fallback_used to identify the route actually used. answer is a synthesized response and should be verified against source URLs.",
                "The full source set is cached under session_id. When truncated is true, follow recovery_hint and use get_sources or web_fetch.",
            ],
        ),
        Some(HelpTopic::GetSources) => topic_help(
            "get_sources",
            "Read sources cached by an earlier web_search without issuing another search.",
            "get_sources({ session_id, offset?, limit? })",
            &[
                "session_id comes from web_search and remains valid while that session is present in the in-memory cache.",
                "Use offset and limit for pagination. Continue with next_offset until it is absent.",
                "When truncated is true, follow recovery_hint; use web_fetch on an individual URL for full page content.",
            ],
        ),
        Some(HelpTopic::WebFetch) => topic_help(
            "web_fetch",
            "Fetch and extract one known HTTP or HTTPS URL.",
            "web_fetch({ url, max_chars? })",
            &[
                "GitHub issues, pull requests, and releases, StackExchange questions, arXiv papers, and Wikipedia articles use specialist public APIs.",
                "Other URLs, or specialist extraction failures, use the configured generic fetch-provider chain.",
                "source_type identifies the extractor actually used. fallback_reason explains why specialist extraction fell back when applicable.",
                "max_chars limits returned content. When truncated is true, follow recovery_hint or request a larger limit if needed.",
            ],
        ),
        Some(HelpTopic::WebMap) => topic_help(
            "web_map",
            "Discover URLs under a site or URL prefix with Tavily Map.",
            "web_map({ url, max_results? })",
            &[
                "Tavily must be configured and enabled by HYBRID_SEARCH_SOURCE_PROVIDERS.",
                "max_results defaults to 20. web_map discovers URLs; use web_fetch to read one of them.",
            ],
        ),
        Some(HelpTopic::Doctor) => topic_help(
            "doctor",
            "Probe enabled providers and return redacted runtime diagnostics.",
            "doctor({})",
            &[
                "Provider probes may consume a small search request, so use doctor for configuration, authentication, rate-limit, network, timeout, or response problems rather than ordinary empty results.",
                "provider_order is the effective route; source_provider_order is the configured fallback chain.",
                "Each provider reports enabled, configured, reachable, redacted endpoints, credential presence, status, and detail.",
                "Common statuses include ok, not_configured, disabled, authentication, rate_limited, network, timeout, invalid_response, and provider_error.",
            ],
        ),
        Some(HelpTopic::Configuration) => topic_help(
            "configuration",
            "Configure at least one search provider, then adjust routing and runtime behavior with HYBRID_SEARCH_* variables.",
            "See README.md or README.zh-CN.md for the complete environment-variable reference.",
            &[
                "ChatGPT2API requires both CHATGPT2API_API_URL and CHATGPT2API_API_KEY. Tavily, Firecrawl, TinyFish, Exa, and Keenable each use their own API key and optional custom endpoint.",
                "HYBRID_SEARCH_SOURCE_PROVIDERS controls the ordered source chain. ChatGPT2API is configured separately and remains first in automatic routing.",
                "HYBRID_SEARCH_TIMEOUT_SECONDS is the shared deadline for one tool call. Response, cache, enrichment, and inline-content limits are configurable.",
                "HYBRID_SEARCH_LOG_PATH enables redacted JSONL diagnostic events. Search query text is not logged.",
            ],
        ),
    }
}

pub fn parse_topic(value: &str) -> Option<HelpTopic> {
    match value {
        "web_search" => Some(HelpTopic::WebSearch),
        "get_sources" => Some(HelpTopic::GetSources),
        "web_fetch" => Some(HelpTopic::WebFetch),
        "web_map" => Some(HelpTopic::WebMap),
        "doctor" => Some(HelpTopic::Doctor),
        "configuration" | "config" => Some(HelpTopic::Configuration),
        _ => None,
    }
}

pub fn render_cli(output: &HelpOutput) -> String {
    let mut rendered = format!("{}\n\n{}", output.topic, output.summary);
    if output.topic == "hybrid-search" {
        rendered.push_str(
            "\n\nUsage:\n  hybrid-search\n  hybrid-search help [topic]\n  hybrid-search --version",
        );
    } else if let Some(usage) = &output.usage {
        rendered.push_str("\n\nMCP usage:\n  ");
        rendered.push_str(usage);
    }
    if !output.topics.is_empty() {
        rendered.push_str("\n\nTopics:");
        for topic in &output.topics {
            rendered.push_str(&format!("\n  {:<14} {}", topic.name, topic.summary));
        }
        rendered.push_str("\n\nRun `hybrid-search help <topic>` for details. The default command starts the MCP server over stdio.");
    }
    if !output.details.is_empty() {
        rendered.push_str("\n\nDetails:");
        for detail in &output.details {
            rendered.push_str("\n  - ");
            rendered.push_str(detail);
        }
    }
    rendered
}

fn topic_help(topic: &str, summary: &str, usage: &str, details: &[&str]) -> HelpOutput {
    HelpOutput {
        topic: topic.to_string(),
        summary: summary.to_string(),
        usage: Some(usage.to_string()),
        details: details.iter().map(|detail| (*detail).to_string()).collect(),
        topics: Vec::new(),
    }
}

fn topic_summaries() -> Vec<HelpTopicSummary> {
    TOPICS
        .iter()
        .map(|(topic, summary)| HelpTopicSummary {
            name: topic.as_str().to_string(),
            summary: (*summary).to_string(),
        })
        .collect()
}
