//! Keyboard handling and asynchronous creation workflow.
//!
//! This is the TUI boundary: it turns key presses into typed service and
//! supervisor calls. Rendering never performs those calls.

use super::model::{
    CreateStage, CreateWizard, DownloadPhase, IdentityField, NoticeKind, Screen, UiState,
};
use crate::instance::{EulaAcceptance, InstanceId, InstanceName};
use crate::runtime::FabricRuntime;
use crate::service::InstanceService;
use crate::supervisor::ServerSupervisor;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::str::FromStr;
use tokio::sync::mpsc;

pub(super) enum RuntimeEvent {
    Resolved(FabricRuntime),
    Finished(Result<FabricRuntime, String>),
}

pub(super) fn apply_runtime_event(state: &mut UiState, event: RuntimeEvent) {
    match event {
        RuntimeEvent::Resolved(runtime) => {
            if let CreateStage::Downloading { phase, .. } = &mut state.wizard.stage {
                *phase = DownloadPhase::Downloading(runtime);
            }
        }
        RuntimeEvent::Finished(Ok(runtime)) => {
            if !state.runtimes.contains(&runtime) {
                state.runtimes.push(runtime.clone());
                state.runtimes.sort_by(|left, right| right.cmp(left));
            }
            state.wizard.stage = CreateStage::Review {
                runtime,
                eula_accepted: false,
            };
            state.notice(
                NoticeKind::Success,
                "Fabric runtime is cached. Review the instance.",
            );
        }
        RuntimeEvent::Finished(Err(message)) => {
            if let CreateStage::Downloading { phase, .. } = &mut state.wizard.stage {
                *phase = DownloadPhase::Failed(message.clone());
            }
            state.notice(NoticeKind::Error, message);
        }
    }
}

pub(super) async fn handle_key(
    key: KeyEvent,
    state: &mut UiState,
    service: &InstanceService,
    supervisor: &ServerSupervisor,
    runtime_tx: &mpsc::Sender<RuntimeEvent>,
) {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        begin_quit(state, supervisor).await;
        return;
    }
    match state.screen {
        Screen::Dashboard => handle_dashboard_key(key, state, service, supervisor).await,
        Screen::Console => handle_console_key(key, state, supervisor).await,
        Screen::Runtimes => handle_runtimes_key(key, state, service),
        Screen::Create => handle_create_key(key, state, service, runtime_tx),
        Screen::Help => state.screen = Screen::Dashboard,
    }
}

async fn handle_dashboard_key(
    key: KeyEvent,
    state: &mut UiState,
    service: &InstanceService,
    supervisor: &ServerSupervisor,
) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => state.select_next_instance(),
        KeyCode::Up | KeyCode::Char('k') => state.select_previous_instance(),
        KeyCode::Char('n') => {
            state.wizard = CreateWizard::default();
            state.screen = Screen::Create;
        }
        KeyCode::Char('v') => {
            refresh_runtimes(state, service);
            state.screen = Screen::Runtimes;
        }
        KeyCode::Char('?') => state.screen = Screen::Help,
        KeyCode::Enter => {
            if state.selected_instance().is_some() {
                state.screen = Screen::Console;
            } else {
                state.notice(NoticeKind::Info, "Create an instance first");
            }
        }
        KeyCode::Char('s') => {
            if let Some(instance) = state.selected_instance().cloned()
                && let Err(error) = supervisor.start(instance).await
            {
                state.notice(NoticeKind::Error, error.to_string());
            }
        }
        KeyCode::Char('x') => {
            if let Some(id) = state
                .selected_instance()
                .map(|instance| instance.id().clone())
                && let Err(error) = supervisor.stop(id).await
            {
                state.notice(NoticeKind::Error, error.to_string());
            }
        }
        KeyCode::Char('r') => match service.list_instances() {
            Ok(instances) => {
                state.replace_instances(instances);
                state.notice(NoticeKind::Success, "Instance list reloaded");
            }
            Err(error) => state.notice(NoticeKind::Error, error.to_string()),
        },
        KeyCode::Char('q') => begin_quit(state, supervisor).await,
        _ => {}
    }
}

async fn handle_console_key(key: KeyEvent, state: &mut UiState, supervisor: &ServerSupervisor) {
    match key.code {
        KeyCode::Esc => state.screen = Screen::Dashboard,
        KeyCode::Backspace => {
            state.console_input.pop();
        }
        KeyCode::Enter => {
            let command = std::mem::take(&mut state.console_input);
            if command.trim().is_empty() {
                return;
            }
            if let Some(id) = state
                .selected_instance()
                .map(|instance| instance.id().clone())
                && let Err(error) = supervisor.send_console(id, command).await
            {
                state.notice(NoticeKind::Error, error.to_string());
            }
        }
        KeyCode::Char(character) if !character.is_control() => state.console_input.push(character),
        _ => {}
    }
}

fn handle_runtimes_key(key: KeyEvent, state: &mut UiState, service: &InstanceService) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('v') => state.screen = Screen::Dashboard,
        KeyCode::Down | KeyCode::Char('j') => state.select_next_runtime(),
        KeyCode::Up | KeyCode::Char('k') => state.select_previous_runtime(),
        KeyCode::Char('r') => refresh_runtimes(state, service),
        KeyCode::Char('n') => {
            state.wizard = CreateWizard::default();
            state.screen = Screen::Create;
        }
        KeyCode::Char('?') => state.screen = Screen::Help,
        _ => {}
    }
}

fn refresh_runtimes(state: &mut UiState, service: &InstanceService) {
    match service.list_runtimes() {
        Ok(runtimes) => state.replace_runtimes(runtimes),
        Err(error) => state.notice(NoticeKind::Error, error.to_string()),
    }
}

fn handle_create_key(
    key: KeyEvent,
    state: &mut UiState,
    service: &InstanceService,
    runtime_tx: &mpsc::Sender<RuntimeEvent>,
) {
    match state.wizard.stage.clone() {
        CreateStage::Identity => handle_identity_key(key, state),
        CreateStage::Runtime { .. } => handle_runtime_choice_key(key, state, service, runtime_tx),
        CreateStage::MinecraftVersion { .. } => {
            handle_minecraft_version_key(key, state, service, runtime_tx)
        }
        CreateStage::Downloading { phase, .. } => {
            if matches!(phase, DownloadPhase::Failed(_))
                && matches!(key.code, KeyCode::Esc | KeyCode::Enter)
            {
                state.wizard.stage = CreateStage::Runtime { selection: 0 };
            } else if key.code == KeyCode::Esc {
                state.notice(NoticeKind::Info, "The Fabric download is still running");
            }
        }
        CreateStage::Review { .. } => handle_review_key(key, state, service),
    }
}

fn handle_identity_key(key: KeyEvent, state: &mut UiState) {
    match key.code {
        KeyCode::Esc => state.screen = Screen::Dashboard,
        KeyCode::Tab | KeyCode::BackTab => {
            state.wizard.identity_field = match state.wizard.identity_field {
                IdentityField::Id => IdentityField::Name,
                IdentityField::Name => IdentityField::Id,
            };
        }
        KeyCode::Backspace => match state.wizard.identity_field {
            IdentityField::Id => {
                state.wizard.id.pop();
            }
            IdentityField::Name => {
                state.wizard.name.pop();
            }
        },
        KeyCode::Char(character) if !character.is_control() => match state.wizard.identity_field {
            IdentityField::Id => state.wizard.id.push(character),
            IdentityField::Name => state.wizard.name.push(character),
        },
        KeyCode::Enter => match validate_identity(&state.wizard) {
            Ok(()) => {
                state.wizard.stage = CreateStage::Runtime { selection: 0 };
                state.notice(
                    NoticeKind::Info,
                    "Choose an installed or new Fabric runtime",
                );
            }
            Err(error) => state.notice(NoticeKind::Error, error),
        },
        _ => {}
    }
}

fn validate_identity(wizard: &CreateWizard) -> Result<(), String> {
    InstanceId::from_str(wizard.id.trim())
        .map_err(|error| format!("Invalid instance ID: {error}"))?;
    InstanceName::parse(&wizard.name).map_err(|error| error.to_string())?;
    Ok(())
}

fn handle_runtime_choice_key(
    key: KeyEvent,
    state: &mut UiState,
    service: &InstanceService,
    runtime_tx: &mpsc::Sender<RuntimeEvent>,
) {
    let CreateStage::Runtime { selection } = &mut state.wizard.stage else {
        return;
    };
    let choice_count = state.runtimes.len() + 2;
    match key.code {
        KeyCode::Esc => state.wizard.stage = CreateStage::Identity,
        KeyCode::Down | KeyCode::Char('j') => *selection = (*selection + 1) % choice_count,
        KeyCode::Up | KeyCode::Char('k') => {
            *selection = selection.checked_sub(1).unwrap_or(choice_count - 1)
        }
        KeyCode::Enter if *selection < state.runtimes.len() => {
            let runtime = state.runtimes[*selection].clone();
            state.wizard.stage = CreateStage::Review {
                runtime,
                eula_accepted: false,
            };
        }
        KeyCode::Enter if *selection == state.runtimes.len() => {
            begin_download(state, None, service, runtime_tx)
        }
        KeyCode::Enter => {
            state.wizard.stage = CreateStage::MinecraftVersion {
                value: String::new(),
            };
        }
        _ => {}
    }
}

fn handle_minecraft_version_key(
    key: KeyEvent,
    state: &mut UiState,
    service: &InstanceService,
    runtime_tx: &mpsc::Sender<RuntimeEvent>,
) {
    let CreateStage::MinecraftVersion { value } = &state.wizard.stage else {
        return;
    };
    let submitted_version = value.trim().to_owned();
    match key.code {
        KeyCode::Esc => state.wizard.stage = CreateStage::Runtime { selection: 0 },
        KeyCode::Backspace => {
            if let CreateStage::MinecraftVersion { value } = &mut state.wizard.stage {
                value.pop();
            }
        }
        KeyCode::Char(character) if !character.is_control() => {
            if let CreateStage::MinecraftVersion { value } = &mut state.wizard.stage {
                value.push(character);
            }
        }
        KeyCode::Enter if submitted_version.is_empty() => {
            state.notice(NoticeKind::Error, "Enter a Minecraft version");
        }
        KeyCode::Enter => begin_download(state, Some(submitted_version), service, runtime_tx),
        _ => {}
    }
}

fn begin_download(
    state: &mut UiState,
    minecraft: Option<String>,
    service: &InstanceService,
    runtime_tx: &mpsc::Sender<RuntimeEvent>,
) {
    state.wizard.stage = CreateStage::Downloading {
        requested: minecraft.clone(),
        phase: DownloadPhase::Resolving,
    };
    state.notice(NoticeKind::Info, "Resolving Fabric versions");
    let service = service.clone();
    let runtime_tx = runtime_tx.clone();
    tokio::spawn(async move {
        let runtime = match service.resolve_runtime(minecraft.as_deref()).await {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = runtime_tx
                    .send(RuntimeEvent::Finished(Err(error.to_string())))
                    .await;
                return;
            }
        };
        if runtime_tx
            .send(RuntimeEvent::Resolved(runtime.clone()))
            .await
            .is_err()
        {
            return;
        }
        let result = service
            .cache_runtime(&runtime)
            .await
            .map(|_| runtime)
            .map_err(|error| error.to_string());
        let _ = runtime_tx.send(RuntimeEvent::Finished(result)).await;
    });
}

fn handle_review_key(key: KeyEvent, state: &mut UiState, service: &InstanceService) {
    match key.code {
        KeyCode::Esc => state.wizard.stage = CreateStage::Runtime { selection: 0 },
        KeyCode::Char(' ') => {
            if let CreateStage::Review { eula_accepted, .. } = &mut state.wizard.stage {
                *eula_accepted = !*eula_accepted;
            }
        }
        KeyCode::Enter => create_instance(state, service),
        _ => {}
    }
}

fn create_instance(state: &mut UiState, service: &InstanceService) {
    let CreateStage::Review {
        runtime,
        eula_accepted,
    } = &state.wizard.stage
    else {
        return;
    };
    if !eula_accepted {
        state.notice(
            NoticeKind::Error,
            "You must accept the Minecraft EULA before creating this instance",
        );
        return;
    }
    let id = match InstanceId::from_str(state.wizard.id.trim()) {
        Ok(id) => id,
        Err(error) => {
            state.notice(NoticeKind::Error, format!("Invalid instance ID: {error}"));
            return;
        }
    };
    let name = match InstanceName::parse(&state.wizard.name) {
        Ok(name) => name,
        Err(error) => {
            state.notice(NoticeKind::Error, error.to_string());
            return;
        }
    };
    let runtime = runtime.clone();
    match service
        .create_with_cached_runtime(id, name, runtime.clone(), EulaAcceptance::Accepted)
        .and_then(|_| service.list_instances())
    {
        Ok(instances) => {
            state.replace_instances(instances);
            state.screen = Screen::Dashboard;
            state.notice(
                NoticeKind::Success,
                format!("Instance created with Minecraft {}", runtime.minecraft),
            );
        }
        Err(error) => state.notice(NoticeKind::Error, error.to_string()),
    }
}

async fn begin_quit(state: &mut UiState, supervisor: &ServerSupervisor) {
    if state.quitting {
        state.notice(NoticeKind::Info, "Forcing running instances to exit");
        if let Err(error) = supervisor.kill_all().await {
            state.notice(NoticeKind::Error, error.to_string());
        }
        return;
    }
    state.quitting = true;
    state.notice(NoticeKind::Info, "Stopping running instances before exit");
    if let Err(error) = supervisor.stop_all().await {
        state.notice(NoticeKind::Error, error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::{CreateStage, CreateWizard, validate_identity};
    use crate::instance::{InstanceId, InstanceName};
    use crate::runtime::FabricRuntime;
    use std::str::FromStr;

    #[test]
    fn create_review_requires_a_runtime() {
        let mut wizard = CreateWizard {
            id: "survival".to_owned(),
            name: "Survival".to_owned(),
            ..CreateWizard::default()
        };
        assert!(validate_identity(&wizard).is_ok());
        wizard.stage = CreateStage::Review {
            runtime: FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap(),
            eula_accepted: false,
        };
        assert!(matches!(wizard.stage, CreateStage::Review { .. }));
        assert_eq!(
            InstanceId::from_str(&wizard.id).unwrap().as_str(),
            "survival"
        );
        assert_eq!(
            InstanceName::parse(&wizard.name).unwrap().to_string(),
            "Survival"
        );
    }
}
