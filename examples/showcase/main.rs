//! The palantir widget tour. `main` wires the winit host, or with
//! `--shot SCRIPT [--out DIR] [--against DIR]`
//! plays a script on a window with no desktop ([`shot`]); chrome and page table are in [`shell`],
//! pages in [`pages`], shared tokens in [`support`].

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pages;
mod shell;
mod shot;
mod support;

use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use palantir::glam::UVec2;
use palantir::{PowerPreference, RequestedGpu, WindowConfig, WinitHost};
use tracing_subscriber::EnvFilter;

/// What the command line asks.
#[derive(Debug)]
enum Mode {
    /// The window.
    Window,
    /// A script played with no desktop, its pictures written into `out` and compared with the ones
    /// in `against`.
    Shot {
        script: PathBuf,
        out: PathBuf,
        against: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    // Logs go to stderr, so a shot's report on stdout holds nothing else.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(io::stderr)
        .init();
    let mut stderr = io::stderr().lock();
    let result = match mode(env::args().skip(1)) {
        Ok(Mode::Window) => window(),
        Ok(Mode::Shot {
            script,
            out,
            against,
        }) => shots(
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

/// The mode `args` ask, or the usage.
fn mode(mut args: impl Iterator<Item = String>) -> Result<Mode, String> {
    let usage = || "usage: showcase [--shot SCRIPT [--out DIR] [--against DIR]]".to_owned();
    let Some(first) = args.next() else {
        return Ok(Mode::Window);
    };
    if first != "--shot" {
        return Err(usage());
    }
    let script = PathBuf::from(args.next().ok_or_else(usage)?);
    let mut out = None;
    let mut against = None;
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--out" => &mut out,
            "--against" => &mut against,
            _ => return Err(usage()),
        };
        if slot.is_some() {
            return Err(usage());
        }
        *slot = Some(PathBuf::from(args.next().ok_or_else(usage)?));
    }
    if against.is_some() && !cfg!(feature = "golden") {
        return Err("--against compares through the golden feature; build with it".to_owned());
    }
    Ok(Mode::Shot {
        script,
        out: out.unwrap_or_else(|| PathBuf::from(shot::DEFAULT_OUT)),
        against,
    })
}

fn window() -> Result<(), String> {
    WinitHost::builder(shell::MAIN_WINDOW)
        .window(WindowConfig::new("palantir showcase").with_inner_size(UVec2::new(1600, 1000)))
        .build(|_ui, _handle| shell::Shell::new())
        .and_then(WinitHost::run)
        .map_err(|error| error.to_string())
}

fn shots(script: &Path, folders: shot::Folders<'_>) -> Result<(), String> {
    // No optional features: the tour draws with what every adapter has.
    #[expect(
        clippy::default_trait_access,
        reason = "the features are a wgpu type, which only `palantir::gpu` names"
    )]
    let gpu = RequestedGpu::headless(PowerPreference::HighPerformance, Default::default())
        .map_err(|error| error.to_string())?
        .gpu;
    shot::run(gpu, script, folders, &mut io::stdout().lock()).map_err(|error| error.to_string())
}
