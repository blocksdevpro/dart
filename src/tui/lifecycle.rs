//! Terminal setup and the top-level UI event loop.

use super::{DownloadEvent, ModEvent, apply_mod_event, draw, handle_key, model::App};
use crate::daemon::{ContentManager, Daemon, InstanceService, ServerEvent, ServerSupervisor};
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::error::Error;
use std::io;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};

const TICK_RATE: Duration = Duration::from_millis(60);

pub async fn run(
    daemon: Daemon,
    mut server_events: broadcast::Receiver<ServerEvent>,
) -> Result<(), Box<dyn Error>> {
    let service = daemon.instance_service().clone();
    let content_manager = daemon.content_manager().clone();
    let supervisor = daemon.supervisor().clone();
    let instances = service.list_instances()?;
    let runtimes = service.list_runtimes()?;
    let mut app = App::new(instances.clone(), runtimes);
    for instance in &instances {
        let state = supervisor.state(instance.id());
        if state != crate::InstanceState::Stopped {
            app.update_state(instance.id().clone(), state);
        }
        for record in supervisor.recent_logs(instance.id(), Some(100)) {
            app.append_console_line(instance.id().clone(), record.stream, record.line);
        }
    }
    let (download_tx, mut download_rx) = mpsc::channel::<DownloadEvent>(4);
    let (mod_tx, mut mod_rx) = mpsc::channel::<ModEvent>(4);

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

    let run_result = match terminal.clear() {
        Ok(()) => {
            event_loop(
                &mut terminal,
                &mut app,
                &service,
                &content_manager,
                &supervisor,
                &download_tx,
                &mut download_rx,
                &mod_tx,
                &mut mod_rx,
                &mut server_events,
            )
            .await
        }
        Err(error) => Err(Box::new(error) as Box<dyn Error>),
    };

    let raw_mode_result = disable_raw_mode();
    let screen_result = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let cursor_result = terminal.show_cursor();

    run_result?;
    raw_mode_result?;
    screen_result?;
    cursor_result?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    service: &InstanceService,
    content_manager: &ContentManager,
    supervisor: &ServerSupervisor,
    download_tx: &mpsc::Sender<DownloadEvent>,
    download_rx: &mut mpsc::Receiver<DownloadEvent>,
    mod_tx: &mpsc::Sender<ModEvent>,
    mod_rx: &mut mpsc::Receiver<ModEvent>,
    server_events: &mut broadcast::Receiver<ServerEvent>,
) -> Result<(), Box<dyn Error>> {
    loop {
        loop {
            match server_events.try_recv() {
                Ok(event) => app.apply_server_event(event),
                Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(
                    broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed,
                ) => break,
            }
        }
        while let Ok(event) = download_rx.try_recv() {
            match event {
                DownloadEvent::Resolved(runtime) => app.download_resolved(runtime),
                DownloadEvent::Finished(Ok(runtime)) => app.download_finished(runtime),
                DownloadEvent::Finished(Err(message)) => app.download_failed(message),
            }
        }
        while let Ok(event) = mod_rx.try_recv() {
            apply_mod_event(app, event);
        }

        if app.is_quitting() {
            return Ok(());
        }

        app.tick();
        terminal.draw(|frame| draw(frame, app))?;

        let mut handled_input = false;
        while event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()?
                && key.kind == event::KeyEventKind::Press
            {
                handle_key(
                    key,
                    app,
                    service,
                    content_manager,
                    supervisor,
                    download_tx,
                    mod_tx,
                )
                .await;
                handled_input = true;
            }
        }

        if handled_input {
            app.tick();
            terminal.draw(|frame| draw(frame, app))?;
        }

        tokio::time::sleep(TICK_RATE).await;
    }
}
