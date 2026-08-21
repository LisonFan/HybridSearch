use crate::model::{
    DoctorOutput, GetSourcesInput, GetSourcesOutput, HelpInput, HelpOutput, WebFetchInput,
    WebFetchOutput, WebMapInput, WebMapOutput, WebSearchInput, WebSearchOutput,
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
        name = "help",
        description = "Show the short HybridSearch capability index, or focused help for one tool or configuration topic."
    )]
    async fn help(&self, Parameters(input): Parameters<HelpInput>) -> Json<HelpOutput> {
        Json(crate::help::help(input.topic))
    }

    #[tool(
        name = "web_search",
        description = "Search the web with automatic routing or one selected provider. Use help(topic=web_search) for routing, filters, formats, and recovery details."
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
        description = "Read sources cached by web_search without searching again. Use help(topic=get_sources) for pagination and recovery details."
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
        description = "Fetch and extract one known URL. Use help(topic=web_fetch) for specialist APIs, fallback, and truncation details."
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
        description = "Discover URLs under a site with Tavily Map. Use help(topic=web_map) for requirements and limits."
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
        description = "Probe enabled providers and return redacted diagnostics. Use help(topic=doctor) before calling it."
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
                "Call help without a topic for the HybridSearch capability index, then request focused help for a tool when its detailed behavior matters."
            )
    }
}
