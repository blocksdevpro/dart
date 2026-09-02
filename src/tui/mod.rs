mod lifecycle;
mod model;

pub use lifecycle::run;

use self::model::{
    App, CreateField, CreateStep, DownloadStatus, ModsFocus, ModsPhase, ModsTab, NoticeKind,
    Screen, state_label,
};
use crate::content::{
    ContentInstallOutcome, ContentInstallPlan, ContentInstallReport, ContentKind, ContentManager,
    ContentSearchHit, InstalledContent,
};
use crate::instance::InstanceService;
use crate::instance::{EulaAcceptance, InstanceId, InstanceName, InstanceState};
use crate::mods::InstalledMod;
use crate::packs::InstalledPack;
use crate::runtime::{FABRIC_LAUNCHER_FILE, FabricRuntime};
use crate::supervisor::{OutputStream, ServerSupervisor};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Row, Table,
    TableState, Wrap,
};
use std::str::FromStr;
use tokio::sync::mpsc;

const ACCENT: Color = Color::Rgb(90, 200, 250);
const MUTED: Color = Color::Rgb(125, 135, 150);
const GOOD: Color = Color::Rgb(95, 215, 140);
const WARN: Color = Color::Rgb(250, 190, 80);
const BAD: Color = Color::Rgb(245, 105, 120);

pub(super) enum DownloadEvent {
    Resolved(FabricRuntime),
    Finished(Result<FabricRuntime, String>),
}

pub(super) enum ModEvent {
    Search {
        instance_id: InstanceId,
        query: String,
        result: Result<Vec<ContentSearchHit>, String>,
    },
    Progress {
        instance_id: InstanceId,
        message: String,
    },
    Finished {
        instance_id: InstanceId,
        result: Result<ModFinished, String>,
    },
}

pub(super) struct ModFinished {
    installed: Vec<InstalledContent>,
    message: String,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_key(
    key: KeyEvent,
    app: &mut App,
    service: &InstanceService,
    content_manager: &ContentManager,
    supervisor: &ServerSupervisor,
    download_tx: &mpsc::Sender<DownloadEvent>,
    mod_tx: &mpsc::Sender<ModEvent>,
) {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        begin_quit(app, supervisor).await;
        return;
    }

    match app.screen() {
        Screen::Dashboard => {
            handle_dashboard_key(key, app, service, content_manager, supervisor).await
        }
        Screen::Console => handle_console_key(key, app, supervisor).await,
        Screen::Create => handle_create_key(key, app, service, download_tx),
        Screen::Mods => handle_mods_key(key, app, content_manager, mod_tx),
        Screen::Runtimes => handle_runtimes_key(key, app),
        Screen::Help => app.close_overlay(),
    }
}

async fn handle_dashboard_key(
    key: KeyEvent,
    app: &mut App,
    service: &InstanceService,
    content_manager: &ContentManager,
    supervisor: &ServerSupervisor,
) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
        KeyCode::Char('n') => app.open_create(),
        KeyCode::Char('m') => {
            if let Some(instance) = app.selected().cloned() {
                match content_manager.list(&instance, ContentKind::Mod) {
                    Ok(mods) => app.open_mods(mods),
                    Err(error) => app.set_notice(NoticeKind::Error, error.to_string()),
                }
            } else {
                app.set_notice(NoticeKind::Info, "Create an instance first");
            }
        }
        KeyCode::Char('v') => app.open_runtimes(),
        KeyCode::Char('?') => app.open_help(),
        KeyCode::Enter => app.open_console(),
        KeyCode::Char('s') => {
            if let Some(instance) = app.selected().cloned() {
                if let Err(error) = supervisor.start(instance).await {
                    app.set_notice(NoticeKind::Error, error.to_string());
                }
            } else {
                app.set_notice(NoticeKind::Info, "Create an instance first");
            }
        }
        KeyCode::Char('x') => {
            if let Some(id) = app.selected().map(|instance| instance.id().clone())
                && let Err(error) = supervisor.stop(id).await
            {
                app.set_notice(NoticeKind::Error, error.to_string());
            }
        }
        KeyCode::Char('r') => match service.list_instances() {
            Ok(instances) => {
                app.replace_instances(instances);
                app.set_notice(NoticeKind::Success, "Instance list reloaded");
            }
            Err(error) => app.set_notice(NoticeKind::Error, error.to_string()),
        },
        KeyCode::Char('q') => begin_quit(app, supervisor).await,
        _ => {}
    }
}

pub(super) fn apply_mod_event(app: &mut App, event: ModEvent) {
    let applies_to_current_view = |instance_id: &InstanceId| {
        app.mods()
            .is_some_and(|mods| mods.instance().id() == instance_id)
    };
    match event {
        ModEvent::Search {
            instance_id,
            query,
            result,
        } if applies_to_current_view(&instance_id) => match result {
            Ok(results) => {
                let count = results.len();
                app.finish_mod_search(&query, results);
                app.set_notice(
                    NoticeKind::Success,
                    format!("Modrinth returned {count} compatible result(s)"),
                );
            }
            Err(message) => {
                app.cancel_mod_operation();
                app.set_notice(NoticeKind::Error, message);
            }
        },
        ModEvent::Progress {
            instance_id,
            message,
        } if applies_to_current_view(&instance_id) => {
            app.update_mod_operation(message.clone());
            app.set_notice(NoticeKind::Info, message);
        }
        ModEvent::Finished {
            instance_id,
            result,
        } if applies_to_current_view(&instance_id) => match result {
            Ok(finished) => {
                app.finish_mod_operation(finished.installed);
                app.set_notice(NoticeKind::Success, finished.message);
            }
            Err(message) => {
                app.cancel_mod_operation();
                app.set_notice(NoticeKind::Error, message);
            }
        },
        _ => {}
    }
}

fn handle_mods_key(
    key: KeyEvent,
    app: &mut App,
    content_manager: &ContentManager,
    mod_tx: &mpsc::Sender<ModEvent>,
) {
    let (busy, remove_confirmation, tab, focus) = match app.mods() {
        Some(mods) => (
            mods.phase().is_busy(),
            mods.remove_confirmation().is_some(),
            mods.tab(),
            mods.focus(),
        ),
        None => {
            app.close_overlay();
            return;
        }
    };
    if busy {
        if key.code == KeyCode::Esc {
            app.set_notice(NoticeKind::Info, "The content operation is still running");
        }
        return;
    }
    if remove_confirmation {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => begin_mod_removal(app, content_manager, mod_tx),
            KeyCode::Esc | KeyCode::Char('n') => app.cancel_mod_removal(),
            _ => {}
        }
        return;
    }

    if matches!(focus, ModsFocus::SearchInput) {
        match key.code {
            KeyCode::Esc => app.close_overlay(),
            KeyCode::Left => switch_content_kind(app, content_manager, false),
            KeyCode::Right => switch_content_kind(app, content_manager, true),
            KeyCode::Tab | KeyCode::BackTab => app.mods_switch_tab(),
            KeyCode::Backspace => app.pop_mod_query_character(),
            KeyCode::Enter => begin_mod_search(app, content_manager, mod_tx),
            KeyCode::Char(character) => app.push_mod_query_character(character),
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('m') => app.close_overlay(),
        KeyCode::Left | KeyCode::Char('h') => switch_content_kind(app, content_manager, false),
        KeyCode::Right | KeyCode::Char('l') => switch_content_kind(app, content_manager, true),
        KeyCode::Tab | KeyCode::BackTab => app.mods_switch_tab(),
        KeyCode::Down | KeyCode::Char('j') => app.mods_select_next(),
        KeyCode::Up | KeyCode::Char('k') => app.mods_select_previous(),
        KeyCode::Char('/') => app.mods_focus_search(),
        KeyCode::Char('r') if matches!(tab, ModsTab::Installed) => {
            reload_mods(app, content_manager)
        }
        KeyCode::Char('u') if matches!(tab, ModsTab::Installed) => {
            begin_mod_update(app, content_manager, mod_tx)
        }
        KeyCode::Char('d') if matches!(tab, ModsTab::Installed) => {
            if !app.request_mod_removal() {
                app.set_notice(
                    NoticeKind::Info,
                    "Dart removes only content it installed. External files and settings stay untouched.",
                );
            }
        }
        KeyCode::Enter | KeyCode::Char('i') if matches!(focus, ModsFocus::SearchResults) => {
            begin_mod_install(app, content_manager, mod_tx)
        }
        KeyCode::Backspace if matches!(focus, ModsFocus::SearchResults) => {
            app.mods_focus_search();
            app.pop_mod_query_character();
        }
        KeyCode::Char(character) if matches!(focus, ModsFocus::SearchResults) => {
            app.mods_focus_search();
            app.push_mod_query_character(character);
        }
        _ => {}
    }
}

fn switch_content_kind(app: &mut App, content_manager: &ContentManager, forward: bool) {
    let Some((instance, current)) = app
        .mods()
        .map(|mods| (mods.instance().clone(), mods.kind()))
    else {
        return;
    };
    let kind = if forward {
        current.next()
    } else {
        current.previous()
    };
    match content_manager.list(&instance, kind) {
        Ok(installed) => app.replace_content_kind(kind, installed),
        Err(error) => app.set_notice(NoticeKind::Error, error.to_string()),
    }
}

fn reload_mods(app: &mut App, content_manager: &ContentManager) {
    let Some(instance) = app.mods().map(|mods| mods.instance().clone()) else {
        return;
    };
    let kind = app.mods().map_or(ContentKind::Mod, |mods| mods.kind());
    match content_manager.list(&instance, kind) {
        Ok(installed) => {
            if let Some(mods) = app.mods_mut() {
                mods.replace_installed(installed);
            }
            app.set_notice(NoticeKind::Success, "Installed content reloaded");
        }
        Err(error) => app.set_notice(NoticeKind::Error, error.to_string()),
    }
}

fn begin_mod_search(
    app: &mut App,
    content_manager: &ContentManager,
    mod_tx: &mpsc::Sender<ModEvent>,
) {
    let Some((instance, query)) = app.begin_mod_search() else {
        app.set_notice(NoticeKind::Error, "Enter a Modrinth search query");
        return;
    };
    let instance_id = instance.id().clone();
    let minecraft = instance.config().fabric.minecraft.clone();
    let kind = app.mods().map_or(ContentKind::Mod, |mods| mods.kind());
    let content_manager = content_manager.clone();
    let mod_tx = mod_tx.clone();
    app.set_notice(
        NoticeKind::Info,
        format!("Searching Modrinth for '{query}'"),
    );
    tokio::spawn(async move {
        let result = content_manager
            .search(kind, &query, &minecraft)
            .await
            .map_err(|error| error.to_string());
        let _ = mod_tx
            .send(ModEvent::Search {
                instance_id,
                query,
                result,
            })
            .await;
    });
}

fn begin_mod_install(
    app: &mut App,
    content_manager: &ContentManager,
    mod_tx: &mpsc::Sender<ModEvent>,
) {
    let Some((instance, hit)) = app.begin_mod_install() else {
        app.set_notice(
            NoticeKind::Info,
            "Select a compatible Modrinth result first",
        );
        return;
    };
    let instance_id = instance.id().clone();
    let minecraft = instance.config().fabric.minecraft.clone();
    let kind = hit.kind();
    let content_manager = content_manager.clone();
    let mod_tx = mod_tx.clone();
    tokio::spawn(async move {
        let result = async {
            let plan = content_manager.prepare_install(hit, &minecraft).await?;
            send_mod_progress(&mod_tx, &instance_id, dependency_plan_message(&plan)).await;
            let report = content_manager.apply_plan(&instance, &plan).await?;
            Ok(ModFinished {
                installed: content_manager.list(&instance, kind)?,
                message: install_report_message(&report, false),
            })
        }
        .await
        .map_err(|error: crate::content::ContentError| error.to_string());
        let _ = mod_tx
            .send(ModEvent::Finished {
                instance_id,
                result,
            })
            .await;
    });
}

fn begin_mod_update(
    app: &mut App,
    content_manager: &ContentManager,
    mod_tx: &mpsc::Sender<ModEvent>,
) {
    let Some((instance, managed)) = app.begin_mod_update() else {
        app.set_notice(
            NoticeKind::Info,
            "Select Dart-managed content to check for an update",
        );
        return;
    };
    let instance_id = instance.id().clone();
    let minecraft = instance.config().fabric.minecraft.clone();
    let kind = managed.kind();
    let content_manager = content_manager.clone();
    let mod_tx = mod_tx.clone();
    tokio::spawn(async move {
        let result = async {
            let plan = content_manager.prepare_update(managed, &minecraft).await?;
            send_mod_progress(&mod_tx, &instance_id, dependency_plan_message(&plan)).await;
            let report = content_manager.apply_plan(&instance, &plan).await?;
            Ok(ModFinished {
                installed: content_manager.list(&instance, kind)?,
                message: install_report_message(&report, true),
            })
        }
        .await
        .map_err(|error: crate::content::ContentError| error.to_string());
        let _ = mod_tx
            .send(ModEvent::Finished {
                instance_id,
                result,
            })
            .await;
    });
}

fn begin_mod_removal(
    app: &mut App,
    content_manager: &ContentManager,
    mod_tx: &mpsc::Sender<ModEvent>,
) {
    let Some((instance, managed)) = app.begin_mod_removal() else {
        return;
    };
    let instance_id = instance.id().clone();
    let kind = managed.kind();
    let title = managed.title().to_owned();
    let content_manager = content_manager.clone();
    let mod_tx = mod_tx.clone();
    tokio::spawn(async move {
        let result = (|| {
            content_manager.remove(&instance, &managed)?;
            Ok(ModFinished {
                installed: content_manager.list(&instance, kind)?,
                message: format!("Removed {title}"),
            })
        })()
        .map_err(|error: crate::content::ContentError| error.to_string());
        let _ = mod_tx
            .send(ModEvent::Finished {
                instance_id,
                result,
            })
            .await;
    });
}

async fn send_mod_progress(
    mod_tx: &mpsc::Sender<ModEvent>,
    instance_id: &InstanceId,
    message: String,
) {
    let _ = mod_tx
        .send(ModEvent::Progress {
            instance_id: instance_id.clone(),
            message,
        })
        .await;
}

fn dependency_plan_message(plan: &ContentInstallPlan) -> String {
    let dependencies = plan.dependency_titles();
    if dependencies.is_empty() {
        return format!("Installing {} {}", plan.kind().singular(), plan.title());
    }
    format!(
        "Installing {} with {} required dependenc{}: {}",
        plan.title(),
        dependencies.len(),
        if dependencies.len() == 1 { "y" } else { "ies" },
        dependencies.join(", ")
    )
}

fn install_report_message(report: &ContentInstallReport, updating: bool) -> String {
    let dependencies = report.dependency_titles();
    let dependency_note = if dependencies.is_empty() {
        String::new()
    } else {
        format!(
            " with {} required dependenc{}: {}",
            dependencies.len(),
            if dependencies.len() == 1 { "y" } else { "ies" },
            dependencies.join(", ")
        )
    };

    if updating
        && report.outcome() == ContentInstallOutcome::AlreadyInstalled
        && report.changed_dependencies() == 0
    {
        return format!(
            "{}{} is already up to date",
            report.title(),
            dependency_note
        );
    }

    let verb = match report.outcome() {
        ContentInstallOutcome::Added => "Installed",
        ContentInstallOutcome::Updated => "Updated",
        ContentInstallOutcome::AlreadyInstalled if report.changed_dependencies() > 0 => {
            "Synchronized dependencies for"
        }
        ContentInstallOutcome::AlreadyInstalled => "Already installed",
    };
    let activation_note = match report.kind() {
        ContentKind::Mod => "",
        ContentKind::DataPack => ". Run /reload or restart the server to activate it",
        ContentKind::ResourcePack => ". Restart the server before players reconnect",
    };
    format!(
        "{verb} {} {}{dependency_note}",
        report.title(),
        report.version()
    ) + activation_note
}

async fn handle_console_key(key: KeyEvent, app: &mut App, supervisor: &ServerSupervisor) {
    match key.code {
        KeyCode::Esc => app.close_overlay(),
        KeyCode::Backspace => app.pop_console_character(),
        KeyCode::Enter => {
            let command = app.take_console_input();
            if command.trim().is_empty() {
                return;
            }
            if let Some(id) = app.selected().map(|instance| instance.id().clone())
                && let Err(error) = supervisor.send_console(id, command).await
            {
                app.set_notice(NoticeKind::Error, error.to_string());
            }
        }
        KeyCode::Char(character) => app.push_console_character(character),
        _ => {}
    }
}

fn handle_runtimes_key(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('v') => app.close_overlay(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next_runtime(),
        KeyCode::Up | KeyCode::Char('k') => app.select_previous_runtime(),
        KeyCode::Char('n') => app.open_create(),
        KeyCode::Char('?') => app.open_help(),
        _ => {}
    }
}

fn handle_create_key(
    key: KeyEvent,
    app: &mut App,
    service: &InstanceService,
    download_tx: &mpsc::Sender<DownloadEvent>,
) {
    match app.create_form().step {
        CreateStep::Identity => handle_identity_key(key, app),
        CreateStep::Runtime => handle_runtime_choice_key(key, app, service, download_tx),
        CreateStep::MinecraftVersion => {
            handle_minecraft_version_key(key, app, service, download_tx)
        }
        CreateStep::Downloading => {
            if matches!(
                app.create_form().download,
                Some(DownloadStatus::Failed { .. })
            ) {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    app.create_form_mut().step = CreateStep::Runtime;
                    app.create_form_mut().download = None;
                }
            } else if key.code == KeyCode::Esc {
                app.set_notice(NoticeKind::Info, "The Fabric download is still running");
            }
        }
        CreateStep::Review => handle_review_key(key, app, service),
    }
}

fn handle_identity_key(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Esc => app.close_overlay(),
        KeyCode::Tab | KeyCode::BackTab => {
            let form = app.create_form_mut();
            form.field = match form.field {
                CreateField::Id => CreateField::Name,
                CreateField::Name => CreateField::Id,
            };
        }
        KeyCode::Backspace => match app.create_form().field {
            CreateField::Id => {
                app.create_form_mut().id.pop();
            }
            CreateField::Name => {
                app.create_form_mut().name.pop();
            }
        },
        KeyCode::Char(character) if !character.is_control() => {
            let form = app.create_form_mut();
            match form.field {
                CreateField::Id => form.id.push(character),
                CreateField::Name => form.name.push(character),
            }
        }
        KeyCode::Enter => {
            if let Err(message) = validate_identity(app) {
                app.set_notice(NoticeKind::Error, message);
            } else {
                app.create_form_mut().step = CreateStep::Runtime;
                app.set_notice(
                    NoticeKind::Info,
                    "Choose an installed or new Fabric runtime",
                );
            }
        }
        _ => {}
    }
}

fn validate_identity(app: &App) -> Result<(), String> {
    InstanceId::from_str(app.create_form().id.trim())
        .map_err(|error| format!("Invalid instance ID: {error}"))?;
    InstanceName::parse(&app.create_form().name).map_err(|error| error.to_string())?;
    Ok(())
}

fn handle_runtime_choice_key(
    key: KeyEvent,
    app: &mut App,
    service: &InstanceService,
    download_tx: &mpsc::Sender<DownloadEvent>,
) {
    match key.code {
        KeyCode::Esc => app.create_form_mut().step = CreateStep::Identity,
        KeyCode::Down | KeyCode::Char('j') => app.next_runtime_choice(),
        KeyCode::Up | KeyCode::Char('k') => app.previous_runtime_choice(),
        KeyCode::Enter => {
            let choice = app.create_form().runtime_choice;
            let installed_count = app.runtimes().len();
            if choice < installed_count {
                app.create_form_mut().runtime = Some(app.runtimes()[choice].clone());
                app.create_form_mut().step = CreateStep::Review;
            } else if choice == installed_count {
                begin_download(app, None, service, download_tx);
            } else {
                app.create_form_mut().step = CreateStep::MinecraftVersion;
            }
        }
        _ => {}
    }
}

fn handle_minecraft_version_key(
    key: KeyEvent,
    app: &mut App,
    service: &InstanceService,
    download_tx: &mpsc::Sender<DownloadEvent>,
) {
    match key.code {
        KeyCode::Esc => app.create_form_mut().step = CreateStep::Runtime,
        KeyCode::Backspace => {
            app.create_form_mut().minecraft_version.pop();
        }
        KeyCode::Char(character) if !character.is_control() => {
            app.create_form_mut().minecraft_version.push(character);
        }
        KeyCode::Enter => {
            let version = app.create_form().minecraft_version.trim().to_owned();
            if version.is_empty() {
                app.set_notice(NoticeKind::Error, "Enter a Minecraft version");
            } else {
                begin_download(app, Some(version), service, download_tx);
            }
        }
        _ => {}
    }
}

fn begin_download(
    app: &mut App,
    minecraft: Option<String>,
    service: &InstanceService,
    download_tx: &mpsc::Sender<DownloadEvent>,
) {
    app.create_form_mut().step = CreateStep::Downloading;
    app.create_form_mut().download = Some(DownloadStatus::Resolving {
        requested: minecraft.clone(),
    });
    app.set_notice(NoticeKind::Info, "Resolving Fabric versions");
    let service = service.clone();
    let download_tx = download_tx.clone();
    tokio::spawn(async move {
        let runtime = match service.resolve_runtime(minecraft.as_deref()).await {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = download_tx
                    .send(DownloadEvent::Finished(Err(error.to_string())))
                    .await;
                return;
            }
        };
        if download_tx
            .send(DownloadEvent::Resolved(runtime.clone()))
            .await
            .is_err()
        {
            return;
        }
        let result = service
            .cache_runtime(&runtime)
            .await
            .map(|()| runtime)
            .map_err(|error| error.to_string());
        let _ = download_tx.send(DownloadEvent::Finished(result)).await;
    });
}

fn handle_review_key(key: KeyEvent, app: &mut App, service: &InstanceService) {
    match key.code {
        KeyCode::Esc => app.create_form_mut().step = CreateStep::Runtime,
        KeyCode::Char(' ') => {
            let form = app.create_form_mut();
            form.eula_accepted = !form.eula_accepted;
            if form.eula_accepted {
                app.set_notice(
                    NoticeKind::Info,
                    "Minecraft EULA accepted for this instance",
                );
            }
        }
        KeyCode::Enter => create_instance(app, service),
        _ => {}
    }
}

fn create_instance(app: &mut App, service: &InstanceService) {
    if !app.create_form().eula_accepted {
        app.set_notice(
            NoticeKind::Error,
            "You must accept the Minecraft EULA before creating this instance",
        );
        return;
    }
    let id = match InstanceId::from_str(app.create_form().id.trim()) {
        Ok(id) => id,
        Err(error) => {
            app.set_notice(NoticeKind::Error, format!("Invalid instance ID: {error}"));
            return;
        }
    };
    let name = match InstanceName::parse(&app.create_form().name) {
        Ok(name) => name,
        Err(error) => {
            app.set_notice(NoticeKind::Error, error.to_string());
            return;
        }
    };
    let Some(runtime) = app.create_form().runtime.clone() else {
        app.set_notice(NoticeKind::Error, "Choose a Fabric runtime");
        return;
    };
    match service.create_with_cached_runtime(id, name, runtime.clone(), EulaAcceptance::Accepted) {
        Ok(_) => match service.list_instances() {
            Ok(instances) => {
                app.replace_instances(instances);
                app.close_overlay();
                app.set_notice(
                    NoticeKind::Success,
                    format!("Instance created with Minecraft {}", runtime.minecraft),
                );
            }
            Err(error) => app.set_notice(NoticeKind::Error, error.to_string()),
        },
        Err(error) => app.set_notice(NoticeKind::Error, error.to_string()),
    }
}

async fn begin_quit(app: &mut App, supervisor: &ServerSupervisor) {
    if app.is_quitting() {
        app.set_notice(NoticeKind::Info, "Forcing running instances to exit");
        if let Err(error) = supervisor.kill_all().await {
            app.set_notice(NoticeKind::Error, error.to_string());
        }
        return;
    }

    app.begin_quit();
    if let Err(error) = supervisor.stop_all().await {
        app.set_notice(NoticeKind::Error, error.to_string());
    }
}

fn draw(frame: &mut Frame<'_>, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(9),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

    draw_header(frame, app, areas[0]);
    match app.screen() {
        Screen::Console => draw_console(frame, app, areas[1]),
        Screen::Mods => draw_mods(frame, app, areas[1]),
        Screen::Runtimes => draw_runtimes(frame, app, areas[1]),
        _ => draw_dashboard(frame, app, areas[1]),
    }
    draw_notice(frame, app, areas[2]);
    draw_footer(frame, app, areas[3]);

    match app.screen() {
        Screen::Create => draw_create(frame, app),
        Screen::Help => draw_help(frame),
        _ => {}
    }
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            " DART ",
            Style::default().fg(Color::Black).bg(ACCENT).bold(),
        ),
        Span::raw("  Fabric instance manager"),
        Span::styled(
            format!(
                "  {} instances · {} running · {} runtimes ",
                app.instances().len(),
                app.running_count(),
                app.runtimes().len()
            ),
            Style::default().fg(MUTED),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(MUTED))
                .padding(Padding::top(1)),
        ),
        area,
    );
}

fn draw_dashboard(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let items = app.instances().iter().map(|instance| {
        let state = app.state(instance.id());
        let (marker, color) = state_marker(state, app.spinner());
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {marker} "), Style::default().fg(color).bold()),
            Span::styled(instance.config().name.to_string(), Style::default().bold()),
            Span::styled(format!("  {}", instance.id()), Style::default().fg(MUTED)),
        ]))
    });
    let list = List::new(items)
        .block(panel(" INSTANCES ", true))
        .highlight_style(Style::default().bg(Color::Rgb(40, 52, 65)).fg(Color::White))
        .highlight_symbol("▌");
    let mut state = ListState::default()
        .with_selected((!app.instances().is_empty()).then_some(app.selected_index()));
    frame.render_stateful_widget(list, columns[0], &mut state);

    let details = if let Some(instance) = app.selected() {
        let state = app.state(instance.id());
        let (marker, color) = state_marker(state, app.spinner());
        let recent = app
            .console_lines(instance.id())
            .rev()
            .take(4)
            .collect::<Vec<_>>();
        let mut lines = vec![
            Line::styled(
                instance.config().name.to_string(),
                Style::default().fg(Color::White).bold(),
            ),
            Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(color).bold()),
                Span::styled(state_label(state), Style::default().fg(color)),
            ]),
            Line::from(""),
            labeled("Minecraft", instance.config().fabric.minecraft.to_string()),
            labeled("Fabric loader", instance.config().fabric.loader.to_string()),
            labeled("Installer", instance.config().fabric.installer.to_string()),
            labeled(
                "Memory",
                format!(
                    "{}–{} MiB",
                    instance.config().launch.min_memory_mib,
                    instance.config().launch.max_memory_mib
                ),
            ),
            labeled("Directory", instance.root().display().to_string()),
            Line::from(""),
            Line::styled("Recent activity", Style::default().fg(ACCENT).bold()),
        ];
        if recent.is_empty() {
            lines.push(Line::styled(
                "No console output yet",
                Style::default().fg(MUTED),
            ));
        } else {
            for entry in recent.into_iter().rev() {
                lines.push(Line::styled(
                    entry.line.clone(),
                    Style::default().fg(match entry.stream {
                        OutputStream::Stdout => MUTED,
                        OutputStream::Stderr => BAD,
                    }),
                ));
            }
        }
        lines
    } else {
        vec![
            Line::styled("No instances yet", Style::default().fg(Color::White).bold()),
            Line::from(""),
            Line::styled("Press n to create one.", Style::default().fg(MUTED)),
            Line::styled(
                "Dart will download Fabric or reuse a cached launcher.",
                Style::default().fg(MUTED),
            ),
        ]
    };
    frame.render_widget(
        Paragraph::new(details)
            .wrap(Wrap { trim: false })
            .block(panel(" OVERVIEW ", false).padding(Padding::uniform(1))),
        columns[1],
    );
}

fn draw_mods(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(mods) = app.mods() else {
        draw_dashboard(frame, app, area);
        return;
    };
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);
    let installed_active = matches!(mods.tab(), ModsTab::Installed);
    let discover_active = matches!(mods.tab(), ModsTab::Discover);
    let mut tabs = [
        ContentKind::Mod,
        ContentKind::DataPack,
        ContentKind::ResourcePack,
    ]
    .into_iter()
    .flat_map(|kind| {
        let active = mods.kind() == kind;
        [
            Span::styled(
                format!(
                    " {} ",
                    if active {
                        kind.label().to_uppercase()
                    } else {
                        kind.label().to_lowercase()
                    }
                ),
                if active {
                    Style::default().fg(Color::Black).bg(ACCENT).bold()
                } else {
                    Style::default().fg(MUTED)
                },
            ),
            Span::raw(" "),
        ]
    })
    .collect::<Vec<_>>();
    tabs.push(Span::raw("  "));
    tabs.extend([
        Span::styled(
            format!(
                " {} ",
                if installed_active {
                    "INSTALLED"
                } else {
                    "installed"
                }
            ),
            if installed_active {
                Style::default().fg(Color::Black).bg(ACCENT).bold()
            } else {
                Style::default().fg(MUTED)
            },
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                " {} ",
                if discover_active {
                    "DISCOVER"
                } else {
                    "discover"
                }
            ),
            if discover_active {
                Style::default().fg(Color::Black).bg(ACCENT).bold()
            } else {
                Style::default().fg(MUTED)
            },
        ),
    ]);
    if let Some(label) = mods.phase().label() {
        tabs.push(Span::styled(
            format!("   {} {label}", app.spinner()),
            Style::default().fg(ACCENT).bold(),
        ));
    }
    let context = Line::from(vec![
        Span::styled(
            " CONTENT ",
            Style::default().fg(Color::Black).bg(ACCENT).bold(),
        ),
        Span::styled(
            format!(
                "  {} · Minecraft {} · Fabric · {} installed",
                mods.instance().config().name,
                mods.instance().config().fabric.minecraft,
                mods.installed().len()
            ),
            Style::default().fg(Color::White).bold(),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(vec![context, Line::from(tabs)]).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(ACCENT)),
        ),
        regions[0],
    );

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(regions[1]);
    match mods.tab() {
        ModsTab::Installed => draw_installed_content(frame, mods, columns[0]),
        ModsTab::Discover => draw_mod_discovery(frame, mods, columns[0]),
    }
    draw_mod_details(frame, mods, columns[1]);
    if let Some(managed) = mods.remove_confirmation() {
        draw_mod_removal_confirmation(frame, managed.title(), managed.kind());
    }
}

fn draw_installed_content(frame: &mut Frame<'_>, mods: &model::ModsView, area: Rect) {
    if mods.installed().is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    format!("No {} installed", mods.kind().label().to_lowercase()),
                    Style::default().fg(Color::White).bold(),
                ),
                Line::from(""),
                Line::styled(
                    format!(
                        "Press Tab to search Modrinth for a compatible {}.",
                        mods.kind().singular()
                    ),
                    Style::default().fg(MUTED),
                ),
            ])
            .wrap(Wrap { trim: false })
            .block(
                panel(
                    &format!(" INSTALLED {} ", mods.kind().label().to_uppercase()),
                    true,
                )
                .padding(Padding::uniform(1)),
            ),
            area,
        );
        return;
    }
    let items = mods.installed().iter().map(|content| match content {
        InstalledContent::Mod(InstalledMod::Managed(managed)) => ListItem::new(Line::from(vec![
            Span::styled("● ", Style::default().fg(GOOD)),
            Span::styled(managed.title.clone(), Style::default().bold()),
            Span::styled(
                format!("  {}", managed.version_number),
                Style::default().fg(MUTED),
            ),
        ])),
        InstalledContent::Mod(InstalledMod::External { filename }) => {
            ListItem::new(Line::from(vec![
                Span::styled("○ ", Style::default().fg(WARN)),
                Span::raw(filename.to_string()),
                Span::styled("  external", Style::default().fg(MUTED)),
            ]))
        }
        InstalledContent::DataPack(InstalledPack::Managed(managed))
        | InstalledContent::ResourcePack(InstalledPack::Managed(managed)) => {
            ListItem::new(Line::from(vec![
                Span::styled("● ", Style::default().fg(GOOD)),
                Span::styled(managed.title.clone(), Style::default().bold()),
                Span::styled(
                    format!("  {}", managed.version_number),
                    Style::default().fg(MUTED),
                ),
            ]))
        }
        InstalledContent::DataPack(InstalledPack::ExternalFile { filename })
        | InstalledContent::ResourcePack(InstalledPack::ExternalFile { filename }) => {
            ListItem::new(Line::from(vec![
                Span::styled("○ ", Style::default().fg(WARN)),
                Span::raw(filename.to_string()),
                Span::styled("  external", Style::default().fg(MUTED)),
            ]))
        }
        InstalledContent::DataPack(InstalledPack::ExternalResource { url })
        | InstalledContent::ResourcePack(InstalledPack::ExternalResource { url }) => {
            ListItem::new(Line::from(vec![
                Span::styled("○ ", Style::default().fg(WARN)),
                Span::raw(url.clone()),
                Span::styled("  external", Style::default().fg(MUTED)),
            ]))
        }
    });
    let panel_title = format!(" INSTALLED {} ", mods.kind().label().to_uppercase());
    let list = List::new(items)
        .block(panel(&panel_title, true))
        .highlight_style(Style::default().bg(Color::Rgb(40, 52, 65)).fg(Color::White))
        .highlight_symbol("▌");
    let mut state = ListState::default().with_selected(Some(mods.installed_selected()));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_mod_discovery(frame: &mut Frame<'_>, mods: &model::ModsView, area: Rect) {
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(area);
    draw_input(
        frame,
        mods.query(),
        regions[0],
        matches!(mods.focus(), ModsFocus::SearchInput),
    );
    if mods.results().is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Search Modrinth", Style::default().fg(Color::White).bold()),
                Line::from(""),
                Line::styled(
                    format!(
                        "Enter a {} name, then press Enter. Results match Minecraft {}.",
                        mods.kind().singular(),
                        mods.instance().config().fabric.minecraft
                    ),
                    Style::default().fg(MUTED),
                ),
            ])
            .wrap(Wrap { trim: false })
            .block(
                panel(
                    " COMPATIBLE RESULTS ",
                    matches!(mods.focus(), ModsFocus::SearchResults),
                )
                .padding(Padding::uniform(1)),
            ),
            regions[1],
        );
        return;
    }
    let items = mods.results().iter().map(|hit| {
        ListItem::new(Line::from(vec![
            Span::styled(hit.title().to_owned(), Style::default().bold()),
            Span::styled(format!("  by {}", hit.author()), Style::default().fg(MUTED)),
        ]))
    });
    let list = List::new(items)
        .block(panel(
            " COMPATIBLE RESULTS ",
            matches!(mods.focus(), ModsFocus::SearchResults),
        ))
        .highlight_style(Style::default().bg(Color::Rgb(40, 52, 65)).fg(Color::White))
        .highlight_symbol("▌");
    let mut state = ListState::default().with_selected(Some(mods.result_selected()));
    frame.render_stateful_widget(list, regions[1], &mut state);
}

fn draw_mod_details(frame: &mut Frame<'_>, mods: &model::ModsView, area: Rect) {
    let mut lines = match mods.tab() {
        ModsTab::Installed => match mods.selected_installed() {
            Some(InstalledContent::Mod(InstalledMod::Managed(managed))) => vec![
                Line::styled(
                    managed.title.clone(),
                    Style::default().fg(Color::White).bold(),
                ),
                Line::styled("Managed by Dart", Style::default().fg(GOOD)),
                Line::from(""),
                labeled("Version", managed.version_number.clone()),
                labeled("File", managed.filename.to_string()),
                labeled("Project", managed.project_id.to_string()),
                Line::from(""),
                Line::styled(
                    "u checks for a compatible update.",
                    Style::default().fg(MUTED),
                ),
                Line::styled(
                    "d asks before removing this file.",
                    Style::default().fg(MUTED),
                ),
            ],
            Some(InstalledContent::Mod(InstalledMod::External { filename })) => vec![
                Line::styled(
                    filename.to_string(),
                    Style::default().fg(Color::White).bold(),
                ),
                Line::styled("External JAR", Style::default().fg(WARN)),
                Line::from(""),
                Line::styled(
                    "Dart did not install this file, so it will not update or remove it.",
                    Style::default().fg(MUTED),
                ),
            ],
            Some(InstalledContent::DataPack(InstalledPack::Managed(managed))) => {
                managed_pack_details(managed, ContentKind::DataPack)
            }
            Some(InstalledContent::ResourcePack(InstalledPack::Managed(managed))) => {
                managed_pack_details(managed, ContentKind::ResourcePack)
            }
            Some(InstalledContent::DataPack(InstalledPack::ExternalFile { filename })) => vec![
                Line::styled(
                    filename.to_string(),
                    Style::default().fg(Color::White).bold(),
                ),
                Line::styled("External data pack", Style::default().fg(WARN)),
                Line::from(""),
                Line::styled(
                    "Dart did not install this ZIP, so it will not update or remove it.",
                    Style::default().fg(MUTED),
                ),
            ],
            Some(InstalledContent::ResourcePack(InstalledPack::ExternalResource { url })) => vec![
                Line::styled(
                    "External resource pack",
                    Style::default().fg(Color::White).bold(),
                ),
                Line::from(""),
                labeled("URL", url),
                Line::from(""),
                Line::styled(
                    "Dart will not replace this server.properties setting.",
                    Style::default().fg(MUTED),
                ),
            ],
            Some(InstalledContent::DataPack(_)) | Some(InstalledContent::ResourcePack(_)) => {
                vec![Line::styled(
                    "Unsupported external content entry",
                    Style::default().fg(WARN),
                )]
            }
            None => vec![Line::styled(
                format!(
                    "Choose Discover to install a compatible {}.",
                    mods.kind().singular()
                ),
                Style::default().fg(MUTED),
            )],
        },
        ModsTab::Discover => match mods.selected_result() {
            Some(hit) => vec![
                Line::styled(
                    hit.title().to_owned(),
                    Style::default().fg(Color::White).bold(),
                ),
                Line::styled(
                    format!("Compatible {} for this instance", hit.kind().singular()),
                    Style::default().fg(GOOD),
                ),
                Line::from(""),
                labeled("Author", hit.author()),
                labeled("Downloads", format_downloads(hit.downloads())),
                Line::from(""),
                Line::from(hit.description().to_owned()),
                Line::from(""),
                Line::styled(
                    "Press i or Enter to download, verify, and install the newest compatible release.",
                    Style::default().fg(MUTED),
                ),
            ],
            None => vec![
                Line::styled(
                    "Modrinth discovery",
                    Style::default().fg(Color::White).bold(),
                ),
                Line::from(""),
                Line::styled(
                    format!(
                        "Search returns {} projects for this instance's exact Minecraft version.",
                        mods.kind().label().to_lowercase()
                    ),
                    Style::default().fg(MUTED),
                ),
            ],
        },
    };
    if let Some(label) = mods.phase().label() {
        let action = match mods.phase() {
            ModsPhase::Searching => "Searching Modrinth",
            ModsPhase::Installing { .. } => "Resolving / installing",
            ModsPhase::Updating { .. } => "Resolving / updating",
            ModsPhase::Removing { .. } => "Removing",
            ModsPhase::Idle => "",
        };
        lines.splice(
            0..0,
            [
                Line::styled(
                    format!("{action}: {label}"),
                    Style::default().fg(ACCENT).bold(),
                ),
                Line::from(""),
            ],
        );
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" DETAILS ", false).padding(Padding::uniform(1))),
        area,
    );
}

fn managed_pack_details(
    managed: &crate::packs::ManagedPack,
    kind: ContentKind,
) -> Vec<Line<'static>> {
    vec![
        Line::styled(
            managed.title.clone(),
            Style::default().fg(Color::White).bold(),
        ),
        Line::styled("Managed by Dart", Style::default().fg(GOOD)),
        Line::from(""),
        labeled("Type", kind.singular()),
        labeled("Version", managed.version_number.clone()),
        labeled("File", managed.filename.to_string()),
        labeled("Project", managed.project_id.to_string()),
        Line::from(""),
        Line::styled(
            match kind {
                ContentKind::DataPack => {
                    "Installed in this world's datapacks directory. Use /reload or restart."
                }
                ContentKind::ResourcePack => {
                    "Configured for joining clients through server.properties."
                }
                ContentKind::Mod => "",
            },
            Style::default().fg(MUTED),
        ),
        Line::styled(
            "u checks for an update. d asks before removal.",
            Style::default().fg(MUTED),
        ),
    ]
}

fn draw_mod_removal_confirmation(frame: &mut Frame<'_>, title: &str, kind: ContentKind) {
    let area = centered_rect(54, 25, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                format!("Remove Dart-managed {}?", kind.singular()),
                Style::default().fg(BAD).bold(),
            ),
            Line::from(""),
            Line::from(title.to_owned()),
            Line::from(""),
            Line::styled(
                match kind {
                    ContentKind::Mod => "This removes its JAR from this instance only.",
                    ContentKind::DataPack => "This removes its ZIP from this world only.",
                    ContentKind::ResourcePack => {
                        "This clears the managed server resource-pack setting."
                    }
                },
                Style::default().fg(MUTED),
            ),
            Line::styled(
                "Press y to remove or n to keep it.",
                Style::default().fg(MUTED),
            ),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(format!(" REMOVE {} ", kind.label().to_uppercase()))
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BAD))
                .padding(Padding::uniform(1)),
        ),
        area,
    );
}

fn format_downloads(downloads: u64) -> String {
    if downloads >= 1_000_000 {
        format!("{:.1}M", downloads as f64 / 1_000_000.0)
    } else if downloads >= 1_000 {
        format!("{:.1}K", downloads as f64 / 1_000.0)
    } else {
        downloads.to_string()
    }
}

fn draw_runtimes(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rows = app.runtimes().iter().map(|runtime| {
        Row::new(vec![
            runtime.minecraft.to_string(),
            runtime.loader.to_string(),
            runtime.installer.to_string(),
            "cached".to_owned(),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(28),
            Constraint::Percentage(28),
            Constraint::Percentage(28),
            Constraint::Percentage(16),
        ],
    )
    .header(
        Row::new(vec!["Minecraft", "Loader", "Installer", "State"])
            .style(Style::default().fg(ACCENT).bold()),
    )
    .block(panel(" FABRIC RUNTIMES ", true).padding(Padding::uniform(1)))
    .row_highlight_style(Style::default().bg(Color::Rgb(40, 52, 65)))
    .highlight_symbol("▌");
    let mut state = TableState::default()
        .with_selected((!app.runtimes().is_empty()).then_some(app.selected_runtime_index()));
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_console(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(3)])
        .split(area);
    let Some(instance) = app.selected() else {
        return;
    };

    let visible_lines = areas[0].height.saturating_sub(2) as usize;
    let all_lines: Vec<_> = app.console_lines(instance.id()).collect();
    let start = all_lines.len().saturating_sub(visible_lines);
    let lines = all_lines[start..]
        .iter()
        .map(|entry| {
            let style = match entry.stream {
                OutputStream::Stdout => Style::default(),
                OutputStream::Stderr => Style::default().fg(BAD),
            };
            Line::styled(entry.line.clone(), style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(
                &format!(" CONSOLE · {} ", instance.config().name),
                true,
            )),
        areas[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(ACCENT).bold()),
            Span::raw(app.console_input()),
        ]))
        .block(panel(" COMMAND ", true)),
        areas[1],
    );
    let cursor_x = areas[1].x + 3 + app.console_input().chars().count() as u16;
    frame.set_cursor_position((
        cursor_x.min(areas[1].right().saturating_sub(2)),
        areas[1].y + 1,
    ));
}

fn draw_notice(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (icon, color) = match app.notice().kind {
        NoticeKind::Info => ("•", ACCENT),
        NoticeKind::Success => ("✓", GOOD),
        NoticeKind::Error => ("!", BAD),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {icon} "), Style::default().fg(color).bold()),
            Span::styled(&app.notice().message, Style::default().fg(color)),
        ])),
        area,
    );
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let hints = match app.screen() {
        Screen::Dashboard => vec![
            ("↑↓", "Select"),
            ("n", "New"),
            ("s", "Start"),
            ("x", "Stop"),
            ("Enter", "Console"),
            ("m", "Content"),
            ("v", "Runtimes"),
            ("?", "Help"),
            ("q", "Quit"),
        ],
        Screen::Console => vec![("Enter", "Send"), ("Esc", "Dashboard"), ("Ctrl-C", "Quit")],
        Screen::Runtimes => vec![
            ("↑↓", "Select"),
            ("n", "New instance"),
            ("Esc", "Dashboard"),
        ],
        Screen::Create => create_hints(app.create_form()),
        Screen::Mods => mods_hints(app),
        Screen::Help => vec![("Any key", "Close")],
    };
    let mut spans = vec![Span::raw(" ")];
    for (key, label) in hints {
        spans.push(Span::styled(
            format!(" {key} "),
            Style::default().fg(Color::Black).bg(MUTED).bold(),
        ));
        spans.push(Span::styled(
            format!(" {label}  "),
            Style::default().fg(MUTED),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::Rgb(55, 65, 75))),
        ),
        area,
    );
}

fn mods_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let Some(mods) = app.mods() else {
        return vec![("Esc", "Dashboard")];
    };
    if mods.phase().is_busy() {
        return vec![("Esc", "Operation continues")];
    }
    if mods.remove_confirmation().is_some() {
        return vec![("y", "Remove"), ("n/Esc", "Keep content")];
    }
    match (mods.tab(), mods.focus()) {
        (ModsTab::Installed, _) => vec![
            ("←→", "Content type"),
            ("↑↓", "Select"),
            ("u", "Update"),
            ("d", "Remove"),
            ("r", "Reload"),
            ("Tab", "Discover"),
            ("Esc", "Dashboard"),
        ],
        (ModsTab::Discover, ModsFocus::SearchInput) => vec![
            ("←→", "Content type"),
            ("Enter", "Search"),
            ("Tab", "Installed"),
            ("Esc", "Dashboard"),
        ],
        (ModsTab::Discover, ModsFocus::SearchResults) => vec![
            ("←→", "Content type"),
            ("↑↓", "Select"),
            ("i/Enter", "Install"),
            ("/", "Search"),
            ("Tab", "Installed"),
            ("Esc", "Dashboard"),
        ],
        (ModsTab::Discover, ModsFocus::Installed) => vec![("Esc", "Dashboard")],
    }
}

fn create_hints(form: &model::CreateForm) -> Vec<(&'static str, &'static str)> {
    match form.step {
        CreateStep::Identity => vec![
            ("Tab", "Next field"),
            ("Enter", "Continue"),
            ("Esc", "Cancel"),
        ],
        CreateStep::Runtime => vec![("↑↓", "Choose"), ("Enter", "Select"), ("Esc", "Back")],
        CreateStep::MinecraftVersion => vec![("Enter", "Download"), ("Esc", "Back")],
        CreateStep::Downloading if matches!(form.download, Some(DownloadStatus::Failed { .. })) => {
            vec![("Enter/Esc", "Back to runtimes")]
        }
        CreateStep::Downloading => vec![("Please wait", "Downloading")],
        CreateStep::Review => vec![
            ("Space", "Accept EULA"),
            ("Enter", "Create"),
            ("Esc", "Back"),
        ],
    }
}

fn draw_create(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(78, 68, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" NEW FABRIC INSTANCE ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .padding(Padding::uniform(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(5)])
        .split(inner);
    let steps = [
        ("1", "Identity", CreateStep::Identity),
        ("2", "Runtime", CreateStep::Runtime),
        ("3", "Review", CreateStep::Review),
    ];
    let mut breadcrumb = Vec::new();
    for (index, (number, label, step)) in steps.iter().enumerate() {
        let active = step_is_active(app.create_form().step, *step);
        breadcrumb.push(Span::styled(
            format!(" {number} {label} "),
            if active {
                Style::default().fg(Color::Black).bg(ACCENT).bold()
            } else {
                Style::default().fg(MUTED)
            },
        ));
        if index < steps.len() - 1 {
            breadcrumb.push(Span::styled(" ─ ", Style::default().fg(MUTED)));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(breadcrumb)), regions[0]);

    match app.create_form().step {
        CreateStep::Identity => draw_identity_step(frame, app, regions[1]),
        CreateStep::Runtime => draw_runtime_step(frame, app, regions[1]),
        CreateStep::MinecraftVersion => draw_version_step(frame, app, regions[1]),
        CreateStep::Downloading => draw_downloading_step(frame, app, regions[1]),
        CreateStep::Review => draw_review_step(frame, app, regions[1]),
    }
}

fn draw_identity_step(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let fields = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new("Instance ID").style(Style::default().fg(MUTED)),
        fields[0],
    );
    draw_input(
        frame,
        &app.create_form().id,
        fields[1],
        app.create_form().field == CreateField::Id,
    );
    frame.render_widget(
        Paragraph::new("lowercase letters, digits, and hyphens").style(Style::default().fg(MUTED)),
        fields[2],
    );
    frame.render_widget(
        Paragraph::new("Display name").style(Style::default().fg(MUTED)),
        fields[3],
    );
    draw_input(
        frame,
        &app.create_form().name,
        fields[4],
        app.create_form().field == CreateField::Name,
    );
}

fn draw_input(frame: &mut Frame<'_>, value: &str, area: Rect, focused: bool) {
    let color = if focused { ACCENT } else { MUTED };
    frame.render_widget(
        Paragraph::new(value).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(color)),
        ),
        area,
    );
    if focused {
        let x = area.x + 1 + value.chars().count() as u16;
        frame.set_cursor_position((x.min(area.right().saturating_sub(2)), area.y + 1));
    }
}

fn draw_runtime_step(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut items = app
        .runtimes()
        .iter()
        .map(|runtime| {
            ListItem::new(Line::from(vec![
                Span::styled("● ", Style::default().fg(GOOD)),
                Span::raw(runtime.label()),
                Span::styled("  cached", Style::default().fg(MUTED)),
            ]))
        })
        .collect::<Vec<_>>();
    items.push(ListItem::new(Line::from(vec![
        Span::styled("↓ ", Style::default().fg(ACCENT)),
        Span::styled("Download latest stable", Style::default().bold()),
    ])));
    items.push(ListItem::new(Line::from(vec![
        Span::styled("↓ ", Style::default().fg(ACCENT)),
        Span::styled("Download a Minecraft version…", Style::default().bold()),
    ])));
    let list = List::new(items)
        .block(
            Block::default()
                .title(" Choose a runtime ")
                .borders(Borders::NONE),
        )
        .highlight_style(Style::default().bg(Color::Rgb(40, 52, 65)))
        .highlight_symbol("▌");
    let mut state = ListState::default().with_selected(Some(app.create_form().runtime_choice));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_version_step(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(
            "Minecraft version\nDart selects the newest compatible stable loader and installer.",
        ),
        regions[0],
    );
    draw_input(
        frame,
        &app.create_form().minecraft_version,
        regions[1],
        true,
    );
}

fn draw_downloading_step(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let lines = match app.create_form().download.as_ref() {
        Some(DownloadStatus::Resolving { requested }) => vec![
            Line::from(""),
            activity_line(app.spinner(), "Resolving Fabric versions"),
            Line::styled(
                requested.as_deref().map_or_else(
                    || "Latest stable Minecraft".to_owned(),
                    |version| format!("Minecraft {version}"),
                ),
                Style::default().fg(MUTED),
            ),
            Line::from(""),
            Line::styled(
                "Checking compatible loader and installer versions…",
                Style::default().fg(MUTED),
            ),
        ],
        Some(DownloadStatus::Downloading { runtime }) => vec![
            Line::from(""),
            activity_line(app.spinner(), "Downloading Fabric launcher"),
            Line::styled(runtime.label(), Style::default().fg(MUTED)),
            Line::from(""),
            Line::styled(
                "Saving the launcher in Dart's cache…",
                Style::default().fg(MUTED),
            ),
        ],
        Some(DownloadStatus::Failed { message }) => vec![
            Line::from(""),
            Line::styled("!  Fabric download failed", Style::default().fg(BAD).bold()),
            Line::from(""),
            Line::styled(message.clone(), Style::default().fg(BAD)),
            Line::from(""),
            Line::styled(
                "Press Enter or Esc to choose another runtime.",
                Style::default().fg(MUTED),
            ),
        ],
        None => vec![Line::styled(
            "Preparing Fabric download…",
            Style::default().fg(MUTED),
        )],
    };
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

fn activity_line(spinner: &str, label: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{spinner}  "), Style::default().fg(ACCENT).bold()),
        Span::styled(label, Style::default().bold()),
    ])
}

fn draw_review_step(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let form = app.create_form();
    let runtime = form.runtime.as_ref();
    let lines = vec![
        Line::styled("Ready to create", Style::default().fg(GOOD).bold()),
        Line::from(""),
        labeled("ID", form.id.trim()),
        labeled("Name", form.name.trim()),
        labeled(
            "Minecraft",
            runtime.map_or("—", |runtime| runtime.minecraft.as_str()),
        ),
        labeled(
            "Fabric loader",
            runtime.map_or("—", |runtime| runtime.loader.as_str()),
        ),
        labeled(
            "Installer",
            runtime.map_or("—", |runtime| runtime.installer.as_str()),
        ),
        Line::from(""),
        Line::styled(
            format!("A private copy of {FABRIC_LAUNCHER_FILE} will be created."),
            Style::default().fg(MUTED),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if form.eula_accepted { "[x] " } else { "[ ] " },
                Style::default()
                    .fg(if form.eula_accepted { GOOD } else { WARN })
                    .bold(),
            ),
            Span::styled(
                "I have read and accept the Minecraft EULA.",
                Style::default().fg(Color::White).bold(),
            ),
        ]),
        Line::styled(
            "Required. Press Space to toggle.",
            Style::default().fg(MUTED),
        ),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_help(frame: &mut Frame<'_>) {
    let area = centered_rect(68, 70, frame.area());
    frame.render_widget(Clear, area);
    let content = vec![
        Line::styled("Navigate", Style::default().fg(ACCENT).bold()),
        Line::from("  ↑/↓ or j/k   Select an instance or runtime"),
        Line::from("  Enter         Open the selected instance console"),
        Line::from(""),
        Line::styled("Instances", Style::default().fg(ACCENT).bold()),
        Line::from("  n             Create an instance"),
        Line::from("  s / x         Start / stop the selected instance"),
        Line::from("  m             Open mods, data packs, and resource packs"),
        Line::from("  r             Reload instance files"),
        Line::from("  v             View cached Fabric runtimes"),
        Line::from(""),
        Line::styled("Global", Style::default().fg(ACCENT).bold()),
        Line::from("  ?             Open this help"),
        Line::from("  q / Ctrl-C    Stop servers and quit"),
        Line::from("  Esc           Return to the previous screen"),
    ];
    frame.render_widget(
        Paragraph::new(content).block(
            Block::default()
                .title(" HELP ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACCENT))
                .padding(Padding::uniform(1)),
        ),
        area,
    );
}

fn panel(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { ACCENT } else { MUTED }))
}

fn labeled(label: &str, value: impl Into<String>) -> Line<'_> {
    Line::from(vec![
        Span::styled(format!("{label:<15}"), Style::default().fg(MUTED)),
        Span::raw(value.into()),
    ])
}

fn state_marker(state: &InstanceState, spinner: &str) -> (String, Color) {
    match state {
        InstanceState::Stopped => ("○".to_owned(), MUTED),
        InstanceState::Starting | InstanceState::Stopping => (spinner.to_owned(), WARN),
        InstanceState::Running { .. } => ("●".to_owned(), GOOD),
        InstanceState::Failed { .. } => ("!".to_owned(), BAD),
    }
}

fn step_is_active(current: CreateStep, breadcrumb: CreateStep) -> bool {
    match breadcrumb {
        CreateStep::Identity => current == CreateStep::Identity,
        CreateStep::Runtime => matches!(
            current,
            CreateStep::Runtime | CreateStep::MinecraftVersion | CreateStep::Downloading
        ),
        CreateStep::Review => current == CreateStep::Review,
        _ => false,
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::model::{App, CreateStep, DownloadStatus, Screen};
    use super::{create_instance, draw, handle_mods_key, handle_review_key};
    use crate::content::{ContentKind, ContentManager};
    use crate::instance::{FabricLaunch, Instance, InstanceConfig, InstanceId, InstanceName};
    use crate::instance::{InstanceService, InstanceStore};
    use crate::mods::{ModManager, ModStore, ModrinthClient};
    use crate::packs::PackManager;
    use crate::runtime::FabricRuntime;
    use crate::runtime::{FabricClient, RuntimeStore};
    use crate::storage::DartPaths;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;
    use std::str::FromStr;
    use tokio::sync::mpsc;

    fn instance() -> Instance {
        Instance::new(
            InstanceId::from_str("survival").unwrap(),
            PathBuf::from("/tmp/dart-mod-ui-test"),
            InstanceConfig::new(
                InstanceName::parse("Survival").unwrap(),
                FabricLaunch::default(),
                FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap(),
            ),
        )
    }

    #[test]
    fn empty_dashboard_explains_the_next_action() {
        let backend = TestBackend::new(110, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = App::new(Vec::new(), Vec::new());

        terminal.draw(|frame| draw(frame, &app)).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("DART"));
        assert!(rendered.contains("No instances yet"));
        assert!(rendered.contains("Runtimes"));
    }

    #[test]
    fn download_and_failure_states_are_visible() {
        let backend = TestBackend::new(110, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(Vec::new(), Vec::new());
        app.open_create();
        app.create_form_mut().step = CreateStep::Downloading;
        app.create_form_mut().download = Some(DownloadStatus::Resolving {
            requested: Some("1.21.8".to_owned()),
        });

        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Resolving Fabric versions"));
        assert!(rendered.contains("Minecraft 1.21.8"), "{rendered}");

        app.download_failed("network unavailable".to_owned());
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Fabric download failed"));
        assert!(rendered.contains("network unavailable"));
    }

    #[test]
    fn review_requires_explicit_eula_acceptance() {
        let mut app = App::new(Vec::new(), Vec::new());
        app.open_create();
        app.create_form_mut().step = CreateStep::Review;
        let backend = TestBackend::new(110, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("[ ]"));
        assert!(rendered.contains("I have read and accept the Minecraft EULA"));

        let data = DartPaths::new(PathBuf::from("/tmp/dart-eula-gate-test-unused"));
        let service = InstanceService::new(
            InstanceStore::new(data.clone()),
            RuntimeStore::new(data),
            FabricClient::new().unwrap(),
        );

        create_instance(&mut app, &service);
        assert!(app.notice().message.contains("must accept"));
        assert!(!app.create_form().eula_accepted);

        handle_review_key(
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
            &mut app,
            &service,
        );
        assert!(app.create_form().eula_accepted);

        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("[x]"));
    }

    #[test]
    fn mods_workspace_keeps_the_instance_compatibility_context_visible() {
        let backend = TestBackend::new(110, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(vec![instance()], Vec::new());
        app.open_mods(Vec::new());
        app.mods_switch_tab();
        app.push_mod_query_character('l');

        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("MODS"));
        assert!(rendered.contains("data packs"));
        assert!(rendered.contains("resource pack"));
        assert!(rendered.contains("Minecraft 1.21.8"));
        assert!(rendered.contains("Search Modrinth"));
    }

    #[test]
    fn mod_search_input_accepts_navigation_and_shortcut_letters() {
        let mut app = App::new(vec![instance()], Vec::new());
        app.open_mods(Vec::new());
        app.mods_switch_tab();
        let manager = ContentManager::new(
            ModManager::new(ModrinthClient::new().unwrap(), ModStore::new()),
            PackManager::new().unwrap(),
        );
        let (mod_tx, _mod_rx) = mpsc::channel(1);

        for character in ['h', 'j', 'k', 'l', 'm', '/'] {
            handle_mods_key(
                KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
                &mut app,
                &manager,
                &mod_tx,
            );
        }

        assert_eq!(app.screen(), Screen::Mods);
        assert_eq!(app.mods().unwrap().query(), "hjklm/");
    }

    #[test]
    fn content_workspace_switches_between_all_content_kinds() {
        let mut app = App::new(vec![instance()], Vec::new());
        app.open_mods(Vec::new());
        let manager = ContentManager::new(
            ModManager::new(ModrinthClient::new().unwrap(), ModStore::new()),
            PackManager::new().unwrap(),
        );
        let (mod_tx, _mod_rx) = mpsc::channel(1);

        handle_mods_key(
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            &mut app,
            &manager,
            &mod_tx,
        );
        assert_eq!(app.mods().unwrap().kind(), ContentKind::DataPack);
        handle_mods_key(
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            &mut app,
            &manager,
            &mod_tx,
        );
        assert_eq!(app.mods().unwrap().kind(), ContentKind::ResourcePack);
        handle_mods_key(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &mut app,
            &manager,
            &mod_tx,
        );
        assert_eq!(app.mods().unwrap().kind(), ContentKind::DataPack);
    }
}
