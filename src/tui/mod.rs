mod app;
mod render;

use std::{
    io::{self, Stdout},
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use self::app::App;
use crate::workflow::tui_channel::{TuiChannel, TuiCommand};

pub async fn run(mut channel: TuiChannel) -> Result<()> {
    let mut terminal = TerminalGuard::new()?;
    let mut app = App::new();

    loop {
        while let Some(event) = channel.try_recv()? {
            app.apply_agent_event(event);
        }
        app.advance_animation();
        terminal.draw(&mut app)?;

        if app.should_quit {
            return Ok(());
        }

        if event::poll(Duration::from_millis(50)).context("poll terminal events")?
            && let Event::Key(key) = event::read().context("read terminal event")?
            && key.kind == KeyEventKind::Press
        {
            handle_key(key, &mut app, &channel).await?;
        }
    }
}

async fn handle_key(key: KeyEvent, app: &mut App, channel: &TuiChannel) -> Result<()> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'q'))
    {
        request_shutdown(app, channel).await?;
        return Ok(());
    }

    if app.status == app::AppStatus::ShuttingDown {
        return Ok(());
    }

    match key.code {
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => app.input.push('\n'),
        KeyCode::Enter if app.is_idle() => {
            if let Some(content) = app.submit_input() {
                channel
                    .send(TuiCommand::SubmitUserMessage { content })
                    .await
                    .context("send user message to agent")?;
            }
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Esc => app.input.clear(),
        KeyCode::Up => app.scroll_up(),
        KeyCode::Down => app.scroll_down(),
        KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input.push(character);
        }
        _ => {}
    }

    Ok(())
}

async fn request_shutdown(app: &mut App, channel: &TuiChannel) -> Result<()> {
    if app.status != app::AppStatus::ShuttingDown {
        channel
            .send(TuiCommand::Shutdown)
            .await
            .context("request agent shutdown")?;
        app.status = app::AppStatus::ShuttingDown;
    }
    Ok(())
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode().context("enable terminal raw mode")?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error).context("enter terminal alternate screen");
        }

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                let _ = disable_raw_mode();
                return Err(error).context("create Ratatui terminal");
            }
        };
        terminal.clear().context("clear Ratatui terminal")?;

        Ok(Self { terminal })
    }

    fn draw(&mut self, app: &mut App) -> Result<()> {
        self.terminal
            .draw(|frame| render::render(frame, app))
            .context("draw Ratatui frame")?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}
