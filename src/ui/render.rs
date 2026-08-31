//! Read-only Ratatui rendering for the terminal interface.

use super::model::{CreateStage, DownloadPhase, IdentityField, NoticeKind, Screen, UiState};
use crate::instance::{Instance, InstanceState};
use crate::runtime::FabricRuntime;
use crate::supervisor::OutputStream;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Row, Table,
    TableState, Wrap,
};

const ACCENT: Color = Color::Rgb(90, 200, 250);
const MUTED: Color = Color::Rgb(125, 135, 150);
const GOOD: Color = Color::Rgb(95, 215, 140);
const WARN: Color = Color::Rgb(250, 190, 80);
const BAD: Color = Color::Rgb(245, 105, 120);

pub(super) fn draw(frame: &mut Frame<'_>, state: &UiState) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(9),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(frame.area());
    draw_header(frame, state, areas[0]);
    match state.screen {
        Screen::Console => draw_console(frame, state, areas[1]),
        Screen::Runtimes => draw_runtimes(frame, state, areas[1]),
        _ => draw_dashboard(frame, state, areas[1]),
    }
    draw_notice(frame, state, areas[2]);
    draw_footer(frame, state, areas[3]);
    match state.screen {
        Screen::Create => draw_create(frame, state),
        Screen::Help => draw_help(frame),
        _ => {}
    }
}

fn draw_header(frame: &mut Frame<'_>, state: &UiState, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            " DART ",
            Style::default().fg(Color::Black).bg(ACCENT).bold(),
        ),
        Span::raw("  Fabric instance manager"),
        Span::styled(
            format!(
                "  {} instances · {} running · {} runtimes ",
                state.instances.len(),
                state.running_count(),
                state.runtimes.len()
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

fn draw_dashboard(frame: &mut Frame<'_>, state: &UiState, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    let items = state.instances.iter().map(|instance| {
        let (marker, color) = state_marker(state.state(instance.id()), state.spinner());
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
    let mut list_state =
        ListState::default().with_selected((!state.instances.is_empty()).then_some(state.selected));
    frame.render_stateful_widget(list, columns[0], &mut list_state);

    let details = state.selected_instance().map_or_else(
        || {
            vec![
                Line::styled("No instances yet", Style::default().fg(Color::White).bold()),
                Line::from(""),
                Line::styled("Press n to create one.", Style::default().fg(MUTED)),
                Line::styled(
                    "Dart will download Fabric or reuse a cached launcher.",
                    Style::default().fg(MUTED),
                ),
            ]
        },
        |instance| instance_details(state, instance),
    );
    frame.render_widget(
        Paragraph::new(details)
            .wrap(Wrap { trim: false })
            .block(panel(" OVERVIEW ", false).padding(Padding::uniform(1))),
        columns[1],
    );
}

fn instance_details(state: &UiState, instance: &Instance) -> Vec<Line<'static>> {
    let current = state.state(instance.id());
    let (marker, color) = state_marker(current, state.spinner());
    let mut lines = vec![
        Line::styled(
            instance.config().name.to_string(),
            Style::default().fg(Color::White).bold(),
        ),
        Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(color).bold()),
            Span::styled(state_label(current), Style::default().fg(color)),
        ]),
        Line::from(""),
        labeled("Minecraft", instance.config().fabric.minecraft.to_string()),
        labeled("Fabric loader", instance.config().fabric.loader.to_string()),
        labeled("Installer", instance.config().fabric.installer.to_string()),
        labeled(
            "Memory",
            format!(
                "{}-{} MiB",
                instance.config().launch.min_memory_mib,
                instance.config().launch.max_memory_mib
            ),
        ),
        labeled("Directory", instance.root().display().to_string()),
        Line::from(""),
        Line::styled("Recent activity", Style::default().fg(ACCENT).bold()),
    ];
    let recent = state
        .console
        .get(instance.id())
        .into_iter()
        .flat_map(|lines| lines.iter().rev().take(4).rev());
    let mut saw_output = false;
    for entry in recent {
        saw_output = true;
        lines.push(Line::styled(
            entry.line.clone(),
            Style::default().fg(match entry.stream {
                OutputStream::Stdout => MUTED,
                OutputStream::Stderr => BAD,
            }),
        ));
    }
    if !saw_output {
        lines.push(Line::styled(
            "No console output yet",
            Style::default().fg(MUTED),
        ));
    }
    lines
}

fn draw_runtimes(frame: &mut Frame<'_>, state: &UiState, area: Rect) {
    let rows = state.runtimes.iter().map(|runtime| {
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
    let mut table_state = TableState::default()
        .with_selected((!state.runtimes.is_empty()).then_some(state.selected_runtime));
    frame.render_stateful_widget(table, area, &mut table_state);
}

fn draw_console(frame: &mut Frame<'_>, state: &UiState, area: Rect) {
    let Some(instance) = state.selected_instance() else {
        draw_dashboard(frame, state, area);
        return;
    };
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(3)])
        .split(area);
    let visible = areas[0].height.saturating_sub(2) as usize;
    let lines = state
        .console
        .get(instance.id())
        .into_iter()
        .flat_map(|entries| entries.iter().rev().take(visible).rev())
        .map(|entry| {
            Line::styled(
                entry.line.clone(),
                match entry.stream {
                    OutputStream::Stdout => Style::default(),
                    OutputStream::Stderr => Style::default().fg(BAD),
                },
            )
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
            Span::raw(&state.console_input),
        ]))
        .block(panel(" COMMAND ", true)),
        areas[1],
    );
    let cursor_x = areas[1].x + 3 + state.console_input.chars().count() as u16;
    frame.set_cursor_position((
        cursor_x.min(areas[1].right().saturating_sub(2)),
        areas[1].y + 1,
    ));
}

fn draw_create(frame: &mut Frame<'_>, state: &UiState) {
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
    let active = match state.wizard.stage {
        CreateStage::Identity => 0,
        CreateStage::Runtime { .. }
        | CreateStage::MinecraftVersion { .. }
        | CreateStage::Downloading { .. } => 1,
        CreateStage::Review { .. } => 2,
    };
    let breadcrumb = [" 1 Identity ", " 2 Runtime ", " 3 Review "]
        .into_iter()
        .enumerate()
        .flat_map(|(index, label)| {
            [
                Span::styled(
                    label,
                    if index == active {
                        Style::default().fg(Color::Black).bg(ACCENT).bold()
                    } else {
                        Style::default().fg(MUTED)
                    },
                ),
                Span::raw(" "),
            ]
        });
    frame.render_widget(Paragraph::new(Line::from_iter(breadcrumb)), regions[0]);
    match &state.wizard.stage {
        CreateStage::Identity => draw_identity(frame, state, regions[1]),
        CreateStage::Runtime { selection } => {
            draw_runtime_choice(frame, state, regions[1], *selection)
        }
        CreateStage::MinecraftVersion { value } => draw_version(frame, value, regions[1]),
        CreateStage::Downloading { requested, phase } => {
            draw_download(frame, requested, phase, regions[1], state.spinner())
        }
        CreateStage::Review {
            runtime,
            eula_accepted,
        } => draw_review(frame, state, runtime, *eula_accepted, regions[1]),
    }
}

fn draw_identity(frame: &mut Frame<'_>, state: &UiState, area: Rect) {
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
        &state.wizard.id,
        fields[1],
        state.wizard.identity_field == IdentityField::Id,
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
        &state.wizard.name,
        fields[4],
        state.wizard.identity_field == IdentityField::Name,
    );
}

fn draw_runtime_choice(frame: &mut Frame<'_>, state: &UiState, area: Rect, selection: usize) {
    let mut items = state
        .runtimes
        .iter()
        .map(|runtime| {
            ListItem::new(Line::from(vec![
                Span::styled("● ", Style::default().fg(GOOD)),
                Span::raw(runtime.label()),
                Span::styled("  cached", Style::default().fg(MUTED)),
            ]))
        })
        .collect::<Vec<_>>();
    items.push(ListItem::new(Line::from("↓ Download latest stable")));
    items.push(ListItem::new(Line::from(
        "↓ Download a Minecraft version...",
    )));
    let list = List::new(items)
        .block(panel(" CHOOSE A RUNTIME ", true))
        .highlight_style(Style::default().bg(Color::Rgb(40, 52, 65)))
        .highlight_symbol("▌");
    let mut list_state = ListState::default().with_selected(Some(selection));
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_version(frame: &mut Frame<'_>, value: &str, area: Rect) {
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
    draw_input(frame, value, regions[1], true);
}

fn draw_download(
    frame: &mut Frame<'_>,
    requested: &Option<String>,
    phase: &DownloadPhase,
    area: Rect,
    spinner: &str,
) {
    let lines = match phase {
        DownloadPhase::Resolving => vec![
            Line::from(""),
            activity_line(spinner, "Resolving Fabric versions"),
            Line::styled(
                requested.as_deref().map_or_else(
                    || "Latest stable Minecraft".to_owned(),
                    |version| format!("Minecraft {version}"),
                ),
                Style::default().fg(MUTED),
            ),
        ],
        DownloadPhase::Downloading(runtime) => vec![
            Line::from(""),
            activity_line(spinner, "Downloading Fabric launcher"),
            Line::styled(runtime.label(), Style::default().fg(MUTED)),
        ],
        DownloadPhase::Failed(message) => vec![
            Line::from(""),
            Line::styled("Fabric download failed", Style::default().fg(BAD).bold()),
            Line::from(""),
            Line::styled(message.clone(), Style::default().fg(BAD)),
            Line::from(""),
            Line::styled(
                "Press Enter or Esc to choose another runtime.",
                Style::default().fg(MUTED),
            ),
        ],
    };
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

fn draw_review(
    frame: &mut Frame<'_>,
    state: &UiState,
    runtime: &FabricRuntime,
    eula_accepted: bool,
    area: Rect,
) {
    let lines = vec![
        Line::styled("Ready to create", Style::default().fg(GOOD).bold()),
        Line::from(""),
        labeled("ID", state.wizard.id.trim()),
        labeled("Name", state.wizard.name.trim()),
        labeled("Minecraft", runtime.minecraft.as_str()),
        labeled("Fabric loader", runtime.loader.as_str()),
        labeled("Installer", runtime.installer.as_str()),
        Line::from(""),
        Line::styled(
            "Dart will copy the cached Fabric launcher into this instance.",
            Style::default().fg(MUTED),
        ),
        Line::from(""),
        Line::styled(
            if eula_accepted {
                "[x] I have read and accept the Minecraft EULA."
            } else {
                "[ ] I have read and accept the Minecraft EULA."
            },
            Style::default()
                .fg(if eula_accepted { GOOD } else { WARN })
                .bold(),
        ),
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
        Line::from("  Up/Down or j/k  Select an instance or runtime"),
        Line::from("  Enter            Open the selected instance console"),
        Line::from(""),
        Line::styled("Instances", Style::default().fg(ACCENT).bold()),
        Line::from("  n                Create an instance"),
        Line::from("  s / x            Start / stop the selected instance"),
        Line::from("  r                Reload instance files"),
        Line::from("  v                View cached Fabric runtimes"),
        Line::from(""),
        Line::styled("Global", Style::default().fg(ACCENT).bold()),
        Line::from("  ?                Open this help"),
        Line::from("  q / Ctrl-C       Stop servers and quit"),
    ];
    frame.render_widget(
        Paragraph::new(content).block(panel(" HELP ", true).padding(Padding::uniform(1))),
        area,
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
fn draw_notice(frame: &mut Frame<'_>, state: &UiState, area: Rect) {
    let (icon, color) = match state.notice.kind {
        NoticeKind::Info => ("•", ACCENT),
        NoticeKind::Success => ("✓", GOOD),
        NoticeKind::Error => ("!", BAD),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {icon} "), Style::default().fg(color).bold()),
            Span::styled(&state.notice.message, Style::default().fg(color)),
        ])),
        area,
    );
}
fn draw_footer(frame: &mut Frame<'_>, state: &UiState, area: Rect) {
    let hints = match state.screen {
        Screen::Dashboard => "n New  s Start  x Stop  Enter Console  v Runtimes  ? Help  q Quit",
        Screen::Console => "Enter Send  Esc Dashboard  Ctrl-C Quit",
        Screen::Runtimes => "Up/Down Select  n New instance  r Reload  Esc Dashboard",
        Screen::Create => "Follow the prompts; Esc goes back",
        Screen::Help => "Any key Close",
    };
    frame.render_widget(
        Paragraph::new(hints)
            .style(Style::default().fg(MUTED))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(MUTED)),
            ),
        area,
    );
}
fn activity_line(spinner: &str, label: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{spinner}  "), Style::default().fg(ACCENT).bold()),
        Span::styled(label, Style::default().bold()),
    ])
}
fn panel<'a>(title: &'a str, focused: bool) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { ACCENT } else { MUTED }))
}
fn labeled<'a>(label: &'a str, value: impl Into<String>) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<15}"), Style::default().fg(MUTED)),
        Span::raw(value.into()),
    ])
}
fn state_label(state: &InstanceState) -> String {
    match state {
        InstanceState::Stopped => "stopped".to_owned(),
        InstanceState::Starting => "starting".to_owned(),
        InstanceState::Running { pid } => format!("running · pid {pid}"),
        InstanceState::Stopping => "stopping".to_owned(),
        InstanceState::Failed { message } => format!("failed · {message}"),
    }
}
fn state_marker(state: &InstanceState, spinner: &str) -> (String, Color) {
    match state {
        InstanceState::Stopped => ("○".to_owned(), MUTED),
        InstanceState::Starting | InstanceState::Stopping => (spinner.to_owned(), WARN),
        InstanceState::Running { .. } => ("●".to_owned(), GOOD),
        InstanceState::Failed { .. } => ("!".to_owned(), BAD),
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
    use super::draw;
    use crate::ui::model::UiState;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn empty_dashboard_explains_the_next_action() {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, &UiState::new(Vec::new(), Vec::new())))
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(screen.contains("No instances yet"));
        assert!(screen.contains("Press n to create one."));
    }
}
