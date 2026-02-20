use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::theme;

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// An animated spinner for async operations.
#[derive(Debug, Clone)]
pub struct Spinner {
    pub tick: usize,
}

impl Spinner {
    pub fn new() -> Self {
        Self { tick: 0 }
    }

    pub fn advance(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    fn frame(&self) -> &'static str {
        FRAMES[self.tick % FRAMES.len()]
    }
}

/// Renders a spinner with a message.
pub struct SpinnerWidget<'a> {
    pub spinner: &'a Spinner,
    pub message: &'a str,
}

impl<'a> SpinnerWidget<'a> {
    pub fn new(spinner: &'a Spinner, message: &'a str) -> Self {
        Self { spinner, message }
    }
}

impl Widget for SpinnerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width < 6 {
            return;
        }

        let line = Line::from(vec![
            Span::styled(format!("  {} ", self.spinner.frame()), theme::title_style()),
            Span::styled(self.message, theme::input_style()),
        ]);

        line.render(area, buf);
    }
}
