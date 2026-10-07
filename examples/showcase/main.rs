//! The palantir widget tour. `main` wires the winit host; chrome and page table are in [`shell`], pages in [`pages`], shared tokens in [`support`].

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pages;
mod shell;
mod support;

fn main() -> Result<(), palantir::WinitHostError> {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    palantir::WinitHost::builder(shell::MAIN_WINDOW)
        .window(
            palantir::WindowConfig::new("palantir showcase")
                .with_inner_size(palantir::UVec2::new(1600, 1000)),
        )
        .build(|ui, _handle| shell::State::new(ui))?
        .run()
}
