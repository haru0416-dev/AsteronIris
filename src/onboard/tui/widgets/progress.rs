use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::WizardStep;
use super::super::theme;

/// Step sidebar showing progress through wizard steps.
pub struct ProgressWidget {
    pub current: WizardStep,
    pub completed: Vec<WizardStep>,
}

impl ProgressWidget {
    pub fn new(current: WizardStep, completed: &[WizardStep]) -> Self {
        Self {
            current,
            completed: completed.to_vec(),
        }
    }
}

impl Widget for ProgressWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 || area.width < 4 {
            return;
        }

        for (row, step) in WizardStep::ALL.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let y = area.y + row as u16;
            if y >= area.y + area.height {
                break;
            }

            let is_current = *step == self.current;
            let is_done = self.completed.contains(step);

            let (marker, style) = if is_done {
                ("●", theme::step_done_style())
            } else if is_current {
                ("◉", theme::step_current_style())
            } else {
                ("○", theme::step_pending_style())
            };

            let short = step.short();
            let line = Line::from(vec![
                Span::styled(format!(" {marker} "), style),
                Span::styled(short, style),
            ]);

            let line_area = Rect::new(area.x, y, area.width, 1);
            line.render(line_area, buf);
        }
    }
}
