use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::theme;

/// A single-select list with keyboard navigation.
#[derive(Debug, Clone)]
pub struct SelectList {
    pub items: Vec<String>,
    pub selected: usize,
}

impl SelectList {
    pub fn new(items: Vec<String>) -> Self {
        Self { items, selected: 0 }
    }

    pub fn up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn down(&mut self) {
        if self.selected + 1 < self.items.len() {
            self.selected += 1;
        }
    }

    pub fn selected_value(&self) -> Option<&str> {
        self.items.get(self.selected).map(String::as_str)
    }

    pub fn set_items(&mut self, items: Vec<String>) {
        self.items = items;
        self.selected = 0;
    }
}

/// Renders a `SelectList` as a vertical list with a cursor indicator.
pub struct SelectListWidget<'a> {
    pub list: &'a SelectList,
    pub focused: bool,
}

impl<'a> SelectListWidget<'a> {
    pub fn new(list: &'a SelectList, focused: bool) -> Self {
        Self { list, focused }
    }
}

impl Widget for SelectListWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let available = area.height as usize;
        if available == 0 || area.width < 6 {
            return;
        }

        // Scroll window: keep selected item visible
        let total = self.list.items.len();
        let start = if self.list.selected >= available {
            self.list.selected - available + 1
        } else {
            0
        };
        let end = (start + available).min(total);

        for (row, idx) in (start..end).enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let y = area.y + row as u16;
            if y >= area.y + area.height {
                break;
            }

            let is_selected = idx == self.list.selected;
            let (indicator, style) = if is_selected && self.focused {
                ("  > ", theme::selected_style())
            } else if is_selected {
                ("  > ", theme::dim_style())
            } else {
                ("    ", theme::unselected_style())
            };

            let line = Line::from(vec![
                Span::styled(indicator, style),
                Span::styled(&self.list.items[idx], style),
            ]);

            let line_area = Rect::new(area.x, y, area.width, 1);
            line.render(line_area, buf);
        }
    }
}
