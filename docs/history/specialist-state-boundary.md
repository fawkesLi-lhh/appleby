# Specialist 期望状态边界设计

> 本文只设计 Specialist 的**状态边界**：一次 Specialist run 可以看到什么、拥有什么、修改什么、返回什么，以及 run 结束后哪些状态可以存活。
>
> 本文不试图一次确定最终的 request/report schema，也不设计长期记忆、技能库和多 Specialist 调度。

## 1. 要解决的具体问题

Appleby 当前的 `LoopState` 同时持有：

- API adapter；
- model；
- system prompt；
- tools；
- conversation context；
- `.appleby/context.jsonl` 对应的长期会话日志。

这对单一主循环是可行的，但不能直接作为 Specialist 的状态模型。若 Synthesizer 和 Specialist 复用同一个 `LoopState`，将发生：

1. Specialist 默认看到主会话最近消息，无法验证显式 handoff 是否充分；
2. Specialist 的中间试错进入主 context，失去上下文隔离的意义；
3. Specialist 的 system prompt、预算和终止原因没有独立边界；
4. 父级无法区分“Specialist 返回的结论”和“Specialist 内部产生过的消息”；
5. 子 run 可能直接修改未来的长期记忆、任务状态或父会话；
6. 出错或超限后，很难判断哪些状态已经提交、哪些只是未完成的局部状态。

因此 Specialist 首先不是一个新 prompt，而是一种受约束的运行单元：

```text
immutable request
    + run-scoped mutable state
    + explicitly granted capabilities
    -> terminal result
    + durable audit record
```

本文的核心判断是：

> Specialist 是一个拥有独立消息历史和运行预算的临时执行单元。它可以通过显式授予的工具读取或修改工作区，但不能直接读写 Synthesizer 的认知状态。父子之间只通过 request 和 result 交换语义信息。

---

## 2. “状态边界”不是“所有东西都隔离”

需要区分四种边界，否则“独立上下文”容易被误解成“独立进程”或“完全无副作用”。

## 2.1 认知状态边界

指模型在一次 completion 中能看到的信息，包括：

- system prompt；
- Specialist request；
- 本次 run 内的 assistant/tool 消息；
- 显式提供的参考材料。

这部分必须与 Synthesizer 隔离。Specialist 不得默认继承父 conversation context、scratchpad、memory 检索结果或其他 Specialist 的 trace。

## 2.2 运行状态边界

指 harness 为一次 run 维护的机器状态，包括：

- run id；
- phase；
- completion turn 数；
- tool call 数；
- 已执行工具记录；
- 当前 termination；
- 持久化日志位置。

这部分由 runtime 拥有，模型只能间接影响，不能自己声称“我还剩 5 轮”或“本次已成功”就改变真实状态。

## 2.3 能力边界

指 Specialist 可以对外界做什么：

- 可使用哪些工具；
- 工具可访问哪些路径；
- 是否允许写文件或执行命令；
- 是否允许网络、子进程或创建其他 run。

能力边界不是 prompt 建议，而必须由实际注册给 Specialist 的工具集合强制保证。

## 2.4 外部世界边界

Specialist 可能通过 `Write`、`Edit`、`Bash` 改变工作区。上下文隔离并不自动意味着文件系统隔离。

第一版不必实现容器、临时工作树或事务文件系统，但必须承认：

> conversation state 是隔离的，workspace side effect 默认不是事务性的。

因此结果中必须记录外部副作用；run 失败也不代表已执行的文件修改自动回滚。

---

## 3. 状态所有权总表

以下表格定义第一版期望边界。

| 状态 | 所有者 | Specialist 是否可见 | Specialist 是否可修改 | run 结束后是否保留 |
|---|---|---:|---:|---:|
| 用户原始完整会话 | Synthesizer | 否 | 否 | 是 |
| 父 conversation context | Synthesizer | 否 | 否 | 是 |
| 父 scratchpad/task ledger | Synthesizer | 默认否 | 否 | 是 |
| 长期 memory store | 系统/Synthesizer | 默认否，只能看到显式摘录 | 否 | 是 |
| skill store | 系统/Synthesizer | 默认否，只能看到显式内容或能力 | 否 | 是 |
| `SpecialistRequest` | 父级创建，run 只读 | 是 | 否 | 是，进入审计记录 |
| Specialist system prompt | runtime | 是 | 否 | 配置保留，实例无需单独保留 |
| Specialist conversation | 当前 run | 是 | 通过模型/tool loop 追加 | 是，作为审计记录；不进入父 context |
| 预算计数器 | runtime | 可不直接暴露 | 否 | 是，进入 result/metrics |
| tool registry | runtime | 只见 tool specs | 否 | 否，配置在系统中保留 |
| tool execution trace | 当前 run/runtime | 通过 tool result 部分可见 | 否，由 runtime 追加 | 是 |
| workspace 文件 | 外部环境 | 通过工具可见 | 取决于授予的工具 | 是，除非以后增加事务层 |
| `SpecialistResult` | runtime 根据终态构造 | 最终输出由模型贡献 | 终态后不可变 | 是，并返回父级 |
| run record | runtime | 无需放进模型上下文 | 否 | 是 |
| 父级最终回答 | Synthesizer | 否 | 否 | 是 |

这个表产生三个重要约束：

1. **Specialist 不拥有父级状态的写权限。**
2. **Specialist 对工作区的修改权限与对父级认知状态的修改权限是两回事。**
3. **审计记录可以永久保存，但不等于默认重新注入任何模型上下文。**

---

## 4. Specialist run 的状态模型

建议把一次 run 分成三层，而不是继续把所有内容塞入一个 `LoopState`。

```text
SpecialistDefinition   可复用的角色/策略配置
SpecialistRequest      父级为这一次任务提供的不可变输入
SpecialistRunState     运行期间产生的可变状态
```

## 4.1 SpecialistDefinition：可复用配置

它描述“这种 run 怎样运行”，不描述“这次要做什么”。

```rust
pub struct SpecialistDefinition {
    pub system_prompt: String,
    pub model: String,
    pub max_completion_tokens: u32,
    pub default_limits: SpecialistLimits,
    pub tool_policy: SpecialistToolPolicy,
}
```

第一版可以只有一个默认 Specialist definition。定义这一层不是为了提前支持多个 persona，而是防止 system prompt、model、limits 混入 request，造成父级可以任意改写安全边界。

父级可以提出任务，不能通过 request 替换 Specialist system prompt 或提升工具权限。

## 4.2 SpecialistRequest：不可变任务快照

建议第一版使用：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistRequest {
    pub task: String,
    pub context: String,
    pub expected_outcome: Option<String>,
}
```

其状态语义如下。

### `task`

本次 run 唯一需要完成的问题或操作。它应当能在不读取父会话的情况下理解。

无效示例：

```text
继续查一下刚才那个问题。
```

有效示例：

```text
检查 src/workflow/loop_workflow.rs 中 agent_loop 的停止条件，判断工具连续调用时是否存在无限循环，并给出代码证据。
```

### `context`

父级主动选择的背景快照。它不是共享引用，也不应在 run 期间随父级变化。

`context` 可以包含：

- 已知事实；
- 用户约束；
- 与任务相关的前序结论；
- 必要术语定义；
- 父级希望 Specialist 核查的证据线索。

`context` 不应包含：

- 未筛选的完整父会话；
- 要求 Specialist 无条件接受的结论；
- 只有父级自己理解的代词或隐式引用；
- 与任务无关、只是“可能以后有用”的历史。

### `expected_outcome`

这是父级的预测或期望观察，不是 Specialist 必须证明的目标。

例如：

```text
预计 agent_loop 只有“本轮没有 tool_calls”这一种正常停止方式，因此连续工具调用会导致无界循环。
```

Specialist 必须能够返回“预期被推翻”。system prompt 中应明确：证据优先于 expected outcome。

第一版将它设为 `Option<String>`，原因是某些纯执行任务没有值得表达的假设；强制填写可能制造没有信息量的形式文本。

## 4.3 SpecialistRunState：run 内部可变状态

建议概念结构如下：

```rust
pub struct SpecialistRunState {
    pub run_id: RunId,
    pub phase: SpecialistPhase,
    pub request: SpecialistRequest,
    pub messages: Vec<ConversationMessage>,
    pub usage: SpecialistUsage,
    pub tool_trace: Vec<ExecutedToolCall>,
    pub effects: Vec<ObservedEffect>,
    pub termination: Option<SpecialistTermination>,
}
```

其中只有 runtime 可以直接修改 `phase`、`usage`、`tool_trace` 和 `termination`。

模型响应只是一条候选消息。runtime 在校验并追加后，才使它成为 run state 的一部分。

---

## 5. 生命周期与状态机

不要用一个布尔值 `completed` 表示所有情况。第一版建议状态机为：

```text
Created
   |
   | validate request + create durable run directory
   v
Running
   |  \
   |   \ unrecoverable runtime error
   |    v
   |   Failed
   |
   | model returns final text without tool calls
   v
Completed

Running -- limit reached --> Exhausted
Running -- external cancellation --> Cancelled
```

对应类型：

```rust
pub enum SpecialistPhase {
    Created,
    Running,
    Terminal,
}

pub enum SpecialistTermination {
    Completed,
    MaxTurns,
    MaxToolCalls,
    Cancelled,
    RuntimeError,
}
```

第一版真正实现时，可以先只支持：

- `Completed`；
- `MaxTurns`；
- `RuntimeError`。

但持久化格式最好从一开始允许扩展终止原因，避免把 `success: bool` 固化为错误抽象。

## 5.1 Created

此阶段应完成：

1. 校验 `task` 非空；
2. 校验输入大小不超过 harness 限制；
3. 生成 run id；
4. 冻结 request；
5. 解析实际 tool set；
6. 创建 run record；
7. 组装初始消息。

若初始化失败，不能进入 `Running`。

## 5.2 Running

每一轮严格执行：

```text
检查预算
-> 组装 ApiRequest
-> 调用 adapter
-> 追加 assistant message
-> 若无 tool call，形成 Completed
-> 若有 tool call，逐个校验并执行
-> 追加 tool result 与 trace
-> 回到预算检查
```

预算检查必须发生在 completion 之前。假设 `max_turns = 8`，最多只能调用 adapter 8 次。

## 5.3 Terminal

任何 terminal 状态都必须满足：

- 不再允许追加模型消息；
- 不再允许执行新工具；
- `termination` 已确定；
- result 被构造并持久化；
- 父级拿到的是 result，不是 run state 的可变引用。

terminal 之后若要继续，应创建新的 run，并显式把旧 result 或所需证据放入新 request。第一版不提供“恢复同一个 run 继续跑”，以保持边界简单。

---

## 6. 初始上下文的精确定义

Specialist 的第一次 API 请求只能由以下部分构成：

```text
system:
    SpecialistDefinition.system_prompt

user:
    serialized SpecialistRequest
```

不包含：

- `.appleby/context.jsonl` 最近消息；
- Synthesizer system prompt；
- Synthesizer assistant/tool 消息；
- 其他 run 的 context；
- 自动检索的 memory；
- 未在 request 中列出的 scratchpad 内容。

建议把 request 编码为有明显字段边界的文本，而不是随意拼接：

```text
# Task
{task}

# Supplied context
{context or "No additional context supplied."}

# Expected outcome
{expected_outcome or "No prior expectation. Determine from evidence."}
```

第一版不必要求 adapter 支持额外消息角色或复杂 JSON response format。保持单个 user message，便于从 fake adapter 中检查输入边界。

## 6.1 为什么 request 本身不直接作为 system prompt

父级生成的内容本质上不可信，特别是未来 request 可能包含用户文本或文件摘录。把它放进 system prompt 会模糊权限层级。

因此：

- system prompt 定义 Specialist 的稳定职责和边界；
- user message 携带具体任务和父级交接内容。

---

## 7. 工具与副作用边界

这是 Specialist 状态设计中最容易遗漏的部分。

## 7.1 工具集合必须按 run 构造

当前 `toolmap()` 返回 `Bash/Read/Write/Edit`。第一版 Specialist 可以复用这些工具，但不应直接无条件克隆父级未来可能拥有的所有工具。

至少需要排除：

- `RunSpecialist`，避免递归；
- 直接写 memory 的工具；
- 直接写 Synthesizer scratchpad/task ledger 的工具；
- 改变父会话 context 的工具；
- 提升权限或修改 SpecialistDefinition 的工具。

建议先用白名单：

```rust
pub struct SpecialistToolPolicy {
    pub allowed_tools: Vec<String>,
}
```

实际注册给 API 的 specs 和可执行 registry 都必须由同一个白名单过滤，避免“模型看不到但仍能通过伪造名字调用”或“看得到但 runtime 不允许”的不一致。

## 7.2 读工具与写工具不是同一级风险

可定义能力等级，但第一版不一定要立即实现所有等级：

```text
Inspect     Read
Execute     Read + Bash
Modify      Read + Bash + Write + Edit
```

这比在 prompt 中写“需要时谨慎修改文件”更可验证。

第一版如果所有 Specialist 都保留修改权限，request 中就必须清楚说明是否要求执行修改；否则模型可能把“分析修复方式”理解成“直接修复”。更稳妥的初始实验是：

- 分析类 Specialist：只授予 `Read` 和受约束的 `Bash`；
- 执行类能力在后续根据真实任务加入。

不过当前 `Bash` 本身可能写文件，所以“只读”不能仅靠不注册 `Write/Edit` 实现。如果要声称 Inspect 真正只读，需要额外约束 Bash；在此之前文档和结果中只能称为“无显式文件编辑工具”，不能称为强只读沙箱。

## 7.3 工具执行 trace

每次执行至少记录：

```rust
pub struct ExecutedToolCall {
    pub sequence: usize,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub outcome: ToolCallOutcome,
}

pub enum ToolCallOutcome {
    Succeeded { output: String },
    Failed { error: String },
    Rejected { reason: String },
}
```

注意三种失败要区分：

- 工具执行并返回成功输出；
- 工具执行失败；
- runtime 因能力边界拒绝调用。

被拒绝的 tool call 仍应以匹配 call id 的 tool result 回填给模型，使消息序列合法，并允许模型调整方案。

## 7.4 副作用记录的限制

仅靠 tool trace 不能精确知道 `Bash` 修改了哪些文件。第一版可以先记录“已执行可能产生副作用的工具调用”，不要假装已经有完整 effect tracking。

后续若实验表明需要可靠产物边界，可增加：

- run 前后 `git diff`；
- workspace snapshot；
- 临时 worktree；
- 工具级 effect report；
- 显式提交/回滚阶段。

在这些机制实现前，`effects` 字段如果存在，应叫 `reported_effects` 或 `observed_effects`，不能叫 `all_effects`。

---

## 8. 输出边界：返回结论，不暴露整个可变状态

建议父级收到：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistResult {
    pub run_id: RunId,
    pub termination: SpecialistTermination,
    pub answer: Option<String>,
    pub usage: SpecialistUsage,
    pub tool_summary: ToolExecutionSummary,
}
```

其中：

```rust
pub struct SpecialistUsage {
    pub completion_turns: usize,
    pub tool_calls: usize,
}

pub struct ToolExecutionSummary {
    pub succeeded: usize,
    pub failed: usize,
    pub rejected: usize,
}
```

## 8.1 `answer` 为什么是 `Option<String>`

只有正常得到最终 assistant text 时，才能可靠地产生 answer。

- `Completed`：通常应有 `Some(answer)`；
- `MaxTurns`：可能只有 `None`，不能把最后一条 tool call 消息伪装成答案；
- `RuntimeError`：可能没有模型结论；
- 未来 `Cancelled`：也可能没有结论。

这比 `answer: String` 配空字符串更清楚。

## 8.2 父级默认不接收完整 trace

`SpecialistResult` 是正常控制流的返回值，完整 `context.jsonl` 和 tool trace 是诊断记录。默认情况下不把完整 trace序列化进父 conversation context，否则隔离收益会消失。

父级需要进一步核查时，有两种方式：

1. 创建新的 Specialist request，并提供旧 run 的必要摘录；
2. 未来增加按 run id 读取证据的工具。

第一版优先采用第一种，避免立刻设计 run artifact 查询协议。

## 8.3 Result 不是事实自动提交

Specialist 返回的 answer 即使 termination 为 `Completed`，也只意味着：

> 模型在预算内结束并给出了最终文本。

它不意味着：

- 答案事实正确；
- 父级假设已被证明；
- 代码修改正确；
- 内容可以直接写入长期记忆；
- 用户任务已整体完成。

因此不要把 termination 命名为 `Succeeded/Failed`。`Completed` 描述运行方式，不替父级做语义判断。

---

## 9. 错误边界

需要区分三类错误，它们对状态的影响不同。

## 9.1 输入错误

例如：

- task 为空；
- request 超过 harness 限制；
- requested capability 不存在。

结果：run 不进入 `Running`。调用者直接得到 Rust error，或者未来使用 `Rejected` 类型。第一版无需创建完整 conversation log，但如果已经分配 run id，应留下初始化失败记录。

## 9.2 可恢复的局部工具错误

例如：

- Read 路径不存在；
- Bash 命令退出非零；
- 模型调用未知工具；
- 工具参数错误。

结果：将错误作为 tool result 回填，run 继续；trace 记为 `Failed` 或 `Rejected`。

这不应直接把整个 Specialist 标记为 `RuntimeError`。

## 9.3 不可恢复的 runtime 错误

例如：

- API 请求失败且当前策略不重试；
- context log 无法持久化；
- adapter 返回无法转换的非法消息；
- runtime 自身 invariant 被破坏。

结果：停止新动作，termination 为 `RuntimeError`，尽力持久化现有 trace 和错误摘要。

API 错误究竟重试几次属于后续策略。第一版可以不重试，但状态模型不能把 API error 与 MaxTurns 混为一类。

---

## 10. 持久化边界

建议每个 run 形成独立目录：

```text
.appleby/runs/<run-id>/
    request.json
    context.jsonl
    result.json
```

如果实现成本允许，再加入：

```text
    metadata.json
    tool-trace.jsonl
```

第一版文件职责：

### `request.json`

不可变。记录实际传入的 `SpecialistRequest`。创建后不得被运行过程覆盖。

### `context.jsonl`

按发生顺序记录本次 Specialist 的 user/assistant/tool messages。它不是父 context 的分片，而是一份独立日志。

### `result.json`

terminal 后一次性写入的结果快照。若使用直接覆盖，应采用临时文件加 rename，避免进程中断留下半个 JSON。

### `metadata.json`（可选）

记录 definition 版本、model、实际 tool names、创建时间等非会话配置。若第一版不创建该文件，至少应在 `result.json` 中保存 model 和实际 tools，以便实验可复现。

## 10.1 审计状态不等于恢复状态

保存完整 context 的第一目的，是复盘和测试，不代表第一版支持中断后续跑。

要支持 resume，还需要定义：

- 工具调用是否已经发生但结果未写入；
- 副作用是否可重复执行；
- 未完成 tool call 如何补 cancelled result；
- budget 如何恢复；
- definition/tool version 变化如何处理。

这些问题没有解决前，不要把“可以读取日志”称为“可恢复 run”。

---

## 11. 必须由 runtime 强制的 invariants

以下规则不能只写入 system prompt。

### I1. 父上下文不可见

Specialist API request 中不得出现父 conversation messages，除非内容被显式复制进 `SpecialistRequest.context`。

### I2. Request 不可变

进入 Running 后，task/context/expected outcome 不能被覆盖。模型可以在 answer 中指出 task 有问题，但不能悄悄改写原始输入。

### I3. 消息只追加

run conversation 使用 append-only 语义。normalization 可以在发送 API 前生成视图，但不能无痕改写持久化原始记录。

### I4. Tool call 与 result 匹配

每个已接受的 tool call 最终必须有且只有一个匹配 `tool_call_id` 的 tool result；未知或重复 call id 必须有确定处理策略。

### I5. 预算由 harness 判定

模型不能绕过 max turns。达到预算后不得再调用 adapter 或工具。

### I6. Terminal 不可逆

进入 terminal 后不得追加消息或执行工具。

### I7. Specialist 不能直接写父认知状态

不注册写 memory、写 task ledger、修改父 context、创建 Specialist 的能力。

### I8. Result 来源明确

`answer` 只能来自正常最终 assistant text，不能从任意中间消息、最后一次 tool output 或错误字符串隐式推断。

### I9. Tool 能力一致

发送给模型的 tool specs 必须与 runtime 实际允许执行的 tools 一致。

### I10. 日志失败不可静默

若设计要求 message 先落盘再成为有效状态，则 append 失败必须停止 run，不能继续执行并让内存状态与审计状态分叉。

---

## 12. Specialist system prompt 的边界职责

system prompt 只负责语义行为，不承担 runtime 已经能强制的事情。第一版可以表达：

```text
You are Appleby's Specialist. You receive one bounded task with explicitly
supplied context. Investigate or execute that task using the available tools.

Treat the expected outcome as a prior expectation, not as a conclusion. Report
contradicting evidence when found. Do not assume access to the parent
conversation or unstated facts.

Use tools when the answer depends on workspace facts. When you have enough
evidence, return a concise final answer that states the conclusion and the
checkable evidence. If the task cannot be completed, state what blocks it and
what was actually established.
```

不需要在 prompt 中反复声称：

- 不要超过 max turns；
- 不要调用未注册工具；
- 不要写 memory；
- 不要污染父 context。

这些必须由状态和能力边界保证。prompt 可以解释行为预期，但不能作为唯一防线。

---

## 13. 与当前代码的映射

当前实现中：

- `LoopState` 同时包含共享配置和单次会话状态；
- `agent_loop` 没有 budget、termination 和 trace 返回值；
- `LoopState::new` 会根据参数加载或归档固定的 `.appleby/context.jsonl`；
- `execute_tool_calls` 返回 tool messages，但不保留结构化执行 trace；
- `agent_loop` 只以“assistant 没有 tool call”作为停止条件。

因此不建议直接给 `LoopState` 加一个 `is_specialist: bool`。这会让每个行为逐渐变成模式分支：

```rust
if is_specialist { ... } else { ... }
```

更合适的最小演进是先把“运行状态”和“交互式会话状态”分开。

一种可实施的模块边界：

```text
src/
  agent/
    run.rs              通用模型/工具单次运行循环
    state.rs            RunState、limits、termination、trace
  specialist/
    mod.rs
    request.rs
    result.rs
    prompt.rs
    runner.rs           组装独立 run 并调用 agent::run
  workflow/
    loop_workflow.rs    现有交互主循环
```

如果暂时不想增加 `agent/` 层，也可以先：

```text
src/specialist.rs
```

并复制少量 `agent_loop` 逻辑做实验，但这只能作为短期 spike。因为两个循环一旦分别修复 tool-call normalization、错误处理和预算，很快会分叉。

## 13.1 建议的通用运行接口

可以先抽出：

```rust
pub struct AgentRunConfig {
    pub model: String,
    pub system_prompt: String,
    pub tools: HashMap<String, Box<dyn Tool>>,
    pub limits: RunLimits,
}

pub struct AgentRunState {
    pub messages: Vec<ConversationMessage>,
    pub turns: usize,
    pub tool_trace: Vec<ExecutedToolCall>,
}

pub async fn run_agent(
    adapter: &dyn ApiAdapter,
    config: AgentRunConfig,
    state: &mut AgentRunState,
) -> anyhow::Result<AgentRunOutcome>;
```

但当前 `Tool` 的生命周期、`HashMap<String, Box<dyn Tool>>` 的所有权以及未来并发需求还需要通过实现确认。第一版不要为共享工具提前加入复杂 `Arc<Mutex<...>>`。Specialist 串行运行时，让每个 run 拥有自己的 tool map 最容易保持状态边界。

当前 `Tool::invoke(&self, ...)` 本身不要求可变借用，因此未来也可以考虑让 registry 共享不可变 tool 对象；但在明确工具是否会持有内部状态前，不应先假定所有工具都安全共享。

---

## 14. 第一版应测试的边界，而不是只测试答案

## T1. 父消息不会泄漏

创建带特征字符串的父 context，request 不包含它。fake adapter 捕获每次 `ApiRequest`。

断言所有 Specialist API request 均不含该字符串。

## T2. 父 context 不被子消息污染

运行前后比较父 message list。Specialist 内部 assistant/tool messages 不得出现于父列表。

未来通过 `RunSpecialist` 调用时，父级只增加该工具调用和序列化 result。

## T3. Request 是快照

创建 request 后修改构造 request 所使用的源字符串或父 scratchpad，Specialist 仍只看到创建时的值。

Rust 的 owned `String` 天然有利于保证这一点，不要使用指向父状态的可变引用。

## T4. Tool 白名单有效

fake model 调用未授权工具。

断言：

- 工具未执行；
- trace 标记 `Rejected`；
- 模型收到对应错误结果；
- run 可在后续轮次继续。

## T5. MaxTurns 是硬边界

fake model 每轮都请求工具。

断言 adapter 调用次数等于 max turns，且终止后工具和消息数量不再增长。

## T6. Terminal 后不可修改

对 terminal state 尝试 append message 或 execute tool，应返回明确错误，不能静默接受。

如果结构上不向外暴露这些方法，也可以通过类型设计使该测试不必要；编译期不可表达非法状态优于运行时报错。

## T7. 工具错误不是 run error

fake tool 返回错误，fake model 下一轮给出最终结论。

断言 run 为 `Completed`，trace 含失败工具，answer 保留模型对失败的处理。

## T8. API 错误形成 RuntimeError

adapter 返回 error。

断言不再调用工具，已有状态被保留，result/错误记录可读取。

这里需要决定 Rust API：是 `Ok(SpecialistResult { termination: RuntimeError })`，还是返回 `Err` 并单独持久化失败状态。建议：

- 预期内的运行终止（MaxTurns）返回 `Ok(result)`；
- 基础设施失败返回 `Err`；
- run record 中仍写 `RuntimeError`。

这样调用者不会把基础设施错误当成一个普通业务结果，同时审计记录仍有统一终态。

## T9. Result 不携带完整 trace

序列化返回给父级的 result，确认其大小和字段不包含整个 `messages`。完整 trace 只能通过 run record 获得。

## T10. 工作区副作用不会被伪装成事务

让 Specialist 写入测试文件后在后续 API 调用处失败。

断言文件仍存在，同时失败记录表明 run 未正常完成。这个测试是为了固定真实语义：第一版不自动回滚。

---

## 15. 第一版明确不保证的边界

为避免实现后产生错误预期，应明确第一版不保证：

1. **文件系统隔离**：Specialist 可能修改同一个 workspace；
2. **副作用回滚**：失败、超限或取消不会自动撤销工具动作；
3. **并发安全**：第一版只允许一个 Specialist run 同时操作 workspace；
4. **崩溃恢复**：日志可审计，但不承诺原 run 可安全续跑；
5. **答案正确性**：Completed 只表示模型正常结束；
6. **完整 effect detection**：Bash 的所有副作用未必可被识别；
7. **长期知识写入**：Specialist 不能直接提交 memory 或 skill；
8. **递归委派**：Specialist 不能创建 Specialist；
9. **父级公平调度**：何时创建 run 属于后续 Synthesizer 的问题；
10. **token 精确预算**：当前 adapter 未暴露统一 usage 时，先以 completion turns 限制。

这些不是设计漏洞，而是第一轮实验刻意保留的范围边界。后续只有真实失败要求时才扩展。

---

## 16. 推荐的第一版类型草案

以下草案刻意保持小，不代表最终 API：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistRequest {
    pub task: String,
    pub context: String,
    pub expected_outcome: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SpecialistLimits {
    pub max_turns: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpecialistTermination {
    Completed,
    MaxTurns,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistUsage {
    pub completion_turns: usize,
    pub tool_calls: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionSummary {
    pub succeeded: usize,
    pub failed: usize,
    pub rejected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistResult {
    pub run_id: String,
    pub termination: SpecialistTermination,
    pub answer: Option<String>,
    pub usage: SpecialistUsage,
    pub tool_summary: ToolExecutionSummary,
}
```

基础设施错误通过 `anyhow::Result<SpecialistResult>` 的 `Err` 表达；同时在 run record 中写入 RuntimeError。这样第一版不需要在返回类型里重复塞入任意 error string。

调用接口：

```rust
pub async fn run_specialist(
    runtime: &SpecialistRuntime,
    request: SpecialistRequest,
    limits: SpecialistLimits,
) -> anyhow::Result<SpecialistResult>;
```

`SpecialistRuntime` 持有或能创建：

- adapter；
- model 配置；
- Specialist system prompt；
- 本次允许的 tool map；
- run store。

它不持有父 conversation context。

---

## 17. 需要通过实现回答的开放问题

以下问题现在不应仅靠讨论定案，应在第一版运行后根据 trace 回答。

## Q1. Result 是否需要结构化 evidence

第一版先返回 `answer`。若经常出现“证据在子 trace 中，但 answer 没带回来”，再加入：

```rust
pub struct Evidence {
    pub claim: String,
    pub source: String,
    pub observation: String,
}
```

## Q2. expected outcome 是否有净收益

用正确预期、错误预期和无预期三组任务比较。如果错误预期显著诱导确认偏误，应改名为 `prior_hypothesis`、增加反证要求，或从部分任务中移除。

## Q3. 是否需要在 request 中限制工具

若同一种 Specialist run 同时承担只读分析和代码修改，父级可能需要请求 capability。但 capability 的最终批准仍属于 runtime，不能让 request 自行授予权限。

可能的后续形式：

```rust
pub enum RequestedCapability {
    Inspect,
    Execute,
    Modify,
}
```

runtime 取“请求能力”和“系统允许能力”的交集。

## Q4. 工具 trace 是否需要输出全文

工具输出可能很大。审计日志可以保留完整或截断后的原始输出，父级 result 只保留计数。具体截断策略应根据真实 Read/Bash 输出大小决定。

## Q5. 是否需要可取消 run

接入交互式 Synthesizer 后，长 Bash 或网络调用可能要求取消。取消需要贯穿 adapter、tool invocation 和持久化状态，不能只加一个 enum 名字。

## Q6. 是否需要 workspace 事务

如果 Specialist 主要做代码修改，那么“失败后留下半成品”会成为重要状态问题。届时优先考虑临时 git worktree 或显式 diff/commit，而不是让模型凭文字描述自己改了什么。

---

## 18. 完成定义

Specialist 的第一版状态边界只有在下面条件全部成立时才算完成：

- 每次 run 有独立 owned request、messages、usage、trace 和 run id；
- Specialist 不读取默认父 context log；
- 父 conversation 不接收 Specialist 内部消息；
- 工具集合由 runtime 白名单决定，且不能递归创建 Specialist；
- max turns 是硬限制；
- 正常完成、预算耗尽和基础设施错误可以区分；
- terminal 后不能继续产生动作；
- result 不包含完整子 trace；
- request、context 和 terminal record 可复盘；
- 文档和测试明确承认 workspace side effect 不自动回滚；
- fake adapter/tool 可以确定性验证上述边界。

完成这些条件后，我们得到的不是一个“更聪明的 agent”，而是一个语义清楚的执行原语：

```text
给定不可变任务快照和有限能力，
在独立认知上下文和硬预算内执行，
留下完整审计记录，
只向父级返回有限结果，
不直接提交父级认知状态。
```

这就是 Synthesizer 可以安全调用、可以实验、也可以在后续逐步加强的 Specialist 状态边界。

---

## 19. 紧接着应该实现什么

建议下一次代码变更只完成以下垂直切片：

1. 定义 `SpecialistRequest`、`SpecialistLimits`、`SpecialistResult`；
2. 为 agent loop 增加 completion turn 计数和 `MaxTurns`；
3. 新建不加载 `.appleby/context.jsonl` 的 Specialist messages；
4. 用独立 tool map 运行；
5. 返回 answer、termination、usage；
6. 使用 fake adapter 完成 T1、T2、T5、T7、T8；
7. 暂不实现 `RunSpecialist` tool，先提供测试或开发入口直接调用。

这一切跑通后，再补 run directory 持久化和 tool trace；随后才把它作为工具交给 Synthesizer。
