use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::WizardState;
use super::super::theme;
use super::super::widgets::text_input::TextInputWidget;
use super::super::widgets::toggle::ToggleWidget;

pub struct WorkspaceStep<'a> {
    pub state: &'a WizardState,
}

impl Widget for WorkspaceStep<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 4 {
            return;
        }

        let mut y = area.y;

        // Show default path
        let path_line = Line::from(vec![
            Span::styled("  ", theme::dim_style()),
            Span::styled(
                t!(
                    "onboard.workspace.default_location",
                    path = &self.state.workspace_dir
                ),
                theme::dim_style(),
            ),
        ]);
        let row = Rect::new(area.x, y, area.width, 1);
        path_line.render(row, buf);
        y += 2;

        // Toggle: use default?
        let toggle_area = Rect::new(area.x, y, area.width, 1);
        let focused = true;
        ToggleWidget::new(
            &self.state.workspace_use_default,
            &t!("onboard.workspace.use_default"),
            focused,
        )
        .render(toggle_area, buf);
        y += 2;

        // If custom path, show text input
        if !self.state.workspace_use_default.value && y < area.y + area.height {
            let input_area = Rect::new(area.x, y, area.width, 1);
            TextInputWidget::new(
                &self.state.workspace_custom_path,
                &t!("onboard.workspace.enter_path"),
                true,
            )
            .render(input_area, buf);
        }
    }
}
