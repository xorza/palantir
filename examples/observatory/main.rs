//! The observatory: five studies of one fictional tool on one model, each built from palantir's
//! widgets, beside the showcase's reference pages. `main` wires the winit host, or with
//! `--shot SCRIPT [--out DIR] [--against DIR]` plays a script on a window with no desktop
//! ([`shot`]); the app is in [`app`].

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
#[path = "../shared/shot/mod.rs"]
mod shot;

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use palantir::glam::UVec2;
use palantir::{WindowConfig, WinitHost};
use tracing_subscriber::EnvFilter;

use crate::app::{MAIN_WINDOW, Shell};
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
        Ok(Mode::Window) => window(),
        Ok(Mode::Shot {
            script,
            out,
            against,
        }) => shot::shots::<Shell>(
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
            writeln!(stderr, "observatory: {message}").ok();
            ExitCode::FAILURE
        }
    }
}

fn window() -> Result<(), String> {
    WinitHost::builder(MAIN_WINDOW)
        .window(WindowConfig::new("palantir observatory").with_inner_size(UVec2::new(1600, 1000)))
        .build(|_ui, _handle| Shell::new())
        .and_then(WinitHost::run)
        .map_err(|error| error.to_string())
}
