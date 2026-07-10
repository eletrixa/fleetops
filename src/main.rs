//! Fleetops — CLI entrypoint: launch the board or run doctor.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/main.rs
//! Deps:    tokio (explicit runtime); tui, doctor, runner
//! Tested:  n/a (thin shell; all seams tested in their modules)
//!
//! Key responsibilities:
//! - Dispatch: (default) tui | doctor. Hand-rolled (house style, no clap).
//! - Report a fatal error on stderr with exit 1; unknown command exits 2.
//!
//! Design constraints:
//! - `unsafe_code = "forbid"` is crate policy (Cargo `[lints]`).

mod board;
mod discovery;
mod doctor;
mod error;
mod fold;
mod panes;
mod paths;
mod runner;
mod telemetry;
mod tui;

use std::process::ExitCode;

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("fleet: cannot start runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match command.as_deref() {
        None => match runtime.block_on(tui::run()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("fleet: {e}");
                ExitCode::FAILURE
            }
        },
        Some("doctor") => {
            let (report, scan_ok) = runtime.block_on(doctor::run(&runner::Exec));
            println!("{report}");
            if scan_ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE // spec 004: exit 1 when the scan itself failed
            }
        }
        Some(other) => {
            eprintln!("fleet: unknown command '{other}' (usage: fleet [doctor])");
            ExitCode::from(2)
        }
    }
}
