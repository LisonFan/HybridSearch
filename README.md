# HybridSearch

[简体中文](README.zh-CN.md)

HybridSearch is a Rust MCP server for evidence-backed web search. It uses ChatGPT2API when configured, then walks an ordered source-provider chain:

```text
ChatGPT2API → Tavily → Firecrawl → TinyFish → Exa
```

The project does not require Grok or an OpenAI-compatible model gateway. Any configured search provider can start the server and serve `web_search`.

## Features

- Ordered provider fallback instead of parallel fan-out.
- Configurable source-provider order and per-request provider selection.
- Optional ChatGPT2API synthesized answers with cited sources.
- Tavily, Firecrawl, TinyFish, and Exa search and page extraction.
- Structured GitHub issue, pull request, and release extraction.
- Specialist StackExchange, arXiv, and Wikipedia extraction through their public APIs.
- Cached source sessions with paginated retrieval.
- Domain and recency filters where the upstream provider can enforce them.
- Shared request deadlines and configurable response limits.
- Actionable recovery hints when a response is trimmed.
- Categorized provider diagnostics and optional redacted JSONL logs.
- Native MCP stdio transport built with the official Rust SDK.

## MCP tools

| Tool | Purpose |
| --- | --- |
| `web_search` | Search, optionally select one provider, merge citations, extract page content, and cache sources. |
| `get_sources` | Read cached sources by `session_id` without running another search. |
| `web_fetch` | Read one URL; GitHub, StackExchange, arXiv, and Wikipedia URLs use specialist APIs. |
| `web_map` | Discover URLs with Tavily Map. |
| `doctor` | Probe configured providers and show redacted runtime diagnostics. |

`web_search` calls ChatGPT2API first when it is configured. Tavily, Firecrawl, TinyFish, and Exa form the default supplemental/fallback chain; the first provider with usable sources wins. Firecrawl is skipped when a request contains domain or recency filters because its search API cannot enforce those filters.

Pass `provider` to use exactly one provider without fallback or supplemental searches:

```json
{
  "query": "Rust 1.97 release notes",
  "provider": "exa",
  "response_format": "concise"
}
```

Accepted values are `chatgpt2api`, `tavily`, `firecrawl`, `tinyfish`, and `exa`. The selected provider must be configured and, for a source provider, enabled by `HYBRID_SEARCH_SOURCE_PROVIDERS`. When a source provider is selected, `extra_sources` controls its maximum result count and defaults to `HYBRID_SEARCH_FALLBACK_SOURCES`.

## Requirements

- Rust 1.97.1 when building from source.
- Node.js 26.7.0 or newer when installing from npm.
- At least one valid search configuration:
  - both `CHATGPT2API_API_URL` and `CHATGPT2API_API_KEY`, or
  - `TAVILY_API_KEY`, `FIRECRAWL_API_KEY`, `TINYFISH_API_KEY`, or `EXA_API_KEY`.

`GITHUB_TOKEN` improves GitHub API rate limits but does not count as a search provider.

## Installation

Install the native binary from npm:

```bash
npm install -g @dctwgroo/hybridsearch-mcp
hybrid-search --version
```

The npm package selects the matching Linux, macOS, or Windows x64/arm64 binary automatically. You can also download an archive from [GitHub Releases](https://github.com/LisonFan/HybridSearch/releases), or build it locally:

```bash
git clone https://github.com/LisonFan/HybridSearch.git
cd HybridSearch
cargo build --release --locked
```

The binary is written to `target/release/hybrid-search` (`hybrid-search.exe` on Windows).

## Configuration

### Search providers

| Variable | Default | Description |
| --- | --- | --- |
| `CHATGPT2API_API_URL` | — | URL of a deployed [chatgpt2api](https://github.com/basketikun/chatgpt2api) service. A root URL, `/v1`, or full `/v1/search` endpoint is accepted. |
| `CHATGPT2API_API_KEY` | — | Bearer key accepted by the chatgpt2api service. Both ChatGPT2API variables are required to enable it. |
| `TAVILY_API_KEY` | — | Tavily key. A comma-separated key list uses round-robin selection and key-scoped failover. |
| `TAVILY_API_URL` | `https://api.tavily.com` | Tavily API base URL. |
| `FIRECRAWL_API_KEY` | — | Firecrawl API key. |
| `FIRECRAWL_API_URL` | `https://api.firecrawl.dev/v1` | Firecrawl API base URL. |
| `TINYFISH_API_KEY` | — | TinyFish API key. |
| `TINYFISH_SEARCH_API_URL` | `https://api.search.tinyfish.ai` | TinyFish search endpoint. |
| `TINYFISH_FETCH_API_URL` | `https://api.fetch.tinyfish.ai` | TinyFish fetch endpoint. |
| `EXA_API_KEY` | — | Exa API key. |
| `EXA_API_URL` | `https://api.exa.ai` | Exa API base URL. |
| `GITHUB_TOKEN` | — | Optional GitHub token for higher API limits and private repositories. |

### Runtime behavior

| Variable | Default | Description |
| --- | --- | --- |
| `HYBRID_SEARCH_SOURCE_PROVIDERS` | `tavily,firecrawl,tinyfish,exa` | Comma-separated source-provider order. Reorder or omit providers; unknown names fail at startup. ChatGPT2API is controlled separately and remains first by default. |
| `HYBRID_SEARCH_TIMEOUT_SECONDS` | `300` | Total deadline shared by one tool call. |
| `HYBRID_SEARCH_EXTRA_SOURCES` | `3` | Supplemental source count after a usable ChatGPT2API result. |
| `HYBRID_SEARCH_FALLBACK_SOURCES` | `5` | Source count when ChatGPT2API is absent or unusable. |
| `HYBRID_SEARCH_CACHE_SIZE` | `256` | Maximum cached search sessions. |
| `HYBRID_SEARCH_FETCH_MAX_CHARS` | unlimited | Default `web_fetch` output limit. |
| `HYBRID_SEARCH_RESPONSE_MAX_CHARS` | `45000` | Approximate maximum search response size. |
| `HYBRID_SEARCH_ENRICH_CONCURRENCY` | `3` | Concurrent inline page extractions. |
| `HYBRID_SEARCH_ENRICH_MAX_CHARS` | `15000` | Maximum inline content per source. |
| `HYBRID_SEARCH_MAX_INLINE_SOURCES` | `5` | Maximum sources enriched inline. |
| `HYBRID_SEARCH_GITHUB_MAX_COMMENTS` | `30` | Maximum rendered GitHub comments. |
| `HYBRID_SEARCH_SOURCE_MAX_ANSWERS` | `5` | Maximum rendered StackExchange answers; accepted answers are shown first. |
| `HYBRID_SEARCH_LOG_PATH` | disabled | Append redacted diagnostic events as JSON Lines to this file. Search query text is not logged. |

`web_fetch` uses the GitHub REST API, Stack Exchange API v2.3, arXiv Export API, and MediaWiki Action API directly for matching URLs. These public specialist APIs do not require Tavily, Firecrawl, TinyFish, or Exa credentials. When a specialist cannot extract the page, HybridSearch records the reason and falls back to the configured generic fetch chain.

When response limits remove inline content or trailing sources, `web_search`, `get_sources`, and `web_fetch` return a `recovery_hint`. Cached search results remain complete: use `get_sources(session_id)` for more source records and `web_fetch(url)` for full page content.

`doctor` lists every supported provider with its enabled/configured state, redacted endpoint, credential presence, reachability, and a categorized status such as `authentication`, `rate_limited`, `network`, or `timeout`. Its live probes may consume a small provider request. Diagnostic logs recursively mask API-key, token, authorization, password, and cookie fields.

## MCP client setup

Codex configuration:

```toml
[mcp_servers.hybrid-search]
command = "/absolute/path/to/hybrid-search"

[mcp_servers.hybrid-search.env]
TAVILY_API_KEY = "tvly-..."
HYBRID_SEARCH_TIMEOUT_SECONDS = "300"
```

When installed globally from npm, use `command = "hybrid-search"` instead of an absolute path.

JSON-based clients:

```json
{
  "mcpServers": {
    "hybrid-search": {
      "command": "/absolute/path/to/hybrid-search",
      "args": [],
      "env": {
        "TAVILY_API_KEY": "tvly-..."
      }
    }
  }
}
```

For ChatGPT2API:

```json
{
  "CHATGPT2API_API_URL": "http://127.0.0.1:8000",
  "CHATGPT2API_API_KEY": "your-auth-key"
}
```

## Development

```bash
cargo fmt --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

No test cases are maintained in this project. GitHub Actions performs formatting, compilation, linting, and release builds.

## Acknowledgements

HybridSearch is inspired by and contains adapted MIT-licensed provider and source-extraction work from [Episkey-G/GrokSearch-rs](https://github.com/Episkey-G/GrokSearch-rs). Thank you to its contributors for the strong foundation.

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the retained license notice.

## License

[MIT](LICENSE)
