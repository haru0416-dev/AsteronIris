use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::time::Duration;

use super::state::WizardState;
use super::widgets::Spinner;

use super::app_handlers;
use super::app_render;

/// Run the full-screen TUI wizard. Returns the completed `WizardState` on success.
pub fn run_app() -> Result<WizardState> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = WizardState::new();
    let mut spinner = Spinner::new();

    let result = main_loop(&mut terminal, &mut state, &mut spinner);

    terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result?;
    Ok(state)
}

fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut WizardState,
    spinner: &mut Spinner,
) -> Result<()> {
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            app_render::draw_ui(area, frame.buffer_mut(), state, spinner);
        })?;

        if state.should_quit {
            return Ok(());
        }

        if state.summary_confirmed {
            return Ok(());
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    state.should_quit = true;
                    continue;
                }
                if key.code == KeyCode::Char('q') && !app_handlers::is_text_input_active(state) {
                    state.should_quit = true;
                    continue;
                }

                app_handlers::handle_key(state, key.code);
            }
        } else {
            spinner.advance();
        }
    }
}
