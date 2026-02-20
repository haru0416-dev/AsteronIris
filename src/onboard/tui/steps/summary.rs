use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::WizardState;
use super::super::theme;

pub struct SummaryStep<'a> {
    pub state: &'a WizardState,
}

impl Widget for SummaryStep<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 6 {
            return;
        }

        let mut y = area.y;

        let title = Line::from(Span::styled(
            format!("  {}", t!("onboard.summary.quick_summary")),
            theme::heading_style(),
        ));
        title.render(Rect::new(area.x, y, area.width, 1), buf);
        y += 2;

        let lines = summary_lines(self.state);
        for line_text in &lines {
            if y >= area.y + area.height {
                break;
            }
            let line = Line::from(Span::styled(format!("  {line_text}"), theme::input_style()));
            line.render(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;
        }

        y += 1;
        if y < area.y + area.height {
            let confirm_style = if self.state.summary_confirmed {
                theme::success_style()
            } else {
                theme::warning_style()
            };
            let confirm = Line::from(Span::styled(
                if self.state.summary_confirmed {
                    format!("  ✓ {}", t!("onboard.summary.ready"))
                } else {
                    "  Press Enter to confirm and save".to_string()
                },
                confirm_style,
            ));
            confirm.render(Rect::new(area.x, y, area.width, 1), buf);
        }
    }
}

fn summary_lines(state: &WizardState) -> Vec<String> {
    let mut lines = Vec::with_capacity(10);

    lines.push(format!(
        "› {} {}",
        t!("onboard.summary.provider"),
        if state.selected_provider.is_empty() {
            "openrouter"
        } else {
            &state.selected_provider
        }
    ));
    lines.push(format!(
        "› {} {}",
        t!("onboard.summary.model"),
        if state.selected_model.is_empty() {
            "(default)"
        } else {
            &state.selected_model
        }
    ));

    // Memory
    let memory_backend = match state.memory_select.selected {
        1 => "markdown",
        2 => "none",
        _ => "sqlite",
    };
    let auto_save = if state.memory_select.selected == 2 {
        "off"
    } else if state.memory_auto_save.value {
        "on"
    } else {
        "off"
    };
    lines.push(format!(
        "› {} {memory_backend} (auto-save: {auto_save})",
        t!("onboard.summary.memory"),
    ));

    // Tunnel
    let tunnel = match state.tunnel_select.selected {
        1 => "Cloudflare",
        2 => "Tailscale",
        3 => "ngrok",
        4 => "Custom",
        _ => "none (local only)",
    };
    lines.push(format!("› {} {tunnel}", t!("onboard.summary.tunnel")));

    // Tool mode
    let tool_mode = if state.tool_mode_select.selected == 1 {
        t!("onboard.summary.composio_enabled").to_string()
    } else {
        t!("onboard.summary.composio_disabled").to_string()
    };
    lines.push(format!("› {} {tool_mode}", t!("onboard.summary.composio")));

    // Secrets
    let secrets = if state.encrypt_toggle.value {
        t!("onboard.summary.secrets_encrypted").to_string()
    } else {
        t!("onboard.summary.secrets_plaintext").to_string()
    };
    lines.push(format!("› {} {secrets}", t!("onboard.summary.secrets")));

    // Workspace
    lines.push(format!(
        "› {} {}",
        t!("status.workspace"),
        &state.workspace_dir
    ));
    lines.push(format!("› {} {}", t!("status.config"), &state.config_path));

    lines
}
