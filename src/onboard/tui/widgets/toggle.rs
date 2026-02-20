use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::theme;

/// A boolean Yes/No toggle.
#[derive(Debug, Clone)]
pub struct Toggle {
    pub value: bool,
}

impl Toggle {
    pub fn new(initial: bool) -> Self {
        Self { value: initial }
    }

    pub fn toggle(&mut self) {
        self.value = !self.value;
    }
}

/// Renders a `Toggle` as `[Yes] / No` or `Yes / [No]`.
pub struct ToggleWidget<'a> {
    pub toggle: &'a Toggle,
    pub label: &'a str,
    pub focused: bool,
}

impl<'a> ToggleWidget<'a> {
    pub fn new(toggle: &'a Toggle, label: &'a str, focused: bool) -> Self {
        Self {
            toggle,
            label,
            focused,
        }
    }
}

impl Widget for ToggleWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width < 10 {
            return;
        }

        let label_style = if self.focused {
            theme::selected_style()
        } else {
            theme::dim_style()
        };

        let (yes_style, no_style) = if self.focused {
            if self.toggle.value {
                (theme::selected_style(), theme::unselected_style())
            } else {
                (theme::unselected_style(), theme::selected_style())
            }
        } else {
            let s = theme::dim_style();
            (s, s)
        };

        let line = Line::from(vec![
            Span::styled(format!("  {}: ", self.label), label_style),
            Span::styled(if self.toggle.value { "[Yes]" } else { " Yes " }, yes_style),
            Span::styled(" / ", theme::dim_style()),
            Span::styled(if self.toggle.value { " No " } else { "[No]" }, no_style),
        ]);

        line.render(area, buf);
    }
}
