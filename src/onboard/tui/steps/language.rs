use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::WizardState;
use super::super::theme;
use super::super::widgets::SelectListWidget;

pub struct LanguageStep<'a> {
    pub state: &'a WizardState,
}

impl Widget for LanguageStep<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 {
            return;
        }

        let header = Line::from(Span::styled(
            format!("  {}", t!("onboard.language.select_prompt")),
            theme::heading_style(),
        ));
        header.render(Rect::new(area.x, area.y, area.width, 1), buf);

        let list_area = Rect::new(
            area.x,
            area.y + 2,
            area.width,
            area.height.saturating_sub(2),
        );
        SelectListWidget::new(&self.state.language_select, true).render(list_area, buf);
    }
}
