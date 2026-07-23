use std::collections::HashSet;

use anyhow::Result;

use crate::{
    api_adapter::{ApiRequest, ConversationMessage, ToolCallRecord},
    state::loop_state::LoopState,
    workflow::tui_channel::{AgentChannel, AgentEvent, TuiCommand},
};

pub async fn agent_loop(mut state: LoopState, mut channel: AgentChannel) -> Result<()> {
    let mut next_turn_id = 1;

    while let Some(command) = channel.recv().await {
        match command {
            TuiCommand::SubmitUserMessage { content } => {
                let turn_id = next_turn_id;
                next_turn_id += 1;
                let user_message = ConversationMessage::user(content.clone());

                if let Err(error) = state.push_message(user_message) {
                    channel
                        .send(AgentEvent::TurnFailed {
                            turn_id,
                            message: error.to_string(),
                        })
                        .await?;
                    continue;
                }

                channel
                    .send(AgentEvent::TurnStarted {
                        turn_id,
                        user_message: content,
                    })
                    .await?;

                match run_turn(&mut state, turn_id, &channel).await {
                    Ok(()) => channel.send(AgentEvent::TurnCompleted { turn_id }).await?,
                    Err(error) => {
                        channel
                            .send(AgentEvent::TurnFailed {
                                turn_id,
                                message: error.to_string(),
                            })
                            .await?;
                    }
                }
            }
            TuiCommand::Shutdown => {
                channel.send(AgentEvent::RunnerStopped).await?;
                return Ok(());
            }
        }
    }

    Ok(())
}

async fn run_turn(state: &mut LoopState, turn_id: u64, channel: &AgentChannel) -> Result<()> {
    loop {
        let messages = normalize_messages(state.get_context());
        channel
            .send(AgentEvent::ModelRequestStarted { turn_id })
            .await?;
        let response = state
            .api_adapter
            .complete(ApiRequest {
                model: state.model.clone(),
                system_prompt: state.system_prompt.clone(),
                messages,
                tools: state.tools.values().map(|tool| tool.tool_spec()).collect(),
                max_tokens: 8000,
            })
            .await?;
        let message = response.assistant_message;
        let tool_calls = match &message {
            ConversationMessage::Assistant { tool_calls, .. } => tool_calls.clone(),
            _ => Vec::new(),
        };
        state.push_message(message.clone())?;
        channel
            .send(AgentEvent::AssistantMessageCompleted { turn_id, message })
            .await?;

        if tool_calls.is_empty() {
            return Ok(());
        }

        for tool_call in tool_calls {
            let presentation = state.describe_tool_call(&tool_call);
            channel
                .send(AgentEvent::ToolCallStarted {
                    turn_id,
                    call: tool_call.clone(),
                    presentation,
                })
                .await?;

            let result = state.execute_tool_call(&tool_call).await;
            state.push_message(result.clone())?;
            channel
                .send(AgentEvent::ToolCallCompleted { turn_id, result })
                .await?;
        }
    }
}

pub fn normalize_messages(messages: &[ConversationMessage]) -> Vec<ConversationMessage> {
    let mut normalized = Vec::new();
    let mut pending_tool_calls: Vec<ToolCallRecord> = Vec::new();

    for message in messages {
        match message {
            ConversationMessage::Tool {
                tool_call_id,
                content,
            } => {
                if let Some(index) = pending_tool_calls
                    .iter()
                    .position(|call| call.id == *tool_call_id)
                {
                    pending_tool_calls.remove(index);
                    normalized.push(ConversationMessage::tool(
                        tool_call_id.clone(),
                        content.clone(),
                    ));
                }
            }
            ConversationMessage::User { content } => {
                flush_cancelled_tool_calls(&mut normalized, &mut pending_tool_calls);
                push_mergeable_message(&mut normalized, ConversationMessage::user(content.clone()));
            }
            ConversationMessage::Assistant {
                content,
                tool_calls,
            } => {
                flush_cancelled_tool_calls(&mut normalized, &mut pending_tool_calls);
                let message = ConversationMessage::assistant(content.clone(), tool_calls.clone());
                if tool_calls.is_empty() {
                    push_mergeable_message(&mut normalized, message);
                } else {
                    let mut seen = HashSet::new();
                    pending_tool_calls = tool_calls
                        .iter()
                        .filter(|call| seen.insert(call.id.clone()))
                        .cloned()
                        .collect();
                    normalized.push(message);
                }
            }
        }
    }

    flush_cancelled_tool_calls(&mut normalized, &mut pending_tool_calls);
    normalized
}

fn flush_cancelled_tool_calls(
    messages: &mut Vec<ConversationMessage>,
    pending_tool_calls: &mut Vec<ToolCallRecord>,
) {
    for tool_call in pending_tool_calls.drain(..) {
        messages.push(ConversationMessage::tool(tool_call.id, "(cancelled)"));
    }
}

fn push_mergeable_message(messages: &mut Vec<ConversationMessage>, message: ConversationMessage) {
    if let Some(last) = messages.last_mut() {
        match (last, &message) {
            (
                ConversationMessage::User { content: prev },
                ConversationMessage::User { content: curr },
            ) => {
                prev.push('\n');
                prev.push_str(curr);
                return;
            }
            (
                ConversationMessage::Assistant {
                    content: prev,
                    tool_calls: prev_tool_calls,
                },
                ConversationMessage::Assistant {
                    content: curr,
                    tool_calls: curr_tool_calls,
                },
            ) if prev_tool_calls.is_empty() && curr_tool_calls.is_empty() => {
                if let Some(curr) = curr {
                    if let Some(prev) = prev {
                        prev.push('\n');
                        prev.push_str(curr);
                    } else {
                        *prev = Some(curr.clone());
                    }
                }
                return;
            }
            _ => {}
        }
    }

    messages.push(message);
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        collections::{HashMap, VecDeque},
        path::Path,
        sync::Mutex,
    };

    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::json;

    use super::{agent_loop, normalize_messages};
    use crate::{
        api_adapter::{ApiAdapter, ApiRequest, ApiResponse, ConversationMessage, ToolCallRecord},
        state::{
            conversation_context::{ContextLoadMode, ConversationContext},
            loop_state::LoopState,
        },
        tool::{Tool, ToolSpec},
        workflow::tui_channel::{AgentEvent, TuiChannel, TuiCommand, tui_channel},
    };

    struct ScriptedAdapter {
        responses: Mutex<VecDeque<ApiResponse>>,
    }

    #[async_trait]
    impl ApiAdapter for ScriptedAdapter {
        async fn complete(&self, _request: ApiRequest) -> Result<ApiResponse> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("scripted adapter ran out of responses"))
        }
    }

    struct TestTool;

    #[async_trait]
    impl Tool for TestTool {
        async fn invoke(&self, _input: &serde_json::Value) -> Result<String> {
            Ok("tool output".to_string())
        }

        fn name(&self) -> Cow<'_, str> {
            Cow::Borrowed("Test")
        }

        fn tool_spec(&self) -> ToolSpec {
            ToolSpec {
                name: "Test".to_string(),
                description: None,
                input_schema: json!({"type": "object"}),
            }
        }

        fn show_to_human(
            &self,
            writer: &mut dyn std::fmt::Write,
            input: &serde_json::Value,
        ) -> Result<()> {
            write!(writer, " test input: {input}")?;
            Ok(())
        }
    }

    fn build_state(
        path: &Path,
        responses: Vec<ApiResponse>,
        tools: HashMap<String, Box<dyn Tool>>,
    ) -> LoopState {
        let context = ConversationContext::open_jsonl(path, ContextLoadMode::FreshArchive).unwrap();
        LoopState::new(
            Box::new(ScriptedAdapter {
                responses: Mutex::new(responses.into()),
            }),
            tools,
            "test-model".to_string(),
            "test system prompt".to_string(),
            context,
        )
    }

    async fn collect_turn(channel: &mut TuiChannel) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        while let Some(event) = channel.recv().await {
            let completed = matches!(
                event,
                AgentEvent::TurnCompleted { .. } | AgentEvent::TurnFailed { .. }
            );
            events.push(event);
            if completed {
                return events;
            }
        }
        panic!("agent loop stopped before completing the turn");
    }

    async fn shutdown(channel: &mut TuiChannel) {
        channel.send(TuiCommand::Shutdown).await.unwrap();
        assert!(matches!(
            channel.recv().await,
            Some(AgentEvent::RunnerStopped)
        ));
    }

    #[tokio::test]
    async fn agent_loop_persists_and_emits_a_text_only_turn() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");
        let state = build_state(
            &path,
            vec![ApiResponse {
                assistant_message: ConversationMessage::assistant(
                    Some("hello".to_string()),
                    Vec::new(),
                ),
            }],
            HashMap::new(),
        );
        let (agent_channel, mut tui_channel) = tui_channel();
        let agent_task = tokio::spawn(agent_loop(state, agent_channel));

        tui_channel
            .send(TuiCommand::SubmitUserMessage {
                content: "hi".to_string(),
            })
            .await
            .unwrap();
        let events = collect_turn(&mut tui_channel).await;

        assert!(matches!(
            events.as_slice(),
            [
                AgentEvent::TurnStarted { turn_id: 1, .. },
                AgentEvent::ModelRequestStarted { turn_id: 1 },
                AgentEvent::AssistantMessageCompleted { turn_id: 1, .. },
                AgentEvent::TurnCompleted { turn_id: 1 },
            ]
        ));

        shutdown(&mut tui_channel).await;
        agent_task.await.unwrap().unwrap();

        let context =
            ConversationContext::open_jsonl(&path, ContextLoadMode::LoadRecent { limit: 10 })
                .unwrap();
        assert_eq!(
            context.messages(),
            [
                ConversationMessage::user("hi"),
                ConversationMessage::assistant(Some("hello".to_string()), Vec::new()),
            ]
        );
    }

    #[tokio::test]
    async fn agent_loop_emits_tool_events_and_persists_tool_results_before_follow_up() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");
        let call = ToolCallRecord {
            id: "call-1".to_string(),
            name: "Test".to_string(),
            arguments: json!({"value": "input"}),
        };
        let state = build_state(
            &path,
            vec![
                ApiResponse {
                    assistant_message: ConversationMessage::assistant(
                        Some("running a tool".to_string()),
                        vec![call.clone()],
                    ),
                },
                ApiResponse {
                    assistant_message: ConversationMessage::assistant(
                        Some("finished".to_string()),
                        Vec::new(),
                    ),
                },
            ],
            HashMap::from([("Test".to_string(), Box::new(TestTool) as Box<dyn Tool>)]),
        );
        let (agent_channel, mut tui_channel) = tui_channel();
        let agent_task = tokio::spawn(agent_loop(state, agent_channel));

        tui_channel
            .send(TuiCommand::SubmitUserMessage {
                content: "run test".to_string(),
            })
            .await
            .unwrap();
        let events = collect_turn(&mut tui_channel).await;

        assert!(matches!(
            events.as_slice(),
            [
                AgentEvent::TurnStarted { turn_id: 1, .. },
                AgentEvent::ModelRequestStarted { turn_id: 1 },
                AgentEvent::AssistantMessageCompleted { turn_id: 1, .. },
                AgentEvent::ToolCallStarted {
                    turn_id: 1,
                    call: started_call,
                    presentation,
                },
                AgentEvent::ToolCallCompleted {
                    turn_id: 1,
                    result: ConversationMessage::Tool { .. },
                },
                AgentEvent::ModelRequestStarted { turn_id: 1 },
                AgentEvent::AssistantMessageCompleted { turn_id: 1, .. },
                AgentEvent::TurnCompleted { turn_id: 1 },
            ] if started_call == &call && presentation == " test input: {\"value\":\"input\"}"
        ));

        shutdown(&mut tui_channel).await;
        agent_task.await.unwrap().unwrap();

        let context =
            ConversationContext::open_jsonl(&path, ContextLoadMode::LoadRecent { limit: 10 })
                .unwrap();
        assert_eq!(
            context.messages(),
            [
                ConversationMessage::user("run test"),
                ConversationMessage::assistant(Some("running a tool".to_string()), vec![call]),
                ConversationMessage::tool("call-1", "tool output"),
                ConversationMessage::assistant(Some("finished".to_string()), Vec::new()),
            ]
        );
    }

    fn read_call(id: &str) -> ToolCallRecord {
        ToolCallRecord {
            id: id.to_string(),
            name: "Read".to_string(),
            arguments: json!({ "path": "Cargo.toml" }),
        }
    }

    #[test]
    fn normalize_messages_drops_tool_results_without_loaded_tool_call() {
        let messages = vec![ConversationMessage::tool(
            "missing-tool-call",
            "stale output",
        )];

        assert!(normalize_messages(&messages).is_empty());
    }

    #[test]
    fn normalize_messages_adds_cancelled_result_for_unanswered_tool_call() {
        let messages = vec![ConversationMessage::assistant(
            None,
            vec![read_call("tool-1")],
        )];

        let normalized = normalize_messages(&messages);

        assert_eq!(normalized.len(), 2);
        assert_eq!(
            normalized[1],
            ConversationMessage::tool("tool-1", "(cancelled)")
        );
    }

    #[test]
    fn normalize_messages_adds_missing_result_before_next_user_message() {
        let messages = vec![
            ConversationMessage::assistant(None, vec![read_call("tool-1"), read_call("tool-2")]),
            ConversationMessage::tool("tool-1", "ok"),
            ConversationMessage::user("next"),
        ];

        let normalized = normalize_messages(&messages);

        assert_eq!(normalized.len(), 4);
        assert_eq!(
            normalized[2],
            ConversationMessage::tool("tool-2", "(cancelled)")
        );
        assert_eq!(normalized[3], ConversationMessage::user("next"));
    }

    #[test]
    fn normalize_messages_merges_plain_text_turns_only() {
        let messages = vec![
            ConversationMessage::user("one"),
            ConversationMessage::user("two"),
            ConversationMessage::assistant(Some("three".to_string()), Vec::new()),
            ConversationMessage::assistant(Some("four".to_string()), Vec::new()),
        ];

        let normalized = normalize_messages(&messages);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0], ConversationMessage::user("one\ntwo"));
        assert_eq!(
            normalized[1],
            ConversationMessage::assistant(Some("three\nfour".to_string()), Vec::new())
        );
    }
}
