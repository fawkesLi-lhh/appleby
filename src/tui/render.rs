use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::app::{App, AppStatus, TimelineKind};

pub fn render(frame: &mut Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .split(frame.area());

    frame.render_widget(header(app), areas[0]);
    frame.render_widget(transcript(app, areas[1]), areas[1]);
    frame.render_widget(input(app), areas[2]);
    frame.render_widget(footer(), areas[3]);
}

fn header(app: &App) -> Paragraph<'static> {
    let activity = app
        .status_spinner()
        .map(|spinner| format!("{spinner} "))
        .unwrap_or_default();
    let (status_label, status_style) = status_view(app.status);
    Paragraph::new(Line::from(vec![
        Span::styled(" Appleby ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("· "),
        Span::styled(format!("{activity}{status_label}"), status_style),
    ]))
    .style(Style::default().bg(Color::DarkGray))
}

fn transcript(app: &mut App, area: Rect) -> Paragraph<'_> {
    let mut lines = Vec::new();
    for entry in &app.entries {
        lines.push(Line::from(Span::styled(
            entry.title.clone(),
            entry_style(entry.kind).add_modifier(Modifier::BOLD),
        )));
        for line in entry.content.lines() {
            lines.push(Line::from(Span::raw(line.to_string())));
        }
        lines.push(Line::default());
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Send a message to begin.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let content_width = area.width.saturating_sub(2).max(1);
    let content_height = area.height.saturating_sub(2);
    let max_scroll = wrapped_line_count(&lines, usize::from(content_width))
        .saturating_sub(usize::from(content_height));
    let max_scroll = u16::try_from(max_scroll).unwrap_or(u16::MAX);

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Conversation"))
        .wrap(Wrap { trim: false })
        .scroll((app.resolve_scroll(max_scroll), 0))
}

fn wrapped_line_count(lines: &[Line<'_>], width: usize) -> usize {
    lines
        .iter()
        .map(|line| line.width().div_ceil(width).max(1))
        .sum()
}

fn input(app: &App) -> Paragraph<'_> {
    let content = if app.input.is_empty() {
        Line::from(Span::styled(
            "Type a message…",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from(app.input.as_str())
    };

    Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title("Input"))
        .wrap(Wrap { trim: false })
}

fn footer() -> Paragraph<'static> {
    Paragraph::new("Enter submit · Shift+Enter newline · ↑/↓ scroll · Esc clear · Ctrl+Q quit")
        .style(Style::default().fg(Color::DarkGray))
}

fn status_view(status: AppStatus) -> (&'static str, Style) {
    match status {
        AppStatus::Idle => ("Idle", Style::default().fg(Color::Green)),
        AppStatus::Thinking => ("Thinking…", Style::default().fg(Color::Yellow)),
        AppStatus::ToolRunning => ("Running tool…", Style::default().fg(Color::LightYellow)),
        AppStatus::ShuttingDown => ("Shutting down", Style::default().fg(Color::LightRed)),
        AppStatus::Failed => ("Failed", Style::default().fg(Color::Red)),
    }
}

fn entry_style(kind: TimelineKind) -> Style {
    match kind {
        TimelineKind::User => Style::default().fg(Color::Cyan),
        TimelineKind::Assistant => Style::default().fg(Color::Green),
        TimelineKind::Tool => Style::default().fg(Color::Yellow),
        TimelineKind::Error => Style::default().fg(Color::Red),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::render;
    use crate::{
        api_adapter::ConversationMessage, tui::app::App, workflow::tui_channel::AgentEvent,
    };

    #[test]
    fn renders_header_conversation_input_and_footer() {
        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.apply_agent_event(AgentEvent::TurnStarted {
            turn_id: 1,
            user_message: "hello".to_string(),
        });

        terminal.draw(|frame| render(frame, &mut app)).unwrap();

        let output = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(output.contains("Appleby"));
        assert!(output.contains("Conversation"));
        assert!(output.contains("Input"));
        assert!(output.contains("Ctrl+Q quit"));
        assert!(output.contains("hello"));
        assert!(output.contains("Thinking"));
    }

    #[test]
    fn follows_the_last_wrapped_transcript_line() {
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.apply_agent_event(AgentEvent::AssistantMessageCompleted {
            turn_id: 1,
            message: ConversationMessage::assistant(Some("word ".repeat(100)), Vec::new()),
        });

        terminal.draw(|frame| render(frame, &mut app)).unwrap();

        assert!(app.follow_tail);
        assert!(app.scroll > 0);
    }
}
