//! Fleetops — CLI entrypoint: launch the board.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/main.rs
//! Deps:    tokio (explicit runtime); tui
//! Tested:  n/a (thin shell; all seams tested in their modules)
//!
//! Key responsibilities:
//! - Build the runtime and run the TUI; report a fatal error on stderr with exit 1.
//!
//! Design constraints:
//! - `unsafe_code = "forbid"` is crate policy (Cargo `[lints]`).
//! - Wave 1 has no subcommands; dispatch stays hand-rolled when they arrive (house style, no clap).

mod error;
mod panes;
mod runner;
mod tui;

use std::process::ExitCode;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("fleet: cannot start runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(tui::run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fleet: {e}");
            ExitCode::FAILURE
        }
    }
}
