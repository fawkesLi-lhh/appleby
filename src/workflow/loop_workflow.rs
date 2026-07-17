use std::collections::HashSet;

use crate::{
    api_adapter::{ApiRequest, ConversationMessage, ToolCallRecord},
    state::loop_state::LoopState,
};
use anyhow::Context;
use inquire::Text;

#[auto_context::auto_context]
pub async fn loop_workflow(state: &mut LoopState) -> Result<(), anyhow::Error> {
    loop {
        let query = Text::new("Human: ")
            .prompt()
            .context("An error happened or user cancelled the input.")?;

        if query.trim() == "exit()" {
            break;
        }
        state.push_message(ConversationMessage::user(query))?;
        agent_loop(state).await?;
        let Some(final_content) = state
            .get_context()
            .last()
            .and_then(ConversationMessage::assistant_text)
        else {
            continue;
        };
        println!("Assistant: {final_content}");
    }
    Ok(())
}

#[auto_context::auto_context]
pub async fn agent_loop(state: &mut LoopState) -> Result<(), anyhow::Error> {
    loop {
        let messages = normalize_messages(state.get_context());
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
        state.push_message(message)?;

        if tool_calls.is_empty() {
            return Ok(());
        }

        for tool_result in state.execute_tool_calls(&tool_calls).await? {
            state.push_message(tool_result)?;
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
    use super::normalize_messages;
    use crate::api_adapter::{ConversationMessage, ToolCallRecord};
    use serde_json::json;

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
