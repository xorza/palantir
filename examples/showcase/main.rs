//! The palantir widget tour. `main` wires the winit host; chrome and page table are in [`shell`], pages in [`pages`], shared tokens in [`support`].

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pages;
mod shell;
mod support;

use palantir::{UVec2, WindowConfig, WinitHost, WinitHostError};
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), WinitHostError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    WinitHost::builder(shell::MAIN_WINDOW)
        .window(WindowConfig::new("palantir showcase").with_inner_size(UVec2::new(1600, 1000)))
        .build(|ui, _handle| shell::Shell::new(ui))?
        .run()
}
