use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::WizardState;
use super::super::theme;
use super::super::widgets::{SelectListWidget, ToggleWidget};

pub struct MemoryStep<'a> {
    pub state: &'a WizardState,
}

impl Widget for MemoryStep<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 {
            return;
        }

        let desc = Line::from(Span::styled(
            format!("  {}", t!("onboard.memory.intro")),
            theme::dim_style(),
        ));
        desc.render(Rect::new(area.x, area.y, area.width, 1), buf);

        let list_area = Rect::new(area.x, area.y + 2, area.width, 3);
        SelectListWidget::new(&self.state.memory_select, true).render(list_area, buf);

        // Auto-save toggle (only if not "none")
        if self.state.memory_select.selected != 2 {
            let y = area.y + 6;
            if y < area.y + area.height {
                let toggle_area = Rect::new(area.x, y, area.width, 1);
                ToggleWidget::new(
                    &self.state.memory_auto_save,
                    &t!("onboard.memory.auto_save_prompt"),
                    true,
                )
                .render(toggle_area, buf);
            }
        }
    }
}
