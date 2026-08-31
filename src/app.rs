use crate::content::{ContentKind, ContentSearchHit, InstalledContent, ManagedContent};
use crate::instance::{Instance, InstanceId, InstanceState};
use crate::runtime::FabricRuntime;
use crate::supervisor::{OutputStream, ServerEvent};
use std::collections::{HashMap, VecDeque};

const MAX_CONSOLE_LINES: usize = 2_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    Dashboard,
    Console,
    Create,
    Mods,
    Runtimes,
    Help,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ModsTab {
    #[default]
    Installed,
    Discover,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModsFocus {
    Installed,
    SearchInput,
    SearchResults,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModsPhase {
    Idle,
    Searching,
    Installing { title: String },
    Updating { title: String },
    Removing { title: String },
}

impl ModsPhase {
    pub fn is_busy(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Idle => None,
            Self::Searching => Some("Searching Modrinth"),
            Self::Installing { title } => Some(title),
            Self::Updating { title } => Some(title),
            Self::Removing { title } => Some(title),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModsView {
    instance: Instance,
    kind: ContentKind,
    tab: ModsTab,
    focus: ModsFocus,
    installed: Vec<InstalledContent>,
    installed_selected: usize,
    query: String,
    results: Vec<ContentSearchHit>,
    result_selected: usize,
    phase: ModsPhase,
    remove_confirmation: Option<ManagedContent>,
}

impl ModsView {
    fn new(instance: Instance, installed: Vec<InstalledContent>) -> Self {
        Self {
            instance,
            kind: ContentKind::Mod,
            tab: ModsTab::Installed,
            focus: ModsFocus::Installed,
            installed,
            installed_selected: 0,
            query: String::new(),
            results: Vec::new(),
            result_selected: 0,
            phase: ModsPhase::Idle,
            remove_confirmation: None,
        }
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn kind(&self) -> ContentKind {
        self.kind
    }

    pub fn tab(&self) -> ModsTab {
        self.tab
    }

    pub fn focus(&self) -> ModsFocus {
        self.focus
    }

    pub fn installed(&self) -> &[InstalledContent] {
        &self.installed
    }

    pub fn installed_selected(&self) -> usize {
        self.installed_selected
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &[ContentSearchHit] {
        &self.results
    }

    pub fn result_selected(&self) -> usize {
        self.result_selected
    }

    pub fn phase(&self) -> &ModsPhase {
        &self.phase
    }

    pub fn remove_confirmation(&self) -> Option<&ManagedContent> {
        self.remove_confirmation.as_ref()
    }

    pub fn selected_installed(&self) -> Option<&InstalledContent> {
        self.installed.get(self.installed_selected)
    }

    pub fn selected_result(&self) -> Option<&ContentSearchHit> {
        self.results.get(self.result_selected)
    }

    pub fn replace_installed(&mut self, installed: Vec<InstalledContent>) {
        let selected_key = self
            .selected_installed()
            .map(InstalledContent::selection_key);
        self.installed = installed;
        self.installed_selected = selected_key
            .and_then(|key| {
                self.installed
                    .iter()
                    .position(|content| content.selection_key() == key)
            })
            .unwrap_or(0)
            .min(self.installed.len().saturating_sub(1));
    }

    fn select_next(&mut self) {
        match self.focus {
            ModsFocus::Installed if !self.installed.is_empty() => {
                self.installed_selected = (self.installed_selected + 1) % self.installed.len();
            }
            ModsFocus::SearchResults if !self.results.is_empty() => {
                self.result_selected = (self.result_selected + 1) % self.results.len();
            }
            _ => {}
        }
    }

    fn select_previous(&mut self) {
        match self.focus {
            ModsFocus::Installed if !self.installed.is_empty() => {
                self.installed_selected = self
                    .installed_selected
                    .checked_sub(1)
                    .unwrap_or(self.installed.len() - 1);
            }
            ModsFocus::SearchResults if !self.results.is_empty() => {
                self.result_selected = self
                    .result_selected
                    .checked_sub(1)
                    .unwrap_or(self.results.len() - 1);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CreateField {
    #[default]
    Id,
    Name,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CreateStep {
    #[default]
    Identity,
    Runtime,
    MinecraftVersion,
    Downloading,
    Review,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DownloadStatus {
    Resolving { requested: Option<String> },
    Downloading { runtime: FabricRuntime },
    Failed { message: String },
}

#[derive(Debug, Default)]
pub struct CreateForm {
    pub id: String,
    pub name: String,
    pub field: CreateField,
    pub step: CreateStep,
    pub runtime_choice: usize,
    pub minecraft_version: String,
    pub runtime: Option<FabricRuntime>,
    pub download: Option<DownloadStatus>,
    pub eula_accepted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoticeKind {
    Info,
    Success,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleEntry {
    pub stream: OutputStream,
    pub line: String,
}

pub struct App {
    instances: Vec<Instance>,
    selected: usize,
    states: HashMap<InstanceId, InstanceState>,
    console: HashMap<InstanceId, VecDeque<ConsoleEntry>>,
    runtimes: Vec<FabricRuntime>,
    selected_runtime: usize,
    screen: Screen,
    mods: Option<ModsView>,
    console_input: String,
    create_form: CreateForm,
    notice: Notice,
    quitting: bool,
    animation_tick: u64,
}

impl App {
    pub fn new(instances: Vec<Instance>, runtimes: Vec<FabricRuntime>) -> Self {
        let states = instances
            .iter()
            .map(|instance| (instance.id().clone(), InstanceState::Stopped))
            .collect();
        Self {
            instances,
            selected: 0,
            states,
            console: HashMap::new(),
            runtimes,
            selected_runtime: 0,
            screen: Screen::Dashboard,
            mods: None,
            console_input: String::new(),
            create_form: CreateForm::default(),
            notice: Notice {
                kind: NoticeKind::Success,
                message: "Ready".to_owned(),
            },
            quitting: false,
            animation_tick: 0,
        }
    }

    pub fn tick(&mut self) {
        self.animation_tick = self.animation_tick.wrapping_add(1);
    }

    pub fn spinner(&self) -> &'static str {
        const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        FRAMES[(self.animation_tick as usize / 2) % FRAMES.len()]
    }

    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected(&self) -> Option<&Instance> {
        self.instances.get(self.selected)
    }

    pub fn state(&self, id: &InstanceId) -> &InstanceState {
        self.states.get(id).unwrap_or(&InstanceState::Stopped)
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn notice(&self) -> &Notice {
        &self.notice
    }

    pub fn set_notice(&mut self, kind: NoticeKind, message: impl Into<String>) {
        self.notice = Notice {
            kind,
            message: message.into(),
        };
    }

    pub fn console_input(&self) -> &str {
        &self.console_input
    }

    pub fn console_lines(&self, id: &InstanceId) -> impl DoubleEndedIterator<Item = &ConsoleEntry> {
        self.console.get(id).into_iter().flatten()
    }

    pub fn runtimes(&self) -> &[FabricRuntime] {
        &self.runtimes
    }

    pub fn selected_runtime_index(&self) -> usize {
        self.selected_runtime
    }

    pub fn create_form(&self) -> &CreateForm {
        &self.create_form
    }

    pub fn create_form_mut(&mut self) -> &mut CreateForm {
        &mut self.create_form
    }

    pub fn select_next(&mut self) {
        if !self.instances.is_empty() {
            self.selected = (self.selected + 1) % self.instances.len();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.instances.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.instances.len() - 1);
        }
    }

    pub fn select_next_runtime(&mut self) {
        if !self.runtimes.is_empty() {
            self.selected_runtime = (self.selected_runtime + 1) % self.runtimes.len();
        }
    }

    pub fn select_previous_runtime(&mut self) {
        if !self.runtimes.is_empty() {
            self.selected_runtime = self
                .selected_runtime
                .checked_sub(1)
                .unwrap_or(self.runtimes.len() - 1);
        }
    }

    pub fn next_runtime_choice(&mut self) {
        let count = self.runtimes.len() + 2;
        self.create_form.runtime_choice = (self.create_form.runtime_choice + 1) % count;
    }

    pub fn previous_runtime_choice(&mut self) {
        let count = self.runtimes.len() + 2;
        self.create_form.runtime_choice = self
            .create_form
            .runtime_choice
            .checked_sub(1)
            .unwrap_or(count - 1);
    }

    pub fn open_console(&mut self) {
        if self.selected().is_some() {
            self.screen = Screen::Console;
            self.console_input.clear();
        } else {
            self.set_notice(NoticeKind::Info, "Create an instance first");
        }
    }

    pub fn open_create(&mut self) {
        self.screen = Screen::Create;
        self.create_form = CreateForm::default();
    }

    pub fn open_runtimes(&mut self) {
        self.screen = Screen::Runtimes;
        self.selected_runtime = self
            .selected_runtime
            .min(self.runtimes.len().saturating_sub(1));
    }

    pub fn open_mods(&mut self, installed: Vec<InstalledContent>) {
        let Some(instance) = self.selected().cloned() else {
            self.set_notice(NoticeKind::Info, "Create an instance first");
            return;
        };
        self.mods = Some(ModsView::new(instance, installed));
        self.screen = Screen::Mods;
    }

    pub fn open_help(&mut self) {
        self.screen = Screen::Help;
    }

    pub fn close_overlay(&mut self) {
        self.screen = Screen::Dashboard;
        self.console_input.clear();
    }

    pub fn mods(&self) -> Option<&ModsView> {
        self.mods.as_ref()
    }

    pub fn mods_mut(&mut self) -> Option<&mut ModsView> {
        self.mods.as_mut()
    }

    pub fn mods_select_next(&mut self) {
        if let Some(mods) = self.mods_mut() {
            mods.select_next();
        }
    }

    pub fn mods_select_previous(&mut self) {
        if let Some(mods) = self.mods_mut() {
            mods.select_previous();
        }
    }

    pub fn mods_switch_tab(&mut self) {
        if let Some(mods) = self.mods_mut() {
            mods.tab = match mods.tab {
                ModsTab::Installed => ModsTab::Discover,
                ModsTab::Discover => ModsTab::Installed,
            };
            mods.focus = match mods.tab {
                ModsTab::Installed => ModsFocus::Installed,
                ModsTab::Discover if mods.results.is_empty() => ModsFocus::SearchInput,
                ModsTab::Discover => ModsFocus::SearchResults,
            };
        }
    }

    pub fn replace_content_kind(&mut self, kind: ContentKind, installed: Vec<InstalledContent>) {
        let Some(mods) = self.mods_mut() else {
            return;
        };
        mods.kind = kind;
        mods.installed = installed;
        mods.installed_selected = 0;
        mods.query.clear();
        mods.results.clear();
        mods.result_selected = 0;
        mods.remove_confirmation = None;
        mods.phase = ModsPhase::Idle;
        mods.focus = match mods.tab {
            ModsTab::Installed => ModsFocus::Installed,
            ModsTab::Discover => ModsFocus::SearchInput,
        };
    }

    pub fn mods_focus_search(&mut self) {
        if let Some(mods) = self.mods_mut() {
            mods.tab = ModsTab::Discover;
            mods.focus = ModsFocus::SearchInput;
        }
    }

    pub fn push_mod_query_character(&mut self, character: char) {
        if !character.is_control()
            && let Some(mods) = self.mods_mut()
        {
            mods.query.push(character);
        }
    }

    pub fn pop_mod_query_character(&mut self) {
        if let Some(mods) = self.mods_mut() {
            mods.query.pop();
        }
    }

    pub fn begin_mod_search(&mut self) -> Option<(Instance, String)> {
        let mods = self.mods_mut()?;
        if mods.query.trim().is_empty() {
            return None;
        }
        mods.phase = ModsPhase::Searching;
        Some((mods.instance.clone(), mods.query.trim().to_owned()))
    }

    pub fn finish_mod_search(&mut self, query: &str, results: Vec<ContentSearchHit>) {
        let Some(mods) = self.mods_mut() else {
            return;
        };
        if mods.query.trim() != query {
            return;
        }
        mods.results = results;
        mods.result_selected = 0;
        mods.focus = if mods.results.is_empty() {
            ModsFocus::SearchInput
        } else {
            ModsFocus::SearchResults
        };
        mods.phase = ModsPhase::Idle;
    }

    pub fn finish_mod_operation(&mut self, installed: Vec<InstalledContent>) {
        if let Some(mods) = self.mods_mut() {
            mods.replace_installed(installed);
            mods.tab = ModsTab::Installed;
            mods.focus = ModsFocus::Installed;
            mods.phase = ModsPhase::Idle;
            mods.remove_confirmation = None;
        }
    }

    pub fn cancel_mod_operation(&mut self) {
        if let Some(mods) = self.mods_mut() {
            mods.phase = ModsPhase::Idle;
        }
    }

    pub fn update_mod_operation(&mut self, message: String) {
        let Some(mods) = self.mods_mut() else {
            return;
        };
        match &mut mods.phase {
            ModsPhase::Installing { title } | ModsPhase::Updating { title } => *title = message,
            ModsPhase::Idle | ModsPhase::Searching | ModsPhase::Removing { .. } => {}
        }
    }

    pub fn begin_mod_install(&mut self) -> Option<(Instance, ContentSearchHit)> {
        let mods = self.mods_mut()?;
        let result = mods.selected_result()?.clone();
        mods.phase = ModsPhase::Installing {
            title: result.title().to_owned(),
        };
        Some((mods.instance.clone(), result))
    }

    pub fn begin_mod_update(&mut self) -> Option<(Instance, ManagedContent)> {
        let mods = self.mods_mut()?;
        let managed = mods.selected_installed()?.managed()?;
        mods.phase = ModsPhase::Updating {
            title: managed.title().to_owned(),
        };
        Some((mods.instance.clone(), managed))
    }

    pub fn request_mod_removal(&mut self) -> bool {
        let Some(mods) = self.mods_mut() else {
            return false;
        };
        let Some(managed) = mods
            .selected_installed()
            .and_then(InstalledContent::managed)
        else {
            return false;
        };
        mods.remove_confirmation = Some(managed);
        true
    }

    pub fn cancel_mod_removal(&mut self) {
        if let Some(mods) = self.mods_mut() {
            mods.remove_confirmation = None;
        }
    }

    pub fn begin_mod_removal(&mut self) -> Option<(Instance, ManagedContent)> {
        let mods = self.mods_mut()?;
        let managed = mods.remove_confirmation.take()?;
        mods.phase = ModsPhase::Removing {
            title: managed.title().to_owned(),
        };
        Some((mods.instance.clone(), managed))
    }

    pub fn push_console_character(&mut self, character: char) {
        if !character.is_control() {
            self.console_input.push(character);
        }
    }

    pub fn pop_console_character(&mut self) {
        self.console_input.pop();
    }

    pub fn take_console_input(&mut self) -> String {
        std::mem::take(&mut self.console_input)
    }

    pub fn replace_instances(&mut self, instances: Vec<Instance>) {
        let selected_id = self.selected().map(|instance| instance.id().clone());
        for instance in &instances {
            self.states
                .entry(instance.id().clone())
                .or_insert(InstanceState::Stopped);
        }
        self.states
            .retain(|id, _| instances.iter().any(|instance| instance.id() == id));
        self.instances = instances;
        self.selected = selected_id
            .and_then(|id| {
                self.instances
                    .iter()
                    .position(|instance| instance.id() == &id)
            })
            .unwrap_or(0)
            .min(self.instances.len().saturating_sub(1));
    }

    pub fn replace_runtimes(&mut self, mut runtimes: Vec<FabricRuntime>) {
        runtimes.sort_by(|left, right| right.cmp(left));
        self.runtimes = runtimes;
        self.selected_runtime = self
            .selected_runtime
            .min(self.runtimes.len().saturating_sub(1));
    }

    pub fn download_finished(&mut self, runtime: FabricRuntime) {
        if !self.runtimes.contains(&runtime) {
            self.runtimes.push(runtime.clone());
            self.runtimes.sort_by(|left, right| right.cmp(left));
        }
        self.create_form.runtime = Some(runtime.clone());
        self.create_form.download = None;
        self.create_form.step = CreateStep::Review;
        self.set_notice(NoticeKind::Success, format!("Cached {runtime}"));
    }

    pub fn download_resolved(&mut self, runtime: FabricRuntime) {
        self.create_form.download = Some(DownloadStatus::Downloading {
            runtime: runtime.clone(),
        });
        self.set_notice(
            NoticeKind::Info,
            format!("Downloading Fabric for Minecraft {}", runtime.minecraft),
        );
    }

    pub fn download_failed(&mut self, message: String) {
        self.create_form.download = Some(DownloadStatus::Failed {
            message: message.clone(),
        });
        self.set_notice(NoticeKind::Error, message);
    }

    pub fn apply_server_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::StateChanged { id, state } => {
                self.set_notice(NoticeKind::Info, format!("{}: {}", id, state_label(&state)));
                self.states.insert(id, state);
            }
            ServerEvent::ConsoleLine { id, stream, line } => {
                let lines = self.console.entry(id).or_default();
                lines.push_back(ConsoleEntry { stream, line });
                while lines.len() > MAX_CONSOLE_LINES {
                    lines.pop_front();
                }
            }
            ServerEvent::OperationFailed { id, message } => {
                self.set_notice(NoticeKind::Error, format!("{id}: {message}"));
            }
        }
    }

    pub fn begin_quit(&mut self) {
        self.quitting = true;
        self.set_notice(NoticeKind::Info, "Stopping running instances before exit");
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting
    }

    pub fn has_active_instances(&self) -> bool {
        self.states.values().any(|state| {
            matches!(
                state,
                InstanceState::Starting | InstanceState::Running { .. } | InstanceState::Stopping
            )
        })
    }

    pub fn running_count(&self) -> usize {
        self.states
            .values()
            .filter(|state| matches!(state, InstanceState::Running { .. }))
            .count()
    }
}

pub fn state_label(state: &InstanceState) -> String {
    match state {
        InstanceState::Stopped => "stopped".to_owned(),
        InstanceState::Starting => "starting".to_owned(),
        InstanceState::Running { pid } => format!("running · pid {pid}"),
        InstanceState::Stopping => "stopping".to_owned(),
        InstanceState::Failed { message } => format!("failed · {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::App;
    use crate::instance::{
        FabricLaunch, Instance, InstanceConfig, InstanceId, InstanceName, InstanceState,
    };
    use crate::runtime::FabricRuntime;
    use crate::supervisor::ServerEvent;
    use std::path::PathBuf;
    use std::str::FromStr;

    fn instance(id: &str) -> Instance {
        let id = InstanceId::from_str(id).unwrap();
        Instance::new(
            id,
            PathBuf::from("/tmp/test"),
            InstanceConfig::new(
                InstanceName::parse("Test").unwrap(),
                FabricLaunch::default(),
                FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap(),
            ),
        )
    }

    #[test]
    fn preserves_selection_when_instances_reload() {
        let mut app = App::new(vec![instance("alpha"), instance("beta")], Vec::new());
        app.select_next();
        app.replace_instances(vec![instance("beta"), instance("gamma")]);
        assert_eq!(app.selected().unwrap().id().as_str(), "beta");
    }

    #[test]
    fn applies_process_state_events() {
        let instance = instance("alpha");
        let id = instance.id().clone();
        let mut app = App::new(vec![instance], Vec::new());
        app.apply_server_event(ServerEvent::StateChanged {
            id: id.clone(),
            state: InstanceState::Running { pid: 42 },
        });
        assert_eq!(app.state(&id), &InstanceState::Running { pid: 42 });
    }
}
