use crate::model::{
    DoctorOutput, GetSourcesInput, GetSourcesOutput, WebFetchInput, WebFetchOutput, WebMapInput,
    WebMapOutput, WebSearchInput, WebSearchOutput,
};
use crate::service::SearchService;
use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

#[derive(Clone)]
pub struct HybridSearchServer {
    service: SearchService,
    tool_router: ToolRouter<Self>,
}

impl HybridSearchServer {
    pub fn new(service: SearchService) -> Self {
        Self {
            service,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl HybridSearchServer {
    #[tool(
        name = "web_search",
        description = "Search the web with the configured provider chain. ChatGPT2API runs first when configured, followed by Tavily, Firecrawl, TinyFish, and Exa. Sources are cached under the returned session_id. Use response_format=concise for metadata only or detailed for inline page content."
    )]
    async fn web_search(
        &self,
        Parameters(input): Parameters<WebSearchInput>,
    ) -> std::result::Result<Json<WebSearchOutput>, String> {
        self.service
            .web_search(input)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_sources",
        description = "Read cached sources from an earlier web_search by session_id without issuing a new search. Supports offset and limit pagination."
    )]
    async fn get_sources(
        &self,
        Parameters(input): Parameters<GetSourcesInput>,
    ) -> std::result::Result<Json<GetSourcesOutput>, String> {
        self.service
            .get_sources(
                input.session_id.trim(),
                input.offset.unwrap_or_default(),
                input.limit,
            )
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "web_fetch",
        description = "Fetch and extract one known URL. GitHub issues, pull requests, and releases, StackExchange questions, arXiv papers, and Wikipedia articles use specialist APIs; other pages use Tavily, Firecrawl, TinyFish, then Exa."
    )]
    async fn web_fetch(
        &self,
        Parameters(input): Parameters<WebFetchInput>,
    ) -> std::result::Result<Json<WebFetchOutput>, String> {
        self.service
            .web_fetch(input.url.trim(), input.max_chars)
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "web_map",
        description = "Discover URLs under a site with Tavily Map. Requires TAVILY_API_KEY."
    )]
    async fn web_map(
        &self,
        Parameters(input): Parameters<WebMapInput>,
    ) -> std::result::Result<Json<WebMapOutput>, String> {
        self.service
            .web_map(input.url.trim(), input.max_results.unwrap_or(20))
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "doctor",
        description = "Probe every configured search provider and return the effective provider order plus redacted runtime diagnostics. Provider probes may consume a small search request."
    )]
    async fn doctor(&self) -> Json<DoctorOutput> {
        Json(self.service.doctor().await)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for HybridSearchServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("hybrid-search", env!("CARGO_PKG_VERSION"))
                    .with_title("HybridSearch")
                    .with_description(env!("CARGO_PKG_DESCRIPTION"))
                    .with_website_url(env!("CARGO_PKG_HOMEPAGE")),
            )
            .with_instructions(
                "Use web_search for discovery, web_fetch for a known URL, get_sources for cached results, web_map for Tavily URL discovery, and doctor for diagnostics."
            )
    }
}
