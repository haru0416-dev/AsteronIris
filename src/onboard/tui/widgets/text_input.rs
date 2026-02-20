use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::theme;

/// A stateful single-line text input with cursor navigation.
#[derive(Debug, Clone)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
    pub masked: bool,
}

impl TextInput {
    pub fn new(initial: &str) -> Self {
        let cursor = initial.len();
        Self {
            value: initial.to_string(),
            cursor,
            masked: false,
        }
    }

    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    pub fn insert(&mut self, ch: char) {
        self.value.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.value[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
            self.value.replace_range(prev..self.cursor, "");
            self.cursor = prev;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.value.len() {
            let next = self.value[self.cursor..]
                .char_indices()
                .nth(1)
                .map_or(self.value.len(), |(i, _)| self.cursor + i);
            self.value.replace_range(self.cursor..next, "");
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.value[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.value.len() {
            self.cursor = self.value[self.cursor..]
                .char_indices()
                .nth(1)
                .map_or(self.value.len(), |(i, _)| self.cursor + i);
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.value.len();
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.value.trim().is_empty()
    }
}

/// Renders a `TextInput` as a single line with cursor highlighting.
pub struct TextInputWidget<'a> {
    pub input: &'a TextInput,
    pub label: &'a str,
    pub focused: bool,
}

impl<'a> TextInputWidget<'a> {
    pub fn new(input: &'a TextInput, label: &'a str, focused: bool) -> Self {
        Self {
            input,
            label,
            focused,
        }
    }
}

impl Widget for TextInputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width < 4 {
            return;
        }

        let label_style = if self.focused {
            theme::selected_style()
        } else {
            theme::dim_style()
        };

        let display_value = if self.input.masked {
            "*".repeat(self.input.value.chars().count())
        } else {
            self.input.value.clone()
        };

        if self.focused {
            // Split the value around cursor for highlighting
            let (before, at_cursor, after) = split_at_cursor(&display_value, self.input.cursor);

            let line = Line::from(vec![
                Span::styled(format!("  {}: ", self.label), label_style),
                Span::styled(before, theme::input_style()),
                Span::styled(at_cursor, theme::cursor_style()),
                Span::styled(after, theme::input_style()),
            ]);
            line.render(area, buf);
        } else {
            let line = Line::from(vec![
                Span::styled(format!("  {}: ", self.label), label_style),
                Span::styled(&display_value, theme::dim_style()),
            ]);
            line.render(area, buf);
        }
    }
}

fn split_at_cursor(display: &str, cursor: usize) -> (String, String, String) {
    let chars: Vec<char> = display.chars().collect();
    let char_cursor = display[..cursor.min(display.len())].chars().count();

    let before: String = chars[..char_cursor].iter().collect();
    let at: String = if char_cursor < chars.len() {
        chars[char_cursor].to_string()
    } else {
        " ".to_string()
    };
    let after: String = if char_cursor + 1 < chars.len() {
        chars[char_cursor + 1..].iter().collect()
    } else {
        String::new()
    };

    (before, at, after)
}
