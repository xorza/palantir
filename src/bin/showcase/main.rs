//! The palantir widget tour. `main` only wires the winit host; the
//! chrome and the page table live in [`shell`], the pages themselves in
//! [`pages`], and every shared token in [`support`].

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
        .title("palantir showcase")
        .build(|ui, _handle| shell::State::new(ui))?
        .run()
}
