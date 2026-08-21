# HybridSearch MCP

Install the native HybridSearch MCP server with npm:

```bash
npm install -g @dctwgroo/hybridsearch-mcp
hybrid-search --version
hybrid-search help
```

The package installs the native Rust binary for the current Linux, macOS, or Windows architecture. HybridSearch supports ordered provider fallback, per-request provider selection, recovery hints, provider diagnostics, and optional redacted JSONL logs. See the [HybridSearch repository](https://github.com/LisonFan/HybridSearch) for configuration and MCP client examples.

Node.js 26.7.0 or newer is required.

Run `hybrid-search help <topic>` for focused help such as `web_search`, `web_fetch`, or `doctor`.

## 中文

使用 npm 安装 HybridSearch MCP 原生程序：

```bash
npm install -g @dctwgroo/hybridsearch-mcp
hybrid-search --version
hybrid-search help
```

安装时会根据当前 Linux、macOS 或 Windows 架构选择对应的 Rust 原生二进制。HybridSearch 支持有序降级、单次请求指定供应商、截断恢复提示、provider 诊断和可选脱敏 JSONL 日志。配置项和 MCP 客户端示例请查看 [HybridSearch 项目](https://github.com/LisonFan/HybridSearch)。

需要 Node.js 26.7.0 或更高版本。

使用 `hybrid-search help <主题>` 按需查看 `web_search`、`web_fetch` 或 `doctor` 等详细帮助。
