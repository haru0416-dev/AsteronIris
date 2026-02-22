use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::{ProviderSubStep, WizardState};
use super::super::theme;
use super::super::widgets::{SelectListWidget, TextInputWidget};

pub struct ProviderStep<'a> {
    pub state: &'a WizardState,
}

impl Widget for ProviderStep<'_> {
    #[allow(clippy::too_many_lines)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 {
            return;
        }

        match self.state.provider_sub_step {
            ProviderSubStep::TierSelect => {
                let header = Line::from(Span::styled(
                    format!("  {}", t!("onboard.provider.select_category")),
                    theme::heading_style(),
                ));
                header.render(Rect::new(area.x, area.y, area.width, 1), buf);

                let list_area = Rect::new(
                    area.x,
                    area.y + 2,
                    area.width,
                    area.height.saturating_sub(2),
                );
                SelectListWidget::new(&self.state.provider_tier_select, true)
                    .render(list_area, buf);
            }
            ProviderSubStep::ProviderSelect => {
                let header = Line::from(Span::styled(
                    format!("  {}", t!("onboard.provider.select_provider")),
                    theme::heading_style(),
                ));
                header.render(Rect::new(area.x, area.y, area.width, 1), buf);

                let list_area = Rect::new(
                    area.x,
                    area.y + 2,
                    area.width,
                    area.height.saturating_sub(2),
                );
                SelectListWidget::new(&self.state.provider_select, true).render(list_area, buf);
            }
            ProviderSubStep::AuthMethodSelect => {
                let header = Line::from(Span::styled(
                    format!("  {}", t!("onboard.provider.auth_method_prompt")),
                    theme::heading_style(),
                ));
                header.render(Rect::new(area.x, area.y, area.width, 1), buf);

                let list_area = Rect::new(
                    area.x,
                    area.y + 2,
                    area.width,
                    area.height.saturating_sub(2),
                );
                SelectListWidget::new(&self.state.provider_auth_method_select, true)
                    .render(list_area, buf);
            }
            ProviderSubStep::ApiKey => {
                let header = Line::from(Span::styled(
                    format!("  {}", t!("onboard.provider.paste_key")),
                    theme::heading_style(),
                ));
                header.render(Rect::new(area.x, area.y, area.width, 1), buf);

                let input_area = Rect::new(area.x, area.y + 2, area.width, 1);
                TextInputWidget::new(&self.state.provider_api_key, "API Key", true)
                    .render(input_area, buf);
            }
            ProviderSubStep::ModelSelect => {
                let header = Line::from(Span::styled(
                    format!("  {}", t!("onboard.provider.select_model")),
                    theme::heading_style(),
                ));
                header.render(Rect::new(area.x, area.y, area.width, 1), buf);

                let list_area = Rect::new(
                    area.x,
                    area.y + 2,
                    area.width,
                    area.height.saturating_sub(2),
                );
                SelectListWidget::new(&self.state.provider_model_select, true)
                    .render(list_area, buf);
            }
            ProviderSubStep::CustomBaseUrl => {
                let header = Line::from(Span::styled(
                    format!("  {}", t!("onboard.provider.custom_title")),
                    theme::heading_style(),
                ));
                header.render(Rect::new(area.x, area.y, area.width, 1), buf);

                let desc = Line::from(Span::styled(
                    format!("  {}", t!("onboard.provider.custom_desc")),
                    theme::dim_style(),
                ));
                desc.render(Rect::new(area.x, area.y + 1, area.width, 1), buf);

                let input_area = Rect::new(area.x, area.y + 3, area.width, 1);
                TextInputWidget::new(
                    &self.state.provider_custom_base_url,
                    &t!("onboard.provider.base_url_prompt"),
                    true,
                )
                .render(input_area, buf);
            }
            ProviderSubStep::CustomApiKey => {
                let input_area = Rect::new(area.x, area.y, area.width, 1);
                TextInputWidget::new(
                    &self.state.provider_custom_api_key,
                    &t!("onboard.provider.api_key_prompt"),
                    true,
                )
                .render(input_area, buf);
            }
            ProviderSubStep::CustomModel => {
                let input_area = Rect::new(area.x, area.y, area.width, 1);
                TextInputWidget::new(
                    &self.state.provider_custom_model,
                    &t!("onboard.provider.model_prompt"),
                    true,
                )
                .render(input_area, buf);
            }
        }
    }
}
