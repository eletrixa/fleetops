//! The TUI event loop: draw the board, fold key/sensor/tick messages. Only I/O site.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/mod.rs
//! Deps:    ratatui (init/restore + panic hook), crossterm (EventStream), tokio; panes, runner
//! Tested:  the seams are tested in keys/model/view/panes; this loop is the thin I/O shell.
//!
//! Key responsibilities:
//! - Own the terminal (via `ratatui::try_init`/`try_restore` — installs the panic hook) and the loop.
//! - Run the wezterm sensor task (poll on tick / on demand) and the jump effect; results and
//!   failures come back over one mpsc channel as `Msg`s.
//!
//! Design constraints:
//! - Async work never runs on the UI task — the sensor and jumps are spawned; the loop only
//!   `select!`s. `view`/`update` do no I/O.
//! - Read-only over the fleet; the only mutating verb is `activate-pane` (focus).

pub mod keys;
pub mod model;
pub mod view;

use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::error::AppResult;
use crate::panes;
use crate::runner::{Exec, Runner};

use model::{App, Msg};

/// Sensor poll cadence (spec 001: ~2 s).
const POLL: Duration = Duration::from_secs(2);
/// Footer age / redraw tick.
const TICK: Duration = Duration::from_secs(1);

/// Run the board until the user quits.
pub async fn run() -> AppResult<()> {
    let mut terminal = ratatui::try_init()?;
    let result = event_loop(&mut terminal).await;
    let _ = ratatui::try_restore();
    result
}

async fn event_loop(terminal: &mut ratatui::DefaultTerminal) -> AppResult<()> {
    let runner: Arc<dyn Runner> = Arc::new(Exec);
    let (tx, mut rx) = mpsc::channel::<Msg>(16);
    spawn_poll(&runner, &tx);

    let mut app = App::default();
    let mut events = EventStream::new();
    let mut poll = tokio::time::interval(POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    poll.tick().await; // consume the immediate first tick; spawn_poll above covers t=0
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        terminal.draw(|f| view::render(f, &app))?;

        tokio::select! {
            maybe = events.next() => match maybe {
                Some(Ok(Event::Key(key))) => {
                    if let Some(action) = keys::map(key) {
                        app.update(Msg::Key(action));
                    }
                }
                Some(Ok(_)) => {} // resize etc. — next draw picks it up
                Some(Err(_)) | None => break,
            },
            Some(msg) = rx.recv() => app.update(msg),
            _ = poll.tick() => spawn_poll(&runner, &tx),
            _ = tick.tick() => app.update(Msg::Tick),
        }

        if app.refresh_requested {
            app.refresh_requested = false;
            spawn_poll(&runner, &tx);
        }
        if let Some(pane_id) = app.pending_jump.take() {
            spawn_jump(&runner, &tx, pane_id);
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}

/// Poll the wezterm sensor off the UI task; the result arrives as a `Msg`.
fn spawn_poll(runner: &Arc<dyn Runner>, tx: &mpsc::Sender<Msg>) {
    let runner = Arc::clone(runner);
    let tx = tx.clone();
    tokio::spawn(async move {
        let msg = match panes::list_panes(runner.as_ref()).await {
            Ok(rows) => Msg::Panes(rows),
            Err(e) => Msg::Error(e.to_string()),
        };
        let _ = tx.send(msg).await; // loop gone = shutting down
    });
}

/// Fire the jump off the UI task; only failures surface (as a footer `Msg::Error`).
fn spawn_jump(runner: &Arc<dyn Runner>, tx: &mpsc::Sender<Msg>, pane_id: u64) {
    let runner = Arc::clone(runner);
    let tx = tx.clone();
    tokio::spawn(async move {
        if let Err(e) = runner.run(&panes::activate_pane_spec(pane_id)).await {
            let _ = tx.send(Msg::Error(format!("jump: {e}"))).await;
        }
    });
}
