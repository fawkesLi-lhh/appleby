# Appleby Project Notes

## Project Overview

Appleby is a Rust CLI agent runtime. It runs an interactive ReAct-style loop, keeps conversation context on disk, and exposes local tools to the model.

Current direction:

- The application is **provider-independent at the core**.
- Provider SDK details are isolated under `src/api_adapter/`.
- The current concrete backend is OpenAI-compatible Chat Completions through `async-openai`.
- Anthropic-specific code has been removed from the Rust code path.

## Main Runtime Flow

Primary binary:

```bash
cargo run --bin appleby
```

Fresh context startup:

```bash
cargo run --bin appleby -- --no-load-context
```

Help:

```bash
cargo run --bin appleby -- --help
```

High-level flow:

1. `src/bin/appleby.rs` initializes logging/config/tool map/API adapter.
2. It constructs `LoopState`.
3. `workflow::loop_workflow::loop_workflow` starts the CLI REPL.
4. User input becomes a `ConversationMessage::User`.
5. `agent_loop` sends a provider-independent `ApiRequest` to `state.api_adapter`.
6. The current OpenAI adapter converts the request to OpenAI Chat Completions format.
7. Assistant replies are converted back into provider-independent `ConversationMessage` values.
8. Tool calls are executed through Appleby tools and appended as `ConversationMessage::Tool`.
9. Each message is appended to `.appleby/context.jsonl` in real time.

## Provider Boundary

Provider-independent API abstractions live in:

```text
src/api_adapter/conversation.rs
```

Important types:

- `ConversationMessage`
- `ToolCallRecord`
- `ApiRequest`
- `ApiResponse`
- `ApiAdapter`

The OpenAI-specific implementation lives in:

```text
src/api_adapter/openai.rs
```

This should be the only normal module that imports `async_openai` types. Keep `state`, `workflow`, and `tool` free of provider SDK structs.

The OpenAI adapter owns:

- `async_openai::Client<OpenAIConfig>`
- conversion from `ConversationMessage` to OpenAI request messages
- conversion from OpenAI assistant responses/tool calls back to `ConversationMessage`
- conversion from Appleby `ToolSpec` to OpenAI function tools
- building/sending Chat Completions requests

## State and Context

Runtime state lives in:

```text
src/state/loop_state.rs
```

`LoopState` owns:

- `api_adapter: Box<dyn ApiAdapter>`
- registered local tools
- model name
- system prompt
- in-memory conversation context
- JSONL context log handle

Conversation persistence lives in:

```text
src/state/conversation_context.rs
src/utils/jsonl.rs
```

Context file:

```text
.appleby/context.jsonl
```

Behavior:

- Every pushed conversation message is appended to JSONL immediately.
- Normal startup loads the last 20 messages from `.appleby/context.jsonl`.
- `--no-load-context` archives the old context file with a timestamp suffix and starts fresh.
- If the context file contains an incompatible legacy format, it is archived and Appleby starts with an empty context.

## Tools

Tool definitions and registration live under:

```text
src/tool/
```

Registered tools:

- `Bash`
- `Read`
- `Write`
- `Edit`

Tool trait and provider-independent schema:

```text
src/tool/mod.rs
```

`ToolSpec` is Appleby-owned and should remain provider-neutral:

```rust
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}
```

Do not add OpenAI/Anthropic SDK types to tool implementations. Provider-specific tool schema conversion belongs in `src/api_adapter/openai.rs` or another provider adapter.

## Workflow

Agent workflow lives in:

```text
src/workflow/loop_workflow.rs
```

Responsibilities:

- read CLI user input
- append user messages
- normalize conversation history
- construct provider-independent `ApiRequest`
- call `ApiAdapter::complete`
- append assistant messages
- execute returned tool calls
- append tool results

`normalize_messages` repairs tool-call context:

- drops orphan tool results without a prior assistant tool call
- inserts `(cancelled)` tool results for unanswered tool calls
- merges consecutive plain user messages
- merges consecutive plain assistant text messages only when neither side has tool calls
- never merges tool-role messages into user/assistant messages

## Config

Configuration lives in:

```text
src/state/config.rs
.appleby/config.toml
```

Current config fields:

- `openai_api_key`
- `openai_base_url`
- `openai_model`

Environment defaults:

- `OPENAI_API_KEY`
- `OPENAI_BASE_URL`
- `OPENAI_MODEL`

Old `anthropic_*` config keys are accepted through serde aliases during transition, but new code should use `openai_*` names.

## Smoke Binary

`src/bin/r1.rs` is a small OpenAI-compatible smoke test. It constructs the same OpenAI adapter path and sends a simple prompt.

Run only when a working API key/base URL/model is configured:

```bash
cargo run --bin r1
```

## Logging

Tracing logs are written under:

```text
.appleby/logs
```

Logging setup:

```text
src/state/log.rs
```

Conversation context JSONL is separate from tracing logs.

## Verification Commands

Run before considering changes complete:

```bash
cargo fmt
cargo test
cargo run --bin appleby -- --help
```

Boundary checks:

```bash
rg "async_openai" src
rg "anthropic_ai_sdk|anthropic-ai-sdk|AnthropicClient|ContentBlock|StopReason" src Cargo.toml Cargo.lock
```

Expected:

- `async_openai` should normally appear only in `src/api_adapter/openai.rs`.
- Anthropic SDK/code symbols should not appear in Rust/Cargo files.

## Design Rules for Future Changes

- Keep provider-independent concepts in `api_adapter/conversation.rs`.
- Keep provider-specific SDK usage inside provider adapter modules, currently `api_adapter/openai.rs`.
- Do not let `state`, `workflow`, or `tool` depend on OpenAI SDK request/response structs.
- Keep persisted JSONL context in Appleby-owned `ConversationMessage` format, not raw SDK structs.
- If adding another provider, implement `ApiAdapter` in a new module under `src/api_adapter/`.
- If adding new tools, return Appleby `ToolSpec`; add provider-specific conversion only inside adapters.
