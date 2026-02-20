use ratatui::style::{Color, Modifier, Style};

pub const PRIMARY: Color = Color::Cyan;
pub const ACCENT: Color = Color::Green;
pub const WARNING: Color = Color::Yellow;
pub const ERROR: Color = Color::Red;
pub const DIM: Color = Color::DarkGray;
pub const TEXT: Color = Color::White;

pub fn title_style() -> Style {
    Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD)
}

pub fn heading_style() -> Style {
    Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD)
}

pub fn unselected_style() -> Style {
    Style::default().fg(DIM)
}

pub fn success_style() -> Style {
    Style::default().fg(ACCENT)
}

pub fn error_style() -> Style {
    Style::default().fg(ERROR)
}

pub fn warning_style() -> Style {
    Style::default().fg(WARNING)
}

pub fn dim_style() -> Style {
    Style::default().fg(DIM)
}

pub fn input_style() -> Style {
    Style::default().fg(TEXT)
}

pub fn cursor_style() -> Style {
    Style::default().fg(Color::Black).bg(TEXT)
}

pub fn step_done_style() -> Style {
    Style::default().fg(ACCENT)
}

pub fn step_current_style() -> Style {
    Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD)
}

pub fn step_pending_style() -> Style {
    Style::default().fg(DIM)
}

pub fn keybinding_style() -> Style {
    Style::default().fg(DIM)
}
