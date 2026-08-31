//! In-memory state for the terminal interface.
//!
//! This module deliberately contains no terminal, filesystem, network, or
//! process calls. Keyboard handling changes this state; `render` reads it.

use crate::instance::{Instance, InstanceId, InstanceState};
use crate::runtime::FabricRuntime;
use crate::supervisor::{OutputStream, ServerEvent};
use std::collections::{HashMap, VecDeque};

const MAX_CONSOLE_LINES: usize = 2_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Screen {
    Dashboard,
    Console,
    Runtimes,
    Create,
    Help,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum IdentityField {
    #[default]
    Id,
    Name,
}

#[derive(Clone, Debug)]
pub(super) struct CreateWizard {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) identity_field: IdentityField,
    pub(super) stage: CreateStage,
}

impl Default for CreateWizard {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            identity_field: IdentityField::Id,
            stage: CreateStage::Identity,
        }
    }
}

/// Each stage carries only the data valid in that stage. For example, a
/// creation review always has a cached runtime and no download error.
#[derive(Clone, Debug)]
pub(super) enum CreateStage {
    Identity,
    Runtime {
        selection: usize,
    },
    MinecraftVersion {
        value: String,
    },
    Downloading {
        requested: Option<String>,
        phase: DownloadPhase,
    },
    Review {
        runtime: FabricRuntime,
        eula_accepted: bool,
    },
}

#[derive(Clone, Debug)]
pub(super) enum DownloadPhase {
    Resolving,
    Downloading(FabricRuntime),
    Failed(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NoticeKind {
    Info,
    Success,
    Error,
}

#[derive(Clone, Debug)]
pub(super) struct Notice {
    pub(super) kind: NoticeKind,
    pub(super) message: String,
}

#[derive(Clone, Debug)]
pub(super) struct ConsoleLine {
    pub(super) stream: OutputStream,
    pub(super) line: String,
}

/// All state that exists only while the TUI is running.
///
/// Persistent instance configuration remains in `instance`; Java process
/// handles remain in `supervisor`.
pub(super) struct UiState {
    pub(super) instances: Vec<Instance>,
    pub(super) selected: usize,
    pub(super) states: HashMap<InstanceId, InstanceState>,
    pub(super) console: HashMap<InstanceId, VecDeque<ConsoleLine>>,
    pub(super) runtimes: Vec<FabricRuntime>,
    pub(super) selected_runtime: usize,
    pub(super) screen: Screen,
    pub(super) wizard: CreateWizard,
    pub(super) console_input: String,
    pub(super) notice: Notice,
    spinner_index: usize,
    pub(super) quitting: bool,
}

impl UiState {
    pub(super) fn new(instances: Vec<Instance>, runtimes: Vec<FabricRuntime>) -> Self {
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
            wizard: CreateWizard::default(),
            console_input: String::new(),
            notice: Notice {
                kind: NoticeKind::Info,
                message: "Ready".to_owned(),
            },
            spinner_index: 0,
            quitting: false,
        }
    }

    pub(super) fn selected_instance(&self) -> Option<&Instance> {
        self.instances.get(self.selected)
    }

    pub(super) fn state(&self, id: &InstanceId) -> &InstanceState {
        self.states.get(id).unwrap_or(&InstanceState::Stopped)
    }

    pub(super) fn select_next_instance(&mut self) {
        if !self.instances.is_empty() {
            self.selected = (self.selected + 1) % self.instances.len();
        }
    }

    pub(super) fn select_previous_instance(&mut self) {
        if !self.instances.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.instances.len() - 1);
        }
    }

    pub(super) fn select_next_runtime(&mut self) {
        if !self.runtimes.is_empty() {
            self.selected_runtime = (self.selected_runtime + 1) % self.runtimes.len();
        }
    }

    pub(super) fn select_previous_runtime(&mut self) {
        if !self.runtimes.is_empty() {
            self.selected_runtime = self
                .selected_runtime
                .checked_sub(1)
                .unwrap_or(self.runtimes.len() - 1);
        }
    }

    pub(super) fn replace_instances(&mut self, instances: Vec<Instance>) {
        let selected_id = self
            .selected_instance()
            .map(|instance| instance.id().clone());
        self.states
            .retain(|id, _| instances.iter().any(|instance| instance.id() == id));
        for instance in &instances {
            self.states
                .entry(instance.id().clone())
                .or_insert(InstanceState::Stopped);
        }
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

    pub(super) fn replace_runtimes(&mut self, runtimes: Vec<FabricRuntime>) {
        self.runtimes = runtimes;
        self.selected_runtime = self
            .selected_runtime
            .min(self.runtimes.len().saturating_sub(1));
    }

    pub(super) fn apply_server_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::StateChanged { id, state } => {
                self.states.insert(id, state);
            }
            ServerEvent::ConsoleLine { id, stream, line } => {
                let lines = self.console.entry(id).or_default();
                lines.push_back(ConsoleLine { stream, line });
                if lines.len() > MAX_CONSOLE_LINES {
                    lines.pop_front();
                }
            }
            ServerEvent::OperationFailed { id, message } => {
                self.states.insert(
                    id,
                    InstanceState::Failed {
                        message: message.clone(),
                    },
                );
                self.notice(NoticeKind::Error, message);
            }
        }
    }

    pub(super) fn notice(&mut self, kind: NoticeKind, message: impl Into<String>) {
        self.notice = Notice {
            kind,
            message: message.into(),
        };
    }

    pub(super) fn tick(&mut self) {
        self.spinner_index = (self.spinner_index + 1) % 10;
    }

    pub(super) fn spinner(&self) -> &'static str {
        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][self.spinner_index]
    }

    pub(super) fn running_count(&self) -> usize {
        self.states
            .values()
            .filter(|state| matches!(state, InstanceState::Running { .. }))
            .count()
    }

    pub(super) fn has_active_instances(&self) -> bool {
        self.states.values().any(|state| {
            matches!(
                state,
                InstanceState::Starting | InstanceState::Running { .. } | InstanceState::Stopping
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::UiState;
    use crate::instance::{FabricLaunch, InstanceConfig, InstanceId, InstanceName};
    use crate::runtime::FabricRuntime;
    use std::path::PathBuf;
    use std::str::FromStr;

    #[test]
    fn replacing_instances_preserves_the_selected_id() {
        let runtime = FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap();
        let first = crate::instance::Instance::new(
            InstanceId::from_str("first").unwrap(),
            PathBuf::from("/tmp/first"),
            InstanceConfig::new(
                InstanceName::parse("First").unwrap(),
                FabricLaunch::default(),
                runtime.clone(),
            ),
        );
        let second = crate::instance::Instance::new(
            InstanceId::from_str("second").unwrap(),
            PathBuf::from("/tmp/second"),
            InstanceConfig::new(
                InstanceName::parse("Second").unwrap(),
                FabricLaunch::default(),
                runtime,
            ),
        );
        let mut state = UiState::new(vec![first.clone(), second.clone()], Vec::new());
        state.select_next_instance();
        state.replace_instances(vec![first, second]);

        assert_eq!(state.selected_instance().unwrap().id().as_str(), "second");
    }
}
