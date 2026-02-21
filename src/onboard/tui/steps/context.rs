use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::WizardState;
use super::super::theme;
use super::super::widgets::{SelectListWidget, TextInputWidget};

pub struct ContextStep<'a> {
    pub state: &'a WizardState,
}

impl Widget for ContextStep<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 4 {
            return;
        }

        let desc = Line::from(Span::styled(
            format!("  {}", t!("onboard.context.intro")),
            theme::dim_style(),
        ));
        desc.render(Rect::new(area.x, area.y, area.width, 1), buf);

        let sub = self.state.context_sub_field;
        let mut y = area.y + 2;

        // Field 0: Name
        if y < area.y + area.height {
            let input_area = Rect::new(area.x, y, area.width, 1);
            TextInputWidget::new(
                &self.state.context_name,
                &t!("onboard.context.name_prompt"),
                sub == 0,
            )
            .render(input_area, buf);
            y += 2;
        }

        // Field 1: Timezone
        if sub == 1 && y < area.y + area.height {
            let label = Line::from(Span::styled(
                format!("  {}:", t!("onboard.context.tz_prompt")),
                theme::heading_style(),
            ));
            label.render(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;

            let remaining = (area.y + area.height).saturating_sub(y);
            let list_area = Rect::new(area.x, y, area.width, remaining);
            SelectListWidget::new(&self.state.context_tz_select, true).render(list_area, buf);
        } else if sub != 1 && y < area.y + area.height {
            // Show timezone value when not editing
            let tz_val = if self.state.context_tz_select.selected
                == self.state.context_tz_select.items.len() - 1
            {
                self.state.context_tz_custom.value.as_str()
            } else {
                self.state
                    .context_tz_select
                    .selected_value()
                    .unwrap_or("UTC")
            };
            let tz_line = Line::from(vec![
                Span::styled(
                    format!("  {}: ", t!("onboard.context.tz_prompt")),
                    theme::dim_style(),
                ),
                Span::styled(tz_val, theme::dim_style()),
            ]);
            tz_line.render(Rect::new(area.x, y, area.width, 1), buf);
            y += 2;
        }

        // Field 2: Agent name
        if sub >= 2 && y < area.y + area.height {
            let input_area = Rect::new(area.x, y, area.width, 1);
            TextInputWidget::new(
                &self.state.context_agent_name,
                &t!("onboard.context.agent_name_prompt"),
                sub == 2,
            )
            .render(input_area, buf);
            y += 2;
        }

        // Field 3: Communication style
        if sub == 3 && y < area.y + area.height {
            let label = Line::from(Span::styled(
                format!("  {}:", t!("onboard.context.style_prompt")),
                theme::heading_style(),
            ));
            label.render(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;

            let remaining = (area.y + area.height).saturating_sub(y);
            let list_area = Rect::new(area.x, y, area.width, remaining);
            SelectListWidget::new(&self.state.context_style_select, true).render(list_area, buf);
        }

        // Field 4: Custom style text (if custom selected)
        if sub == 4 && y < area.y + area.height {
            let input_area = Rect::new(area.x, y, area.width, 1);
            TextInputWidget::new(
                &self.state.context_style_custom,
                &t!("onboard.context.custom_style_prompt"),
                true,
            )
            .render(input_area, buf);
        }
    }
}
