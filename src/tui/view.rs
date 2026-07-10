//! Pure render: the board table + footer, a function of `&App` only.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/view.rs
//! Deps:    ratatui
//! Tested:  inline `#[cfg(test)]` TestBackend render assertions
//!
//! Key responsibilities:
//! - Lay out the pane board: status | name | cwd | tab/pane, with the selected row highlighted.
//! - Footer: pane count, refresh age, key hints, last error.
//!
//! Design constraints:
//! - Pure and read-only over `App`: no I/O, no `.await`, no state mutation (ratatui rule).
//! - Colour is a pure function of status (one `status → Style` map).

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;

use crate::panes::PaneStatus;

use super::model::App;

/// Render the whole board.
pub fn render(f: &mut Frame<'_>, app: &App) {
    let [body, footer] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
    render_table(f, body, app);
    render_footer(f, footer, app);
}

fn status_style(status: PaneStatus) -> (&'static str, Style) {
    match status {
        PaneStatus::Working => ("⠿ working", Style::default().fg(Color::Green)),
        PaneStatus::Idle => ("✳ idle", Style::default().fg(Color::Yellow)),
    }
}

fn render_table(f: &mut Frame<'_>, area: Rect, app: &App) {
    let header = Row::new(["STATUS", "SESSION", "CWD", "TAB:PANE"])
        .style(Style::default().add_modifier(Modifier::BOLD));
    let selected = app.selected_index();
    let rows = app.rows.iter().enumerate().map(|(i, r)| {
        let (label, style) = status_style(r.status);
        let marker = if r.is_active { "● " } else { "" };
        let row = Row::new([
            Cell::from(label).style(style),
            Cell::from(format!("{marker}{}", r.name)),
            Cell::from(r.cwd.clone()),
            Cell::from(format!("{}:{}", r.tab_id, r.pane_id)),
        ]);
        if selected == Some(i) {
            row.style(Style::default().add_modifier(Modifier::REVERSED))
        } else {
            row
        }
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Min(20),
            Constraint::Max(30),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(" fleet "));
    f.render_widget(table, area);
}

fn render_footer(f: &mut Frame<'_>, area: Rect, app: &App) {
    let text = match &app.error {
        Some(e) => format!("! {e}"),
        None => format!(
            "{} panes · refreshed {}s ago · j/k move · Enter jump · r refresh · q quit",
            app.rows.len(),
            app.refresh_age_secs
        ),
    };
    let style = if app.error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(Paragraph::new(Line::from(text)).style(style), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::PaneRow;
    use crate::tui::model::Msg;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn row(pane_id: u64, status: PaneStatus, name: &str, active: bool) -> PaneRow {
        PaneRow {
            pane_id,
            tab_id: 3,
            status,
            name: name.to_string(),
            cwd: "/tui/fleetops".to_string(),
            is_active: active,
        }
    }

    fn rendered(app: &App) -> String {
        let backend = TestBackend::new(90, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn board_renders_rows_and_footer() {
        let mut app = App::default();
        app.update(Msg::Panes(vec![
            row(
                47,
                PaneStatus::Working,
                "Resume FleetOps conversation",
                false,
            ),
            row(51, PaneStatus::Idle, "Review skills", true),
        ]));
        let screen = rendered(&app);
        assert!(screen.contains("Resume FleetOps conversation"));
        assert!(screen.contains("⠿ working"));
        assert!(screen.contains("✳ idle"));
        assert!(screen.contains("● Review skills"), "active pane marker");
        assert!(screen.contains("3:47"));
        assert!(screen.contains("2 panes"));
        assert!(screen.contains("Enter jump"));
    }

    #[test]
    fn footer_shows_error_instead_of_hints() {
        let mut app = App::default();
        app.update(Msg::Error("wezterm.exe: timed out after 5s".into()));
        let screen = rendered(&app);
        assert!(screen.contains("! wezterm.exe: timed out after 5s"));
        assert!(!screen.contains("Enter jump"));
    }

    #[test]
    fn empty_board_renders_without_panicking() {
        let app = App::default();
        let screen = rendered(&app);
        assert!(screen.contains("0 panes"));
    }
}
