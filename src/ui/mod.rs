//! Interactive terminal interface.
//!
//! This module owns terminal setup and the event loop. Its child modules have
//! narrower responsibilities: `model` owns in-memory UI state, `input` maps
//! keys to application actions, and `render` draws state without side effects.

mod input;
mod model;
mod render;

use crate::service::InstanceService;
use crate::supervisor::{ServerEvent, ServerSupervisor};
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use input::{RuntimeEvent, apply_runtime_event, handle_key};
use model::UiState;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::error::Error;
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

const TICK_RATE: Duration = Duration::from_millis(60);

pub async fn run(
    service: InstanceService,
    supervisor: ServerSupervisor,
    mut server_events: mpsc::Receiver<ServerEvent>,
) -> Result<(), Box<dyn Error>> {
    let mut state = UiState::new(service.list_instances()?, service.list_runtimes()?);
    let (runtime_tx, mut runtime_events) = mpsc::channel(4);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(Box::new(error));
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            return Err(Box::new(error));
        }
    };

    let result = event_loop(
        &mut terminal,
        &mut state,
        &service,
        &supervisor,
        &runtime_tx,
        &mut runtime_events,
        &mut server_events,
    )
    .await;
    let raw_result = disable_raw_mode();
    let screen_result = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let cursor_result = terminal.show_cursor();

    result?;
    raw_result?;
    screen_result?;
    cursor_result?;
    Ok(())
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut UiState,
    service: &InstanceService,
    supervisor: &ServerSupervisor,
    runtime_tx: &mpsc::Sender<RuntimeEvent>,
    runtime_events: &mut mpsc::Receiver<RuntimeEvent>,
    server_events: &mut mpsc::Receiver<ServerEvent>,
) -> Result<(), Box<dyn Error>> {
    loop {
        while let Ok(event) = server_events.try_recv() {
            state.apply_server_event(event);
        }
        while let Ok(event) = runtime_events.try_recv() {
            apply_runtime_event(state, event);
        }
        if state.quitting && !state.has_active_instances() {
            return Ok(());
        }

        state.tick();
        terminal.draw(|frame| render::draw(frame, state))?;
        while event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()?
                && key.kind == event::KeyEventKind::Press
            {
                handle_key(key, state, service, supervisor, runtime_tx).await;
            }
        }
        tokio::time::sleep(TICK_RATE).await;
    }
}
