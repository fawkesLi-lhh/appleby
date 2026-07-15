# 当前 ReAct 架构说明

> 生成日期：2026-07-16  
> 说明：本文基于当前 `src/` 代码整理，介绍 Appleby 当前的 **ReAct（Reason + Act）式 AI Agent 架构**。这里的 ReAct 不是前端 React，而是大模型 Agent 中“推理 → 行动 → 观察 → 再推理”的工作模式。

## 1. 架构概览

Appleby 当前是一个 Rust CLI AI Agent 项目，核心能力是：

1. 接收用户在终端输入的问题；
2. 将对话上下文、系统提示词和工具定义发送给模型；
3. 如果模型返回工具调用请求，则执行本地工具；
4. 将工具结果回填给模型；
5. 重复上述过程，直到模型返回最终回答。

整体结构如下：

```text
src/
├── bin/
│   ├── appleby.rs          # 主入口：初始化配置、日志、client、工具和 LoopState
│   └── r1.rs               # 简单 Anthropic API 调用示例
├── lib.rs                  # crate 入口，导出 state / tool / workflow
├── state/
│   ├── config.rs           # Anthropic API 配置
│   ├── log.rs              # tracing 日志初始化
│   ├── loop_state.rs       # Agent 运行时状态、上下文、工具执行入口
│   ├── mod.rs
│   └── system_prompt.rs    # 系统提示词加载
├── tool/
│   ├── bash.rs             # bash 工具
│   ├── edit_file.rs        # 文件精确替换工具
│   ├── mod.rs              # Tool trait、工具注册表、安全路径检查
│   ├── read_file.rs        # 文件读取工具
│   └── write_file.rs       # 文件写入工具
└── workflow/
    ├── loop_workflow.rs    # 用户交互循环和 Agent Loop
    └── mod.rs
```

## 2. ReAct 主流程

当前 ReAct 流程主要由 `workflow::loop_workflow`、`workflow::agent_loop` 和 `state::LoopState` 协作完成。

```text
Human 输入
   │
   ▼
loop_workflow 读取用户问题
   │
   ▼
LoopState.push_message(User)
   │
   ▼
agent_loop
   │
   ├─ Reason：调用模型，让模型基于上下文推理
   │
   ├─ Act：如果模型返回 tool_use，执行工具
   │
   ├─ Observe：将工具输出包装成 tool_result 回填上下文
   │
   └─ Repeat：继续调用模型，直到没有 tool_use
   │
   ▼
输出 Assistant 最终回答
```

对应到 ReAct 概念：

| ReAct 阶段 | 当前代码中的体现 |
| --- | --- |
| Reason / Thought | `agent_loop` 调用 `state.client.create_message(...)`，模型基于上下文、系统提示词、工具 schema 生成下一步内容 |
| Act | 模型返回 `ContentBlock::ToolUse`，并且 `stop_reason == StopReason::ToolUse` |
| Tool Execution | `LoopState::execute_tool_call` 遍历 tool use，调用 `LoopState::execute` 执行具体工具 |
| Observe | 工具执行结果被包装为 `ContentBlock::ToolResult` |
| Continue / Final | `tool_result` 作为新的 `User` 消息加入上下文，继续调用模型；如果不再需要工具，则返回最终回答 |

## 3. 入口层：`src/bin/appleby.rs`

`appleby.rs` 是主程序入口，代码流程较薄：

```rust
let _log_guard = appleby::state::log::init();

let config = appleby::state::config::CONFIG.clone();
let client = AnthropicClientBuilder::new(config.anthropic_api_key, "")
    .with_api_base_url(config.anthropic_base_url)
    .build::<MessageError>()?;
let tools = toolmap();
let mut loop_state = LoopState::new(client, tools);
loop_workflow(&mut loop_state).await?;
```

它主要负责：

1. 初始化日志；
2. 加载 `.appleby/config.toml` 配置；
3. 创建 Anthropic client；
4. 注册工具；
5. 初始化 `LoopState`；
6. 启动交互式 ReAct workflow。

## 4. Workflow 层

核心文件：`src/workflow/loop_workflow.rs`。

### 4.1 用户交互循环：`loop_workflow`

`loop_workflow` 是外层 CLI REPL：

```rust
pub async fn loop_workflow(state: &mut LoopState) -> Result<(), anyhow::Error> {
    loop {
        let query = Text::new("Human: ").prompt()?;

        if query.trim() == "exit()" {
            break;
        }
        state.push_message(Message::new_text(Role::User, query));
        agent_loop(state).await?;
        let Some(final_content) = state.get_context().last() else {
            continue;
        };
        println!("Assistant: {}", extract_text(&final_content.content));
    }
    Ok(())
}
```

职责：

- 读取终端用户输入；
- 遇到 `exit()` 退出；
- 把用户输入写入上下文；
- 调用 `agent_loop` 完成一轮或多轮模型/工具交互；
- 输出 assistant 最终文本。

### 4.2 ReAct 循环：`agent_loop`

`agent_loop` 是 ReAct 的核心实现。

简化流程：

```rust
loop {
    let request = CreateMessageParams::new(...)
        .with_system(state.system_prompt.clone())
        .with_tools(state.tools.values().map(|tool| tool.tool_spec()).collect());

    let response = state.client.create_message(Some(&request)).await?;
    let message = Message::new_blocks(Role::Assistant, response.content.clone());
    state.push_message(message.clone());

    if stop_reason 不是 ToolUse {
        return Ok(());
    }

    let tool_result = state.execute_tool_call(&response.content).await?;
    state.push_message(Message::new_blocks(Role::User, tool_result));
}
```

它完成了：

1. **构建模型请求**  
   请求中包含：
   - `model`；
   - 归一化后的历史上下文；
   - `max_tokens`；
   - system prompt；
   - 所有工具的 schema。

2. **模型推理**  
   调用：

   ```rust
   state.client.create_message(Some(&request)).await?
   ```

3. **保存 assistant 消息**  
   模型返回内容以 `Assistant` 消息形式加入上下文。

4. **判断是否需要工具**  
   如果 `response.stop_reason != ToolUse`，说明模型已经给出最终回答，本轮结束。

5. **执行工具并回填观察结果**  
   如果是 `ToolUse`，则执行工具，并把结果作为新的 `User` 消息追加到上下文，继续循环。

## 5. State 层

核心文件：`src/state/loop_state.rs`。

### 5.1 `LoopState` 结构

```rust
pub struct LoopState {
    pub client: AnthropicClient,
    pub tools: HashMap<String, Box<dyn Tool>>,
    pub model: String,
    pub system_prompt: String,
    context: Vec<Message>,
    random_id: i32,
}
```

字段说明：

| 字段 | 说明 |
| --- | --- |
| `client` | Anthropic API client |
| `tools` | 工具注册表，key 是工具名，value 是工具实例 |
| `model` | 当前使用的模型名 |
| `system_prompt` | 系统提示词 |
| `context` | 当前对话上下文 |
| `random_id` | 日志关联 ID，用于区分不同会话 |

### 5.2 上下文管理

`push_message` 会做两件事：

1. 将消息格式化写入日志；
2. 将消息追加到 `context`。

```rust
pub fn push_message(&mut self, message: Message) {
    info!(
        "Random ID: {}: Pushing message:\n{}",
        self.random_id,
        format_message_for_log(&message)
    );
    self.context.push(message);
}
```

因此，模型的每次输入、输出、工具调用结果都会被保存到上下文中，形成 ReAct 循环的记忆基础。

### 5.3 工具执行

模型返回的 `ContentBlock::ToolUse` 会由 `execute_tool_call` 处理：

```rust
pub async fn execute_tool_call(
    &mut self,
    content: &[ContentBlock],
) -> Result<Vec<ContentBlock>> {
    let mut result = Vec::new();
    for block in content {
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                let output = self.execute(name, input).await?;
                result.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: output,
                });
            }
            _ => {}
        }
    }
    Ok(result)
}
```

关键点：

- 一个 assistant 响应里可以包含多个 `ToolUse` block；
- 每个工具调用结果都转换成一个 `ToolResult` block；
- `tool_use_id` 会保持与原始 `id` 对应，便于模型识别哪个观察结果属于哪个工具动作。

真正执行工具的是 `execute`：

```rust
pub async fn execute(&mut self, name: &str, input: &serde_json::Value) -> Result<String> {
    let Some(tool) = self.tools.get_mut(name) else {
        anyhow::bail!("Unknown tool: {name}");
    };

    let mut buf = String::new();
    tool.show_to_human(&mut buf, input)?;
    println!("Assistant ToolUse:{}", buf);

    tool.invoke(input)
        .await
        .context(format!("Error invoking tool {name}"))
}
```

## 6. Tool 层

### 6.1 工具统一接口

所有工具都实现 `Tool` trait：

```rust
#[async_trait]
pub trait Tool {
    async fn invoke(&self, input: &Value) -> Result<String>;
    fn name(&self) -> Cow<'_, str>;
    fn tool_spec(&self) -> ToolSpec;
    fn show_to_human(&self, writer: &mut dyn Write, input: &Value) -> Result<(), anyhow::Error>;
}
```

| 方法 | 职责 |
| --- | --- |
| `invoke` | 接收 JSON input，执行工具逻辑，返回字符串结果 |
| `name` | 返回工具名 |
| `tool_spec` | 返回给模型的工具 schema |
| `show_to_human` | 将工具调用以可读形式打印给终端用户 |

### 6.2 工具注册表

`toolmap()` 当前注册 4 个工具：

```rust
pub fn toolmap() -> HashMap<String, Box<dyn Tool>> {
    HashMap::from([
        ("bash".to_string(), bash_tool()),
        ("read_file".to_string(), read_file_tool()),
        ("write_file".to_string(), write_file_tool()),
        ("edit_file".to_string(), edit_file_tool()),
    ])
}
```

### 6.3 当前内置工具

| 工具名 | 文件 | 功能 | 主要限制 |
| --- | --- | --- | --- |
| `bash` | `tool/bash.rs` | 在当前 workspace 执行 shell 命令 | 拦截部分危险命令；120 秒超时；输出最多 50000 字符 |
| `read_file` | `tool/read_file.rs` | 按行读取文件内容 | 路径不能逃逸 workspace；支持 `start_line` / `end_line` |
| `write_file` | `tool/write_file.rs` | 写入文件内容 | 路径不能逃逸 workspace；会覆盖已有文件；自动创建父目录 |
| `edit_file` | `tool/edit_file.rs` | 精确替换文件中的字符串 | 路径不能逃逸 workspace；默认要求 `old_string` 唯一；可 `replace_all` |

### 6.4 路径安全检查

文件类工具都使用 `safe_path(path)`：

- 以当前工作目录作为 workspace 根；
- 支持目标文件不存在的情况；
- 拒绝 `..` 或绝对路径逃逸当前 workspace；
- 防止模型工具调用读写项目外部文件。

这是当前工具安全边界中比较关键的一层。

## 7. 消息归一化

`normalize_messages(messages)` 在每次请求模型前执行。

它主要解决两个问题。

### 7.1 补全孤立的 `tool_use`

如果上下文里存在 assistant 的 `tool_use`，但没有对应的 `tool_result`，代码会自动补一个：

```text
ToolResult(content = "(cancelled)")
```

这样可以避免 API 侧因为工具调用缺少结果而拒绝请求。

### 7.2 合并连续同角色消息

连续的 `User` 或连续的 `Assistant` 消息会被合并。

合并规则：

| 情况 | 合并方式 |
| --- | --- |
| Text + Text | 用换行拼接 |
| Blocks + Blocks | 合并 block 数组 |
| Text + Blocks | 将 Text 转为 `ContentBlock::Text` 后合并 |
| Blocks + Text | 追加一个 `ContentBlock::Text` |

这能让上下文更符合模型 API 的消息序列要求。

## 8. 配置与系统提示词

### 8.1 配置

配置文件路径：

```text
.appleby/config.toml
```

字段：

```toml
anthropic_api_key = "..."
anthropic_model = "..."
anthropic_base_url = "..."
```

首次运行时，如果配置不存在，会写入默认配置。

### 8.2 系统提示词

系统提示词路径：

```text
.appleby/system_prompt.txt
```

默认内容：

```text
You are a helpful assistant. You can use the following tools to help the user.
```

`LoopState::new` 会读取 `SYSTEM_PROMPT` 并保存到 `state.system_prompt`，之后每次模型请求都会通过 `.with_system(...)` 注入。

## 9. 一次完整 ReAct 请求示例

假设用户输入：

```text
阅读 src/lib.rs
```

可能的内部流程：

```text
1. Human 输入被包装成 User message

2. agent_loop 调用模型：
   messages = [User("阅读 src/lib.rs")]
   tools = [bash, read_file, write_file, edit_file]

3. 模型决定调用工具：
   Assistant:
     ToolUse {
       id: "toolu_xxx",
       name: "read_file",
       input: {
         "path": "src/lib.rs",
         "start_line": 1,
         "end_line": 100
       }
     }

4. LoopState 执行 read_file

5. 工具结果被包装成：
   User:
     ToolResult {
       tool_use_id: "toolu_xxx",
       content: "1: pub mod tool;\n2: pub mod state;..."
     }

6. agent_loop 再次调用模型

7. 模型基于文件内容生成最终回答

8. stop_reason 不再是 ToolUse，agent_loop 结束
```

## 10. 当前架构优点

- ReAct 主链路已经完整：模型推理、工具调用、观察结果回填、继续推理。
- 工具抽象统一，新增工具只需实现 `Tool` trait 并注册到 `toolmap()`。
- 文件工具具备 workspace 级路径保护。
- `LoopState` 统一保存上下文，便于多轮对话。
- 日志会记录消息内容，方便调试模型行为和工具调用过程。
- `normalize_messages` 对工具调用上下文做了容错处理。

## 11. 当前风险与改进建议

### 11.1 配置安全

当前默认配置中包含硬编码 API 信息，建议：

- 使用 `.appleby/config.example.toml` 作为示例；
- 将真实 `.appleby/config.toml` 加入 `.gitignore`；
- 支持从环境变量读取 `ANTHROPIC_API_KEY`、`ANTHROPIC_BASE_URL`、`ANTHROPIC_MODEL`。

### 11.2 工具安全

`bash` 工具目前只使用简单黑名单：

```rust
let dangerous = ["rm -rf /", "sudo", "shutdown", "reboot", "> /dev/"];
```

建议后续改为：

- 命令 allowlist；
- 敏感命令人工确认；
- 工具调用权限分级；
- sandbox / container 隔离执行。

### 11.3 UI 与 Agent 逻辑解耦

当前 `agent_loop` 和 `LoopState::execute` 中直接 `println!`。如果未来要支持 Web、TUI 或 API 服务，建议抽象输出层，例如：

```text
AgentCore 只返回事件：AssistantText、ToolUse、ToolResult、Error
CLI / Web / TUI 分别订阅并展示事件
```

### 11.4 测试建议

建议重点补充：

- `normalize_messages` 的各种消息合并测试；
- `agent_loop` 的 mock client 测试；
- 工具调用失败时的错误处理测试；
- 多个 tool_use 同时返回时的执行顺序测试；
- system prompt / config 加载测试。

## 12. 总结

当前 Appleby 的 `src` 已形成一个清晰的 Rust ReAct Agent 架构：

```text
CLI 输入
  → LoopState 保存上下文
  → agent_loop 调用模型推理
  → 模型产生 tool_use
  → 本地 Tool 执行动作
  → tool_result 回填为观察结果
  → 模型继续推理
  → 最终回答输出
```

该架构已经具备可扩展工具系统、上下文管理、系统提示词注入、日志记录和基础安全约束。后续重点应放在配置安全、工具权限、Agent 核心与 UI 解耦以及 workflow 测试覆盖上。
