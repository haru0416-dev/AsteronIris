use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::WizardState;
use super::super::theme;
use super::super::widgets::select_list::SelectListWidget;
use super::super::widgets::text_input::TextInputWidget;
use super::super::widgets::toggle::ToggleWidget;

pub struct ToolModeStep<'a> {
    pub state: &'a WizardState,
}

impl Widget for ToolModeStep<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 {
            return;
        }

        let desc = Line::from(Span::styled(
            format!("  {}", t!("onboard.tool_mode.intro")),
            theme::dim_style(),
        ));
        desc.render(Rect::new(area.x, area.y, area.width, 1), buf);

        let list_area = Rect::new(area.x, area.y + 2, area.width, 2);
        SelectListWidget::new(&self.state.tool_mode_select, true).render(list_area, buf);

        let mut y = area.y + 5;

        // If Composio selected, show API key input
        if self.state.tool_mode_select.selected == 1 && y < area.y + area.height {
            let input_area = Rect::new(area.x, y, area.width, 1);
            TextInputWidget::new(
                &self.state.composio_api_key,
                &t!("onboard.tool_mode.composio_key_prompt"),
                true,
            )
            .render(input_area, buf);
            y += 2;
        }

        // Encryption toggle
        if y < area.y + area.height {
            let encrypt_desc = Line::from(Span::styled(
                format!("  {}", t!("onboard.tool_mode.encrypt_intro")),
                theme::dim_style(),
            ));
            encrypt_desc.render(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;
        }

        if y < area.y + area.height {
            let toggle_area = Rect::new(area.x, y, area.width, 1);
            ToggleWidget::new(
                &self.state.encrypt_toggle,
                &t!("onboard.tool_mode.encrypt_prompt"),
                true,
            )
            .render(toggle_area, buf);
        }
    }
}
