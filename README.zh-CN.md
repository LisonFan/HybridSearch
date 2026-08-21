# HybridSearch

[English](README.md)

HybridSearch 是一个使用 Rust 开发、强调可靠来源的网页搜索 MCP 服务。配置 ChatGPT2API 后优先调用它，随后按固定顺序使用搜索来源：

```text
ChatGPT2API → Tavily → Firecrawl → TinyFish → Exa
```

项目不依赖 Grok，也不要求 OpenAI Compatible 模型网关。只要配置任意一个有效搜索来源，就可以启动服务并使用 `web_search`。

## 功能

- 按固定顺序降级，不并行请求全部来源。
- 可选的 ChatGPT2API 综合回答及引用来源。
- 支持 Tavily、Firecrawl、TinyFish、Exa 搜索和网页提取。
- 对 GitHub issue、PR 和 release 进行结构化解析。
- 按搜索会话缓存来源，并支持分页读取。
- 在上游支持时执行域名和时间范围过滤。
- 所有 provider 共享单次调用总超时，并支持响应大小限制。
- 使用官方 Rust SDK 实现原生 MCP stdio 传输。

## MCP 工具

| 工具 | 用途 |
| --- | --- |
| `web_search` | 搜索、合并引用、按需提取正文并缓存来源。 |
| `get_sources` | 通过 `session_id` 读取缓存来源，不重新搜索。 |
| `web_fetch` | 读取指定 URL；GitHub URL 使用结构化 REST 解析。 |
| `web_map` | 使用 Tavily Map 发现站点 URL。 |
| `doctor` | 探测已配置 provider，并返回脱敏后的运行诊断。 |

配置 ChatGPT2API 时，`web_search` 会先调用它。Tavily、Firecrawl、TinyFish、Exa 构成补充和降级链，首个返回有效来源的 provider 即停止。请求包含域名或时间过滤条件时会跳过 Firecrawl，因为它的搜索接口无法严格执行这些过滤条件。

## 环境要求

- 从源码构建需要 Rust 1.97.1。
- 通过 npm 安装需要 Node.js 26.7.0 或更高版本。
- 至少配置一个有效搜索来源：
  - 同时配置 `CHATGPT2API_API_URL` 和 `CHATGPT2API_API_KEY`；或者
  - 配置 `TAVILY_API_KEY`、`FIRECRAWL_API_KEY`、`TINYFISH_API_KEY`、`EXA_API_KEY` 中任意一个。

`GITHUB_TOKEN` 仅用于提高 GitHub API 限额或访问私有仓库，不单独算作搜索来源。

## 安装

可以通过 npm 安装原生二进制：

```bash
npm install -g hybridsearch-mcp
hybrid-search --version
```

npm 包会自动选择与当前 Linux、macOS 或 Windows x64/arm64 环境匹配的二进制。也可以从 [GitHub Releases](https://github.com/LisonFan/HybridSearch/releases) 下载压缩包，或者从源码构建：

```bash
git clone https://github.com/LisonFan/HybridSearch.git
cd HybridSearch
cargo build --release --locked
```

构建结果位于 `target/release/hybrid-search`，Windows 下为 `hybrid-search.exe`。

## 配置

### 搜索来源

| 环境变量 | 默认值 | 说明 |
| --- | --- | --- |
| `CHATGPT2API_API_URL` | — | 已部署的 [chatgpt2api](https://github.com/basketikun/chatgpt2api) 服务地址。支持根地址、`/v1` 或完整 `/v1/search` 地址。 |
| `CHATGPT2API_API_KEY` | — | chatgpt2api 服务接受的 Bearer Key。必须同时配置两个 ChatGPT2API 变量才能启用。 |
| `TAVILY_API_KEY` | — | Tavily Key。支持用逗号分隔多个 Key，轮询使用，并在 Key 失效或限流时切换。 |
| `TAVILY_API_URL` | `https://api.tavily.com` | Tavily API 根地址。 |
| `FIRECRAWL_API_KEY` | — | Firecrawl API Key。 |
| `FIRECRAWL_API_URL` | `https://api.firecrawl.dev/v1` | Firecrawl API 根地址。 |
| `TINYFISH_API_KEY` | — | TinyFish API Key。 |
| `TINYFISH_SEARCH_API_URL` | `https://api.search.tinyfish.ai` | TinyFish 搜索接口。 |
| `TINYFISH_FETCH_API_URL` | `https://api.fetch.tinyfish.ai` | TinyFish 抓取接口。 |
| `EXA_API_KEY` | — | Exa API Key。 |
| `EXA_API_URL` | `https://api.exa.ai` | Exa API 根地址。 |
| `GITHUB_TOKEN` | — | 可选 GitHub Token，用于提高 API 限额和访问私有仓库。 |

### 运行参数

| 环境变量 | 默认值 | 说明 |
| --- | --- | --- |
| `HYBRID_SEARCH_TIMEOUT_SECONDS` | `300` | 单次工具调用共享的总超时。 |
| `HYBRID_SEARCH_EXTRA_SOURCES` | `3` | ChatGPT2API 返回有效结果后的补充来源数。 |
| `HYBRID_SEARCH_FALLBACK_SOURCES` | `5` | ChatGPT2API 未配置或结果不可用时的来源数。 |
| `HYBRID_SEARCH_CACHE_SIZE` | `256` | 最大缓存搜索会话数。 |
| `HYBRID_SEARCH_FETCH_MAX_CHARS` | 不限制 | `web_fetch` 默认输出长度限制。 |
| `HYBRID_SEARCH_RESPONSE_MAX_CHARS` | `45000` | 搜索响应近似最大长度。 |
| `HYBRID_SEARCH_ENRICH_CONCURRENCY` | `3` | 来源正文并发提取数。 |
| `HYBRID_SEARCH_ENRICH_MAX_CHARS` | `15000` | 每个来源内联正文最大长度。 |
| `HYBRID_SEARCH_MAX_INLINE_SOURCES` | `5` | 最大内联正文来源数。 |
| `HYBRID_SEARCH_GITHUB_MAX_COMMENTS` | `30` | GitHub 评论最大渲染数量。 |

## MCP 客户端配置

Codex 配置：

```toml
[mcp_servers.hybrid-search]
command = "/absolute/path/to/hybrid-search"

[mcp_servers.hybrid-search.env]
TAVILY_API_KEY = "tvly-..."
HYBRID_SEARCH_TIMEOUT_SECONDS = "300"
```

通过 npm 全局安装后，可以使用 `command = "hybrid-search"`，不需要填写绝对路径。

使用 JSON 配置的客户端：

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

ChatGPT2API 配置：

```json
{
  "CHATGPT2API_API_URL": "http://127.0.0.1:8000",
  "CHATGPT2API_API_KEY": "your-auth-key"
}
```

## 开发检查

```bash
cargo fmt --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

本项目不维护 test case。GitHub Actions 负责格式检查、编译、Clippy 和发布构建。

## 致谢

HybridSearch 参考并改编了 [Episkey-G/GrokSearch-rs](https://github.com/Episkey-G/GrokSearch-rs) 中采用 MIT 许可证的 provider 和来源提取实现。感谢 GrokSearch-rs 贡献者提供的优秀基础。

保留的许可证声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

## 许可证

[MIT](LICENSE)
