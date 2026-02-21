use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::WizardState;
use super::super::theme;
use super::super::widgets::{SelectListWidget, TextInputWidget, ToggleWidget};

pub struct TunnelStep<'a> {
    pub state: &'a WizardState,
}

impl Widget for TunnelStep<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 {
            return;
        }

        let desc = Line::from(Span::styled(
            format!("  {}", t!("onboard.tunnel.intro")),
            theme::dim_style(),
        ));
        desc.render(Rect::new(area.x, area.y, area.width, 1), buf);

        let selected = self.state.tunnel_select.selected;

        // If no sub-input needed, show the list
        let list_area = Rect::new(
            area.x,
            area.y + 2,
            area.width,
            area.height.saturating_sub(4),
        );
        SelectListWidget::new(&self.state.tunnel_select, true).render(list_area, buf);

        // Show conditional inputs below the list
        #[allow(clippy::cast_possible_truncation)]
        let input_y = area.y + 2 + self.state.tunnel_select.items.len() as u16 + 1;
        if input_y >= area.y + area.height {
            return;
        }

        match selected {
            1 => {
                // Cloudflare — token input
                let input_area = Rect::new(area.x, input_y, area.width, 1);
                TextInputWidget::new(
                    &self.state.tunnel_token,
                    &t!("onboard.tunnel.cloudflare_token_prompt"),
                    true,
                )
                .render(input_area, buf);
            }
            2 => {
                // Tailscale — funnel toggle
                let toggle_area = Rect::new(area.x, input_y, area.width, 1);
                ToggleWidget::new(
                    &self.state.tunnel_funnel,
                    &t!("onboard.tunnel.tailscale_funnel_prompt"),
                    true,
                )
                .render(toggle_area, buf);
            }
            3 => {
                // ngrok — token + domain
                let input_area = Rect::new(area.x, input_y, area.width, 1);
                TextInputWidget::new(
                    &self.state.tunnel_token,
                    &t!("onboard.tunnel.ngrok_token_prompt"),
                    true,
                )
                .render(input_area, buf);

                if input_y + 1 < area.y + area.height {
                    let domain_area = Rect::new(area.x, input_y + 1, area.width, 1);
                    TextInputWidget::new(
                        &self.state.tunnel_domain,
                        &t!("onboard.tunnel.ngrok_domain_prompt"),
                        false,
                    )
                    .render(domain_area, buf);
                }
            }
            4 => {
                // Custom — command input
                let input_area = Rect::new(area.x, input_y, area.width, 1);
                TextInputWidget::new(
                    &self.state.tunnel_command,
                    &t!("onboard.tunnel.custom_prompt"),
                    true,
                )
                .render(input_area, buf);
            }
            _ => {} // Skip — no inputs
        }
    }
}
