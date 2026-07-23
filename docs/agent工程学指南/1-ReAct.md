ReAct 主流程
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