//! The palantir widget tour. `main` wires the winit host, open on the first page or on
//! `--page PAGE`, or with `--shot SCRIPT [--out DIR] [--against DIR]` plays a script on a window
//! with no desktop ([`shot`]); the rail and the page table are in [`shell`], pages in [`pages`],
//! the look in [`support`] over the [`scaffold`].

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod appearance;
mod job;
mod pages;
mod scaffold;
mod shell;
mod shot;
#[cfg(test)]
mod shot_tests;
mod support;
/// Here and not in `support`, which the visual suite compiles whole, so these run once.
#[cfg(test)]
mod support_tests;

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use palantir::glam::UVec2;
use palantir::{WindowConfig, WinitHost};
use tracing_subscriber::EnvFilter;

use crate::shell::Shell;
use crate::shot::Mode;

fn main() -> ExitCode {
    // Logs go to stderr, so a shot's report on stdout holds nothing else.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(io::stderr)
        .init();
    let mut stderr = io::stderr().lock();
    let result = match Mode::of(env::args().skip(1)) {
        Ok(Mode::Window { page }) => window(page),
        Ok(Mode::Shot {
            script,
            out,
            against,
        }) => shot::shots(
            &script,
            shot::Folders {
                out: &out,
                against: against.as_deref(),
            },
        ),
        Err(usage) => Err(usage),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            writeln!(stderr, "showcase: {message}").ok();
            ExitCode::FAILURE
        }
    }
}

/// The window, open on the page at `page`.
fn window(page: usize) -> Result<(), String> {
    WinitHost::builder(shell::MAIN_WINDOW)
        .window(WindowConfig::new("palantir showcase").with_inner_size(UVec2::new(1600, 1000)))
        .build(move |_ui, _handle| Shell::open(page))
        .and_then(WinitHost::run)
        .map_err(|error| error.to_string())
}
