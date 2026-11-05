//! The palantir widget tour. `run` wires the winit host, open on the first page or on
//! `--page PAGE`, or with `--shot SCRIPT [--out DIR] [--against DIR]` plays a script on a window
//! with no desktop (`shot`); the rail and the page table are in `shell`, pages in [`pages`],
//! the look in [`support`] over the `scaffold`.
//!
//! A library, as egui's `egui_demo_lib`: it reaches palantir's public API alone, so the tour shows
//! what an application can write, and the benches and integration tests here run its pages. The
//! binary is `main.rs`.

mod app_state;
mod appearance;
#[cfg(feature = "internals")]
pub mod bench;
pub mod cube_renderer;
mod job;
mod made_image;
pub mod pages;
mod scaffold;
mod shell;
mod shot;
#[cfg(test)]
mod shot_tests;
pub mod support;
#[cfg(test)]
mod support_tests;

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use palantir::{Size, WindowConfig, WinitHost};
use tracing_subscriber::EnvFilter;

pub use crate::shell::{MAIN_WINDOW, Shell};
use crate::shot::Mode;

/// The tour as the command line asks.
pub fn run() -> ExitCode {
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
    WinitHost::builder(MAIN_WINDOW)
        .window(WindowConfig::new("palantir showcase").with_inner_size(Size::new(1600.0, 1000.0)))
        .build(move |_ui, _handle| Shell::open(page))
        .and_then(WinitHost::run)
        .map_err(|error| error.to_string())
}
