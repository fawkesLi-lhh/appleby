use anyhow::Result;
use tokio::sync::mpsc;

use crate::api_adapter::{ConversationMessage, ToolCallRecord};

const COMMAND_CHANNEL_CAPACITY: usize = 8;
const EVENT_CHANNEL_CAPACITY: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum TuiCommand {
    SubmitUserMessage { content: String },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    TurnStarted {
        turn_id: u64,
        user_message: String,
    },
    ModelRequestStarted {
        turn_id: u64,
    },
    AssistantMessageCompleted {
        turn_id: u64,
        message: ConversationMessage,
    },
    ToolCallStarted {
        turn_id: u64,
        call: ToolCallRecord,
        presentation: String,
    },
    ToolCallCompleted {
        turn_id: u64,
        result: ConversationMessage,
    },
    TurnCompleted {
        turn_id: u64,
    },
    TurnFailed {
        turn_id: u64,
        message: String,
    },
    RunnerStopped,
}

pub struct AgentChannel {
    commands: mpsc::Receiver<TuiCommand>,
    events: mpsc::Sender<AgentEvent>,
}

impl AgentChannel {
    pub async fn recv(&mut self) -> Option<TuiCommand> {
        self.commands.recv().await
    }

    pub async fn send(&self, event: AgentEvent) -> Result<()> {
        self.events
            .send(event)
            .await
            .map_err(|_| anyhow::anyhow!("TUI event receiver dropped"))
    }
}

pub struct TuiChannel {
    commands: mpsc::Sender<TuiCommand>,
    events: mpsc::Receiver<AgentEvent>,
}

impl TuiChannel {
    pub async fn send(&self, command: TuiCommand) -> Result<()> {
        self.commands
            .send(command)
            .await
            .map_err(|_| anyhow::anyhow!("agent command receiver dropped"))
    }

    pub async fn recv(&mut self) -> Option<AgentEvent> {
        self.events.recv().await
    }

    pub fn try_recv(&mut self) -> Result<Option<AgentEvent>> {
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                anyhow::bail!("agent event sender dropped")
            }
        }
    }
}

pub fn tui_channel() -> (AgentChannel, TuiChannel) {
    let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
    let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);

    (
        AgentChannel {
            commands: command_rx,
            events: event_tx,
        },
        TuiChannel {
            commands: command_tx,
            events: event_rx,
        },
    )
}
