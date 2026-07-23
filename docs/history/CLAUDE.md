# Appleby Project Notes

## Project Overview

Appleby is a Rust CLI agent runtime. It runs an interactive ReAct-style loop, keeps conversation context on disk, and exposes local tools to the model.

Current direction:

- The application is **provider-independent at the core**.
- Provider SDK details are isolated under `src/api_adapter/`.
- The current concrete backend is OpenAI-compatible Chat Completions through `async-openai`.
- Anthropic-specific code has been removed from the Rust code path.
- `bin` owns app wiring, file paths, startup modes, and other deployment choices; `lib` receives explicit dependencies.
- The TUI is a projection of agent/runtime state, not a second conversation authority.

## Consistency Rules and Priority

This file contains both long-lived rules and current implementation notes. Keep them distinct:

- **Normative rules** define the consistency the code must approach and preserve.
- **Current implementation notes** describe today's files, types, and runtime flow; update them when the implementation changes.
- Do not preserve a stale implementation merely because it is described here. Preserve the ranked normative rules instead.

When the code cannot satisfy every rule at once, use this order. A lower-ranked rule must not be used to justify violating a higher-ranked rule.

1. **Durable protocol validity and recoverability**
   - Persisted conversation records and provider-bound message sequences must be semantically valid.
   - Tool-call/result links must be reconstructible across process restarts.
   - Truncated, filtered, refused, malformed, or otherwise incomplete provider responses must not be recorded as normal completed turns.
2. **Single authority and commit ordering**
   - `ConversationContext` is the authority for committed conversation messages.
   - Workflow/runner code is the only writer of conversation messages.
   - TUI state is a projection; pending UI state must not be presented as committed state.
3. **Provider boundary and Appleby-owned schema**
   - Provider SDK types stay in provider adapters.
   - Core runtime, tools, events, and durable JSONL use Appleby-owned types.
4. **Decision ownership and dependency direction**
   - `bin` chooses deployment policy and constructs dependencies.
   - workflow orchestrates, state stores runtime state, adapters translate providers, tools act, and TUI presents state and emits user commands.
5. **Lifecycle, completion, and event-correlation semantics**
   - Event names, payloads, statuses, shutdown, cancellation, success, and failure must describe actual runtime behavior.
   - Correlation IDs are required only when the concurrency model needs them, and must be consumed when present.
6. **Explicit long-running resource policy**
   - Channels, model/tool loops, provider request context, and TUI history must have explicit backpressure, termination, and retention policies.
7. **Direct types, narrow APIs, and no speculative abstractions**
   - Prefer the smallest direct design that still satisfies rules 1-6.
   - Code reduction is not a reason to weaken protocol or ownership guarantees.
8. **Terminal lifecycle safety**
   - TUI setup and teardown must restore terminal state on normal and error paths.
9. **Verification and documentation synchronization**
   - Tests, verification commands, and current implementation notes must agree with the code before work is considered complete.

Use the following format when proposing a required change:

```text
Target rule:
Current evidence:
Violation:
Required change:
Acceptance check:
Non-goals:
```

Do not describe a change as an "optimization" unless there is a defined target rule and evidence that the current code violates it. If evidence is insufficient, classify the topic as a specification gap or optional refactor, not as a bug.

## Bin / Lib Separation

`src/bin/appleby.rs` is responsible for application startup policy and the application data root:

- `.appleby`

Each file-backed state component receives that root explicitly and may derive only its own intrinsic child location:

- `Config` → `config.toml`
- `SystemPrompt` → `system_prompt.txt`
- `ConversationContext` → `context.jsonl`

Logging destination and file prefix are deployment choices rather than intrinsic state-component names. The binary chooses and passes both explicitly.

The library must not choose `.appleby` or read global singletons. Components may keep private constants for intrinsic child names and provide reusable explicit-path APIs, but the binary chooses the root, startup modes, logging destination, and other deployment policy, constructs objects, and passes dependencies into the library.

`LoopState::new` should stay dependency-injected. It receives already-constructed runtime dependencies:

- `Box<dyn ApiAdapter>`
- tool map
- model string
- system prompt string
- `ConversationContext`

Do not make `LoopState::new` read config, system prompt, context file paths, global singletons, or boolean startup flags.

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

Required high-level flow:

1. `src/bin/appleby.rs` parses CLI args and chooses `ContextLoadMode`.
2. It loads config and system prompt from caller-selected paths.
3. It chooses logging destination and file prefix explicitly.
4. It constructs tools, the OpenAI adapter, and a file-backed `ConversationContext`.
5. It constructs `LoopState` from those explicit dependencies.
6. It creates the bounded command/event channels and starts `workflow::loop_workflow::agent_loop` as the agent runner task.
7. `tui::run` owns terminal input and rendering. User actions become provider-independent `TuiCommand` values.
8. The runner persists an accepted user message before emitting the event that presents it as committed.
9. The runner prepares a valid provider-independent `ApiRequest` and calls `state.api_adapter`.
10. The OpenAI adapter converts the request to OpenAI Chat Completions format and validates provider completion semantics before returning a provider-independent assistant reply.
11. The runner persists the assistant message, emits assistant/tool lifecycle events, executes tool calls, and persists linked `ConversationMessage::Tool` results.
12. `ConversationContext` appends each committed message to JSONL before updating the in-memory context.
13. The TUI consumes `AgentEvent` values as a projection of runner and durable state; it does not write conversation context directly.

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
- conversion from OpenAI assistant responses/tool calls back to Appleby-owned reply data
- conversion from Appleby `ToolSpec` to OpenAI function tools
- building/sending Chat Completions requests
- validation of provider-specific completion semantics such as finish reason, refusal, truncation, filtering, and malformed tool arguments

`ApiAdapter::complete` is an assistant-completion boundary. Its successful return value must be structurally unable to represent a user or tool-role message. If the current `ApiResponse` type does not encode that invariant, treat that as code to align with this rule rather than as permission to persist an arbitrary `ConversationMessage`.

Provider-specific completion states must be resolved inside the adapter. Workflow should receive either a valid provider-independent assistant reply or an error; it should not interpret OpenAI SDK finish-reason types.

## Conversation Model and JSONL Persistence

The provider-independent conversation schema is:

```rust
pub enum ConversationMessage {
    User { content: String },
    Assistant {
        content: Option<String>,
        tool_calls: Vec<ToolCallRecord>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}
```

Important persistence detail:

- The `assistant` record stores the tool call intent: tool call id, tool name, and arguments.
- The `tool` record stores the result for a prior tool call: `tool_call_id` and output content.
- `tool.tool_call_id` links back to `assistant.tool_calls[*].id`.
- Tool result records intentionally do not duplicate tool name/arguments; inspect the preceding assistant record for those details.

Conversation legality rules:

- Tool-call IDs within one assistant message must be unique.
- A tool result sent to a provider must reference a preceding assistant tool call.
- Every assistant tool call included in a provider request must have exactly one corresponding tool result before the next non-tool message.
- Orphan or duplicate tool results must not be sent to a provider.
- Recovery may synthesize an explicit cancellation result for an interrupted tool call, but it must not silently rewrite unrelated user/assistant turn boundaries.
- Provider refusal, filtering, truncation, malformed replies, or assistant replies with neither visible content nor tool calls are not normal completed assistant messages.
- Persist the original Appleby-owned conversation semantics. Provider-specific request compatibility transformations belong at the adapter/request-projection boundary, not in the durable log.

Example shape:

```json
{"role":"assistant","tool_calls":[{"id":"call_1","name":"Read","arguments":{"path":"Cargo.toml","start_line":1,"end_line":80}}]}
{"role":"tool","tool_call_id":"call_1","content":"1: ..."}
```

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
- `ConversationContext`

Conversation persistence lives in:

```text
src/state/conversation_context.rs
src/utils/jsonl.rs
```

Important context types:

- `ConversationContext`
- `ContextLoadMode`

Behavior:

- `ConversationContext` directly uses `JsonlLog`; there is no separate Store abstraction.
- `ConversationContext::push` appends to the JSONL log and updates in-memory messages.
- `ConversationContext::open_jsonl(path, ContextLoadMode::LoadRecent { limit })` loads recent messages from the caller-provided path.
- `ConversationContext::open_jsonl(path, ContextLoadMode::FreshArchive)` archives the caller-provided file and starts empty.
- If `LoadRecent` encounters malformed or incompatible JSONL, it returns an error and leaves the file untouched. Users can explicitly run with `--no-load-context` to archive and start fresh.

The context module may perform file I/O for caller-provided paths, but it must not hard-code application paths or read global configuration.

Commit ordering:

- A conversation message is committed only after `ConversationContext::push` successfully appends it.
- The runner must not emit an event that presents a message as committed before that append succeeds.
- TUI-local pending input may exist, but it must be visibly distinct from committed conversation history.
- If the agent loaded prior context, the TUI must either project that context or explicitly state that prior context is loaded but hidden. It must not present a loaded conversation as an empty new conversation.

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

Tool outcome rules:

- Tool implementations return `Ok(output)` for success and `Err(error)` for failure; do not encode failures as successful strings such as `Ok("Error: ...")`.
- Tool implementations should not add presentation prefixes that the workflow will add again.
- Workflow is the single boundary that converts a tool `Result` into the provider-visible `ConversationMessage::Tool` content.
- Agent events must preserve whether a tool invocation succeeded or failed; the TUI must not infer outcome by parsing output text.

## Workflow

Agent workflow lives in:

```text
src/workflow/loop_workflow.rs
```

Responsibilities:

- receive provider-independent frontend commands
- append accepted user messages through `LoopState`
- prepare a legal provider request projection from durable conversation history
- construct provider-independent `ApiRequest`
- call `ApiAdapter::complete`
- validate provider-independent assistant reply invariants that are not provider-specific
- append assistant messages
- execute returned tool calls
- append linked tool results
- emit provider-independent `AgentEvent` values after the corresponding state transition

Conversation repair is limited to protocol legality and crash recovery:

- drop orphan tool results without a prior assistant tool call
- insert explicit cancellation results for unanswered tool calls when recovery requires a complete provider request
- reject or repair duplicate/mismatched tool-call/result relationships without leaving the original assistant message internally inconsistent
- never merge tool-role messages into user/assistant messages

Do not merge adjacent user or assistant messages merely to reduce message count. Preserve durable turn boundaries by default. If a specific provider requires same-role merging, perform that provider compatibility transformation at the adapter/request-projection boundary and test it there.

Long-running policy:

- A model/tool turn must have an explicit termination policy; do not rely on an unbounded tool-call loop.
- Provider request context must have an explicit window or token-budget policy distinct from durable JSONL retention.
- Context selection must preserve complete assistant tool-call/result groups.
- Deployment values such as maximum tool rounds or request budget are chosen by the binary/runtime policy and passed explicitly when they are configurable.

## TUI and Agent Event Boundary

TUI implementation lives under:

```text
src/tui/
```

Frontend/runner commands and events currently live in:

```text
src/workflow/tui_channel.rs
```

TUI responsibilities:

- own terminal lifecycle, keyboard input, view-local input state, scrolling, and rendering
- convert user actions into provider-independent commands
- project committed conversation and runtime lifecycle events into visible state
- keep view-only state out of durable conversation JSONL

TUI must not:

- write `ConversationContext` directly
- interpret OpenAI SDK types or provider finish reasons
- decide tool-call/result legality
- execute tools
- present an uncommitted message as an ordinary committed timeline entry

AgentEvent rules:

- Each event must define whether it is emitted before or after persistence/state commit.
- Event payloads must represent only states legal for that event. An assistant-completed event must not accept user/tool-role messages; a tool-completed event must not accept assistant/user-role messages.
- Event names must distinguish started, committed, succeeded, failed, cancelled, and stopped states when those distinctions affect behavior.
- If the runner is strictly serial, ordered channels are sufficient and unused turn IDs should not be added speculatively.
- If streaming, concurrent turns, or cancellation can produce stale events, events must carry a turn/generation ID and the TUI must actively reject events for inactive generations.

Lifecycle rules:

- `TurnCompleted` means a semantically valid turn completed, not merely that an API future returned.
- Tool failure remains a tool result for provider conversation completeness, but its failure outcome must remain visible to workflow and TUI.
- Graceful shutdown-after-turn and cancellation-of-current-turn are different commands and statuses.
- Do not claim cancellation unless the API/tool future is actually cancellable and the persisted partial-turn policy is defined.

History and resource rules:

- At startup, the TUI must receive a snapshot of loaded conversation state or clearly disclose that loaded history is hidden.
- TUI timeline retention must be an explicit product/runtime policy: full session, bounded recent entries, or on-demand durable-history loading.
- Command and event channels must be bounded unless event volume is proven to be strictly bounded by another mechanism.
- Redraw frequency and render allocation changes require a declared resource target or measured problem; code shape alone is not evidence of a defect.

Terminal rules:

- Use RAII to restore raw mode, alternate screen, and cursor state on normal and error exits.
- Renderer code maps App state to widgets; it does not own terminal setup, provider calls, persistence, or tool execution.

## Config and System Prompt

Configuration type lives in:

```text
src/state/config.rs
```

System prompt type lives in:

```text
src/state/system_prompt.rs
```

Both expose explicit path-based loading APIs and should not be accessed through global singletons.

Current config fields:

- `openai_api_key`
- `openai_base_url`
- `openai_model`

Environment seed/refresh variables:

- `OPENAI_API_KEY`
- `OPENAI_BASE_URL`
- `OPENAI_MODEL`

Behavior:

- If the config file is absent, all three non-empty environment variables are required; Appleby writes their values into the new config file.
- If the config file exists, any non-empty environment variable overrides only its matching config field and persists that refresh.
- If no environment variables are supplied, Appleby reads the existing config file unchanged.
- There are no hard-coded credential, endpoint, model, or fallback defaults.

Old `anthropic_*` config keys are accepted through serde aliases during transition, but new code should use `openai_*` names.

## Smoke Binary

`src/bin/r1.rs` is a small OpenAI-compatible smoke test. It constructs the OpenAI adapter and sends a simple prompt through the provider-independent `ApiAdapter` interface.

Run only when a working API key/base URL/model is configured:

```bash
cargo run --bin r1
```

## Logging

Tracing setup lives in:

```text
src/state/log.rs
```

The binary must choose and pass both the log directory and file prefix explicitly:

```rust
const LOG_DIR: &str = ".appleby/logs";
const LOG_FILE_PREFIX: &str = "appleby.log";

appleby::state::log::init_with_prefix(LOG_DIR, LOG_FILE_PREFIX)
```

Do not use a library convenience API that receives only the app root and silently chooses the logging destination or prefix. Conversation context JSONL is separate from tracing logs.

## Verification Commands

Run before considering changes complete:

```bash
cargo fmt
cargo test
cargo run --bin appleby -- --help
```

For TUI/agent protocol changes, tests should cover the affected consistency rule rather than only rendering text. Relevant cases include:

- a user message is not presented as committed when persistence fails
- loaded context is projected or explicitly disclosed at TUI startup
- provider truncation/refusal/filtering does not produce `TurnCompleted`
- duplicate or orphan tool-call/result relationships are rejected or repaired into a legal request
- tool success and failure remain distinguishable in workflow and TUI events
- shutdown-after-turn and cancellation behavior match their names
- turn/generation IDs, when present, reject stale events
- context-window selection preserves complete tool-call/result groups

Boundary checks:

```bash
rg "CONFIG|SYSTEM_PROMPT|CONTEXT_FILE|CONTEXT_LOAD_LIMIT|load_previous_context" src
rg "\.appleby" src/state src/workflow src/api_adapter src/tool
rg "async_openai" src
rg "anthropic_ai_sdk|anthropic-ai-sdk|AnthropicClient|ContentBlock|StopReason" src Cargo.toml Cargo.lock
```

Expected:

- The application root constant should live in `src/bin/*`; library modules may only own private intrinsic child names and must receive the root explicitly.
- `async_openai` should normally appear only in `src/api_adapter/openai.rs`.
- Anthropic SDK/code symbols should not appear in Rust/Cargo files.

## Clean Code Design Principles

Use these principles when changing Appleby. These principles are subordinate to the ranked consistency rules above: they choose between already-correct designs and do not justify weakening protocol validity, authority, or ownership boundaries.

- Every required change should identify its target consistency rule, current evidence, acceptance check, and non-goals.
- Distinguish durable conversation history, provider request projection, runtime lifecycle state, and TUI display state; do not collapse them into one type merely to reduce code.
- At protocol boundaries, prefer types that cannot represent illegal roles or lifecycle states.
- Keep ownership of decisions at the right layer: `bin` decides deployment/startup details; `lib` implements reusable behavior from explicit inputs.
- Prefer direct concrete types when there is only one real implementation. Do not introduce traits like Store/Repository/Manager unless there is a real second implementation or a test seam that cannot be simpler.
- Use traits only at true architectural boundaries, such as provider adapters (`ApiAdapter`) or runtime tools (`Tool`).
- Keep persistent schemas Appleby-owned and small. `ConversationMessage` is the durable context schema; provider SDK structs are adapter details.
- Avoid global singletons in library code. Pass config, paths, model names, prompts, adapters, and context explicitly.
- Avoid boolean parameters for meaningful modes. Use enums such as `ContextLoadMode` so call sites describe intent.
- Keep modules at one altitude: workflow orchestrates, state stores runtime state, adapters translate providers, tools perform actions.
- Avoid speculative extensibility. If the project does not expect another implementation, keep the design simple and concrete.
- Prefer narrow public APIs. Make helper functions private unless another module genuinely needs them.
- Keep the application-root constant in binaries. A reusable library type may keep private child-name constants when those names are intrinsic to that type, but callers must pass the root explicitly.
- When storing event logs, prefer append-only records that can reconstruct the conversation without relying on tracing logs.
- Keep tests focused on externally important behavior rather than implementation details.

## Design Rules for Future Changes

- Keep provider-independent API concepts in `api_adapter/conversation.rs`.
- Keep provider-specific SDK usage inside provider adapter modules, currently `api_adapter/openai.rs`.
- Do not let `state`, `workflow`, or `tool` depend on OpenAI SDK request/response structs.
- Keep persisted JSONL context in Appleby-owned `ConversationMessage` format, not raw SDK structs.
- If adding another provider, implement `ApiAdapter` in a new module under `src/api_adapter/`.
- If adding new tools, return Appleby `ToolSpec`; add provider-specific conversion only inside adapters.
- Let binaries choose app paths, config files, context load modes, log file prefix, logging destinations, and configurable long-running budgets; keep library constructors explicit and testable.
- Keep TUI commands and Agent events provider-independent.
- Emit committed-message events only after the corresponding `ConversationContext::push` succeeds.
- Make event payloads role-specific so assistant/tool/user lifecycle events cannot carry the wrong conversation role.
- Use turn/generation IDs only when the runtime can produce stale or concurrent events, and require the consumer to validate them.
- Keep durable history retention separate from provider request-window and TUI history-retention policies.
- Do not classify unmeasured performance ideas or style-only rewrites as required changes; first tie them to a ranked consistency rule and an acceptance check.
