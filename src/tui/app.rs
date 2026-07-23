use crate::{api_adapter::ConversationMessage, workflow::tui_channel::AgentEvent};

const TOOL_OUTPUT_PREVIEW_LIMIT: usize = 2_000;
const SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AppStatus {
    Idle,
    Thinking,
    ToolRunning,
    ShuttingDown,
    Failed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TimelineKind {
    User,
    Assistant,
    Tool,
    Error,
}

#[derive(Debug, Eq, PartialEq)]
pub struct TimelineEntry {
    pub kind: TimelineKind,
    pub title: String,
    pub content: String,
}

#[derive(Debug)]
pub struct App {
    pub input: String,
    pub entries: Vec<TimelineEntry>,
    pub status: AppStatus,
    pub scroll: u16,
    pub follow_tail: bool,
    pub should_quit: bool,
    spinner_frame: usize,
}

impl App {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            entries: Vec::new(),
            status: AppStatus::Idle,
            scroll: 0,
            follow_tail: true,
            should_quit: false,
            spinner_frame: 0,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.status, AppStatus::Idle | AppStatus::Failed)
    }

    pub fn status_spinner(&self) -> Option<&'static str> {
        if matches!(self.status, AppStatus::Thinking | AppStatus::ToolRunning) {
            Some(SPINNER_FRAMES[self.spinner_frame])
        } else {
            None
        }
    }

    pub fn advance_animation(&mut self) {
        if self.status_spinner().is_some() {
            self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
        }
    }

    pub fn submit_input(&mut self) -> Option<String> {
        let content = self.input.trim().to_string();
        if content.is_empty() {
            return None;
        }

        self.input.clear();
        self.status = AppStatus::Thinking;
        Some(content)
    }

    pub fn apply_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStarted { user_message, .. } => {
                self.entries.push(TimelineEntry {
                    kind: TimelineKind::User,
                    title: "You".to_string(),
                    content: user_message,
                });
                self.status = AppStatus::Thinking;
                self.follow_tail = true;
            }
            AgentEvent::ModelRequestStarted { .. } => {
                self.status = AppStatus::Thinking;
            }
            AgentEvent::AssistantMessageCompleted { message, .. } => {
                if let ConversationMessage::Assistant {
                    content: Some(content),
                    ..
                } = message
                {
                    self.entries.push(TimelineEntry {
                        kind: TimelineKind::Assistant,
                        title: "Assistant".to_string(),
                        content,
                    });
                }
            }
            AgentEvent::ToolCallStarted {
                call, presentation, ..
            } => {
                self.status = AppStatus::ToolRunning;
                self.entries.push(TimelineEntry {
                    kind: TimelineKind::Tool,
                    title: format!("Tool · {} · running", call.name),
                    content: presentation,
                });
            }
            AgentEvent::ToolCallCompleted { result, .. } => {
                if let ConversationMessage::Tool {
                    tool_call_id,
                    content,
                } = result
                {
                    self.entries.push(TimelineEntry {
                        kind: TimelineKind::Tool,
                        title: format!("Tool · {tool_call_id} · completed"),
                        content: preview_tool_output(&content),
                    });
                }
            }
            AgentEvent::TurnCompleted { .. } => {
                if self.status != AppStatus::ShuttingDown {
                    self.status = AppStatus::Idle;
                }
            }
            AgentEvent::TurnFailed { message, .. } => {
                self.entries.push(TimelineEntry {
                    kind: TimelineKind::Error,
                    title: "Agent error".to_string(),
                    content: message,
                });
                self.status = AppStatus::Failed;
            }
            AgentEvent::RunnerStopped => self.should_quit = true,
        }
    }

    pub fn scroll_up(&mut self) {
        self.follow_tail = false;
        self.scroll = self.scroll.saturating_sub(3);
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(3);
    }

    pub fn resolve_scroll(&mut self, max_scroll: u16) -> u16 {
        if self.follow_tail {
            self.scroll = max_scroll;
        } else {
            self.scroll = self.scroll.min(max_scroll);
            if self.scroll == max_scroll {
                self.follow_tail = true;
            }
        }
        self.scroll
    }
}

fn preview_tool_output(content: &str) -> String {
    let mut characters = content.chars();
    let preview = characters
        .by_ref()
        .take(TOOL_OUTPUT_PREVIEW_LIMIT)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{preview}\n… output truncated in TUI preview")
    } else {
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::{App, AppStatus, TimelineKind};
    use crate::{api_adapter::ConversationMessage, workflow::tui_channel::AgentEvent};

    #[test]
    fn submission_waits_for_turn_started_before_adding_committed_user_entry() {
        let mut app = App::new();
        app.input = "  hello  ".to_string();

        assert_eq!(app.submit_input(), Some("hello".to_string()));
        assert_eq!(app.status, AppStatus::Thinking);
        assert!(app.entries.is_empty());

        app.apply_agent_event(AgentEvent::TurnStarted {
            turn_id: 1,
            user_message: "hello".to_string(),
        });

        assert_eq!(app.entries.len(), 1);
        assert_eq!(app.entries[0].kind, TimelineKind::User);
        assert_eq!(app.entries[0].content, "hello");
        assert!(app.follow_tail);
    }

    #[test]
    fn failed_commit_does_not_present_user_message_as_committed() {
        let mut app = App::new();
        app.input = "hello".to_string();

        assert_eq!(app.submit_input(), Some("hello".to_string()));
        app.apply_agent_event(AgentEvent::TurnFailed {
            turn_id: 1,
            message: "append failed".to_string(),
        });

        assert_eq!(app.entries.len(), 1);
        assert_eq!(app.entries[0].kind, TimelineKind::Error);
        assert_eq!(app.entries[0].content, "append failed");
        assert_eq!(app.status, AppStatus::Failed);
    }

    #[test]
    fn tool_output_preview_is_truncated_without_changing_activity_state() {
        let mut app = App::new();
        let output = "x".repeat(2_001);

        app.apply_agent_event(AgentEvent::ToolCallCompleted {
            turn_id: 1,
            result: ConversationMessage::tool("call-1", output),
        });

        assert!(
            app.entries[0]
                .content
                .ends_with("output truncated in TUI preview")
        );
        assert_eq!(app.status, AppStatus::Idle);
    }

    #[test]
    fn model_request_switches_to_thinking_and_advances_spinner() {
        let mut app = App::new();
        app.apply_agent_event(AgentEvent::ModelRequestStarted { turn_id: 1 });
        let first_frame = app.status_spinner();

        app.advance_animation();

        assert_eq!(app.status, AppStatus::Thinking);
        assert_ne!(app.status_spinner(), first_frame);
    }

    #[test]
    fn scroll_follows_tail_until_user_scrolls_up() {
        let mut app = App::new();
        assert_eq!(app.resolve_scroll(12), 12);

        app.scroll_up();
        assert!(!app.follow_tail);
        assert_eq!(app.resolve_scroll(12), 9);

        app.scroll_down();
        assert_eq!(app.resolve_scroll(12), 12);
        assert!(app.follow_tail);
    }
}
