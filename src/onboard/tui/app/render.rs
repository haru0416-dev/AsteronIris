use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::onboard::tui::state::{WizardState, WizardStep};
use crate::onboard::tui::steps;
use crate::onboard::tui::theme;
use crate::onboard::tui::widgets::progress::ProgressWidget;
use crate::onboard::tui::widgets::spinner::Spinner;

use super::handlers::is_text_input_active;

pub(super) fn draw_ui(area: Rect, buf: &mut Buffer, state: &WizardState, spinner: &Spinner) {
    // Top bar: step title
    let step_idx = state.current_step.index() + 1;
    let total = WizardStep::ALL.len();
    let step_label = state.current_step.label();

    let title_line = Line::from(vec![Span::styled(
        format!("  [{step_idx}/{total}] {step_label}"),
        theme::title_style(),
    )]);

    // Layout: top title (1) + body + bottom keybindings (1)
    let vertical = Layout::vertical([
        Constraint::Length(2), // Title
        Constraint::Min(4),    // Body
        Constraint::Length(2), // Keybindings
    ])
    .split(area);

    title_line.render(
        Rect::new(vertical[0].x, vertical[0].y, vertical[0].width, 1),
        buf,
    );

    // Separator
    let sep = Line::from(Span::styled(
        format!("  {}", "─".repeat(area.width.saturating_sub(4) as usize)),
        theme::dim_style(),
    ));
    sep.render(
        Rect::new(vertical[0].x, vertical[0].y + 1, vertical[0].width, 1),
        buf,
    );

    // Body: sidebar (8 chars) + main content
    let body = vertical[1];
    let horizontal = Layout::horizontal([
        Constraint::Length(8), // Sidebar
        Constraint::Min(20),   // Content
    ])
    .split(body);

    // Sidebar: step progress
    ProgressWidget::new(state.current_step, &state.completed_steps).render(horizontal[0], buf);

    // Main content: current step
    let content = horizontal[1];
    render_step(content, buf, state, spinner);

    // Bottom: keybindings
    let keys = keybinding_text(state);
    let keys_line = Line::from(Span::styled(format!("  {keys}"), theme::keybinding_style()));
    keys_line.render(vertical[2], buf);
}

fn render_step(area: Rect, buf: &mut Buffer, state: &WizardState, spinner: &Spinner) {
    match state.current_step {
        WizardStep::Workspace => {
            steps::workspace::WorkspaceStep { state }.render(area, buf);
        }
        WizardStep::Provider => {
            steps::provider::ProviderStep { state }.render(area, buf);
        }
        WizardStep::Channels => {
            steps::channels::ChannelsStep { state, spinner }.render(area, buf);
        }
        WizardStep::Tunnel => {
            steps::tunnel::TunnelStep { state }.render(area, buf);
        }
        WizardStep::ToolMode => {
            steps::tool_mode::ToolModeStep { state }.render(area, buf);
        }
        WizardStep::Memory => {
            steps::memory::MemoryStep { state }.render(area, buf);
        }
        WizardStep::Context => {
            steps::context::ContextStep { state }.render(area, buf);
        }
        WizardStep::Summary => {
            steps::summary::SummaryStep { state }.render(area, buf);
        }
    }
}

fn keybinding_text(state: &WizardState) -> String {
    let mut keys = Vec::new();

    if is_text_input_active(state) {
        keys.push("Enter Confirm");
        keys.push("Esc Cancel");
    } else {
        keys.push("↑↓ Navigate");
        keys.push("Enter Confirm");
        if state.current_step.index() > 0 {
            keys.push("Esc Back");
        }
        keys.push("q Quit");
    }

    keys.join("  ")
}
