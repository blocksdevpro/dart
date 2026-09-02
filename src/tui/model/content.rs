//! State for the mod, data-pack, and resource-pack workspace.

use crate::content::{ContentKind, ContentSearchHit, InstalledContent, ManagedContent};
use crate::instance::Instance;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) enum ModsTab {
    #[default]
    Installed,
    Discover,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ModsFocus {
    Installed,
    SearchInput,
    SearchResults,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ModsPhase {
    Idle,
    Searching,
    Installing { title: String },
    Updating { title: String },
    Removing { title: String },
}

impl ModsPhase {
    pub(in crate::tui) fn is_busy(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    pub(in crate::tui) fn label(&self) -> Option<&str> {
        match self {
            Self::Idle => None,
            Self::Searching => Some("Searching Modrinth"),
            Self::Installing { title } | Self::Updating { title } | Self::Removing { title } => {
                Some(title)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::tui) struct ModsView {
    pub(super) instance: Instance,
    pub(super) kind: ContentKind,
    pub(super) tab: ModsTab,
    pub(super) focus: ModsFocus,
    pub(super) installed: Vec<InstalledContent>,
    pub(super) installed_selected: usize,
    pub(super) query: String,
    pub(super) results: Vec<ContentSearchHit>,
    pub(super) result_selected: usize,
    pub(super) phase: ModsPhase,
    pub(super) remove_confirmation: Option<ManagedContent>,
}

impl ModsView {
    pub(in crate::tui) fn new(instance: Instance, installed: Vec<InstalledContent>) -> Self {
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

    pub(in crate::tui) fn instance(&self) -> &Instance {
        &self.instance
    }
    pub(in crate::tui) fn kind(&self) -> ContentKind {
        self.kind
    }
    pub(in crate::tui) fn tab(&self) -> ModsTab {
        self.tab
    }
    pub(in crate::tui) fn focus(&self) -> ModsFocus {
        self.focus
    }
    pub(in crate::tui) fn installed(&self) -> &[InstalledContent] {
        &self.installed
    }
    pub(in crate::tui) fn installed_selected(&self) -> usize {
        self.installed_selected
    }
    pub(in crate::tui) fn query(&self) -> &str {
        &self.query
    }
    pub(in crate::tui) fn results(&self) -> &[ContentSearchHit] {
        &self.results
    }
    pub(in crate::tui) fn result_selected(&self) -> usize {
        self.result_selected
    }
    pub(in crate::tui) fn phase(&self) -> &ModsPhase {
        &self.phase
    }
    pub(in crate::tui) fn remove_confirmation(&self) -> Option<&ManagedContent> {
        self.remove_confirmation.as_ref()
    }
    pub(in crate::tui) fn selected_installed(&self) -> Option<&InstalledContent> {
        self.installed.get(self.installed_selected)
    }
    pub(in crate::tui) fn selected_result(&self) -> Option<&ContentSearchHit> {
        self.results.get(self.result_selected)
    }

    pub(in crate::tui) fn replace_installed(&mut self, installed: Vec<InstalledContent>) {
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

    pub(in crate::tui) fn select_next(&mut self) {
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

    pub(in crate::tui) fn select_previous(&mut self) {
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
