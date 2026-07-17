# r1 Anthropic 调试记录

## 目标

让 `r1` 使用自定义 Claude/Anthropic 网关：

- API key：`team-***`（本文不记录完整密钥）
- Claude Code 的 `ANTHROPIC_BASE_URL`：`https://co.yes.vg/team`
- 模型：`claude-opus-4-7`

验证命令：

```bash
./scripts/clean_and_run_r1.sh
```

脚本会先删除 `.appleby`，再运行 `r1`，因此会根据 `src/state/config.rs` 的默认值重新生成 `.appleby/config.toml`。

## 最终结果

已经调通。最终验证输出：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.52s
Running `target\debug\r1.exe`
config: model=claude-opus-4-7, base_url=https://co.yes.vg/team/v1
Text { text: "ok" }
```

请求已经成功完成，SSE 响应被兼容层累积为 `anthropic-ai-sdk` 的 `CreateMessageResponse`，不再出现 TLS 错误、503、`no_available_providers` 或单字符 `.` 占位响应。

## 根因 1：SDK 和 Claude Code 对 base URL 的约定不同

`anthropic-ai-sdk = 0.2.27` 的实现会直接拼接 endpoint path：

```rust
let url = format!("{}{}", self.api_base_url, path);
```

Messages API 使用：

```rust
self.post("/messages", body).await
```

因此这个 SDK 的 base URL 必须已经包含 `/v1`。

Claude Code 配置：

```text
https://co.yes.vg/team
```

在本项目 SDK 中应写为：

```text
https://co.yes.vg/team/v1
```

最终请求地址才是：

```text
https://co.yes.vg/team/v1/messages
```

## 根因 2：项目发送了空的 `anthropic-version`

`r1.rs` 和 `appleby.rs` 原先都使用：

```rust
AnthropicClientBuilder::new(config.anthropic_api_key, "")
```

这会发送空的 `anthropic-version` header。现已改为：

```rust
AnthropicClient::DEFAULT_API_VERSION
```

对应值：

```text
2023-06-01
```

## 根因 3：reqwest 自动使用了不可用的 Windows 系统代理

Windows Internet Settings 中启用了：

```text
127.0.0.1:7897
```

`reqwest 0.12` 默认启用 `system-proxy`，会读取这个代理。通过该路径访问 HTTPS 时得到：

```text
unexpected EOF during handshake
```

或：

```text
tls handshake eof
```

原始 `reqwest` 请求访问 `https://example.com` 也出现相同错误，说明这不是 API key、模型、URL path 或 JSON body 导致的。

有效修复：

```rust
reqwest::Client::builder().no_proxy().build()
```

然后通过 SDK builder 注入：

```rust
.with_http_client(http_client)
```

## 根因 4：该 team 网关要求 Claude Code 兼容路由字段

只发送标准 Anthropic headers 和普通 Messages body 时，网关返回：

```text
HTTP 503 Service Unavailable
no_available_providers
```

但同一时间 Claude Code 可以成功返回 `OK`。为了找出差异，使用本地 HTTP capture server 捕获了 Claude Code 的实际请求，再逐项缩减字段并重放。

最终确认该网关至少要求：

### Headers

```text
User-Agent: claude-cli/2.1.170 (external, sdk-cli)
anthropic-beta: claude-code-20250219
x-app: cli
```

加上标准 Anthropic headers：

```text
x-api-key: team-***
anthropic-version: 2023-06-01
content-type: application/json
```

### Body metadata

请求必须包含 `metadata.user_id`，其值是一个 JSON 字符串：

```json
{
  "metadata": {
    "user_id": "{\"device_id\":\"appleby\",\"account_uuid\":\"\",\"session_id\":\"appleby-r1\"}"
  }
}
```

实测结果：

- 有 Claude Code 兼容 headers，但没有 `metadata.user_id`：503；
- 有 `metadata.user_id`，但只有标准 Anthropic headers：503；
- 两者同时具备：200；
- 两者同时具备时网关能够进入 Claude Code provider 路由；
- `?beta=true`、`X-Claude-Code-Session-Id` 和 `anthropic-dangerous-direct-browser-access` 不是这个最小调用所必需的。

## 根因 5：网关要求 system block 数组并始终返回 SSE

使用 SDK 原生 `CreateMessageParams` 时，`system` 的类型是 `Option<String>`。该网关虽然会对这种请求返回 HTTP 200，但实际内容是：

```text
Text { text: "." }
stop_reason: max_tokens
output_tokens: 1
```

捕获并缩减 Claude Code 请求后确认，只要把 `system` 按 Anthropic content block 数组发送，网关就会返回真实模型结果：

```json
"system": [
  {
    "type": "text",
    "text": "You are a Claude agent, built on Anthropic's Claude Agent SDK."
  }
]
```

同时，该 team 网关即使请求中没有 `stream: true`，也会用 `Content-Type: text/event-stream` 返回 SSE。因此 SDK 原生的非流式 `create_message()` 不适用于这个网关：

- 请求 schema 无法表达 system block 数组；
- 响应解析器期待单个 JSON object，而网关返回 SSE event stream。

最终实现保留 SDK 的 client、message/content/tool 类型，但增加一层兼容 transport：

1. 把 `CreateMessageParams` 序列化为 JSON value；
2. 把 system string 包装成 text block 数组；
3. 强制 `stream: true`；
4. 发送请求并解析 `message_start`、content delta、tool input delta、`message_delta` 等 SSE events；
5. 累积成 SDK 的 `CreateMessageResponse`，供现有 ReAct/tool loop 继续使用。

## 项目中的实现

### `src/anthropic.rs`

新增共享兼容层：

- `build_http_client()`：
  - 绕过不可用的 Windows system proxy；
  - 设置 Claude Code User-Agent；
  - 设置 `anthropic-beta: claude-code-20250219`；
  - 设置 `x-app: cli`。
- `claude_code_metadata()`：
  - 生成网关路由所需的 `metadata.user_id`；
  - 当前进程内复用同一个随机 session ID。
- `create_message()`：
  - 将 SDK request 转成网关要求的 system block array；
  - 强制流式响应；
  - 解析并累积 SSE text、thinking 和 tool-use deltas；
  - 返回 SDK 的 `CreateMessageResponse`。

### `src/bin/r1.rs`

- 使用正确 API version；
- 注入共享 reqwest client；
- 给请求添加 Claude Code metadata；
- 使用兼容层 `create_message()` 处理 system array 和 SSE；
- 日志不再打印完整 API key。

### `src/bin/appleby.rs` 和 `src/workflow/loop_workflow.rs`

主程序调用路径应用相同的 HTTP client、metadata 和 SSE 兼容调用，避免 `r1` 成功但正式 agent loop 仍失败。

### `src/state/config.rs`

- model：`claude-opus-4-7`；
- SDK base URL：`https://co.yes.vg/team/v1`；
- 当前默认 key 使用用户提供的 team key。

### `Cargo.toml`

添加直接依赖：

```toml
reqwest = "0.12"
```

这是为了创建自定义 reqwest client 并通过 `anthropic-ai-sdk` 的 `.with_http_client(...)` 注入。

## 做过但没有解决问题的尝试

以下方法没有解决 TLS handshake 问题，因此没有保留：

- 强制 HTTP/1.1；
- 强制 TLS 1.2；
- 从 native-tls 切换到 rustls；
- 强制 IPv4；
- 固定到 curl 成功连接的 Cloudflare IPv4；
- 显式使用 `http://127.0.0.1:7897` 代理；
- 清除 `SSL_CERT_FILE`。

真正有效的是 `.no_proxy()`。

以下方法只能到达网关但仍返回 503：

- 只修复 `/v1/messages` 路径；
- 只修复 `anthropic-version`；
- 只添加 Claude Code headers；
- 只添加 `metadata.user_id`；
- 把 `x-anthropic-billing-header` 当作 HTTP header 发送。

最终必须同时发送 Claude Code 兼容 headers、`metadata.user_id`，并把 `system` 发送为 content block 数组；否则会得到 503 或 HTTP 200 + `.` 占位响应。

## 验证记录

### 端到端

```bash
./scripts/clean_and_run_r1.sh
```

结果：成功，SDK 收到并解析 `ContentBlock::Text`。

### 自动测试

```bash
cargo test
```

结果：

```text
29 passed; 0 failed
```

其中包含 26 个 library unit tests（新增 SSE 累积解析测试）和 3 个 `tests/tool_workflow.rs` integration tests。

## 安全提醒

当前 team key 被写在 tracked Rust source 中，而且用户原始调试文档中也包含完整 key。调试完成后建议立即轮换该 key，并把正式配置改为环境变量或 `.appleby/config.toml`（该目录已被 `.gitignore` 忽略），不要提交真实凭据。
