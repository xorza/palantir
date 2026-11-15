//! The palantir widget tour. `run` wires the winit host, open on the first page or on
//! `--page PAGE`, or with `--shot SCRIPT [--out DIR] [--against DIR]` plays a script on a window
//! with no desktop (`shot`); the rail and the page table are in `shell`, pages in `pages`, the
//! look in `scaffold`.
//!
//! A library, as egui's `egui_demo_lib`: it reaches palantir's public API alone, so the tour shows
//! what an application can write, and the benches and integration tests here run its pages. The
//! binary is `main.rs`.

mod app_state;
mod appearance;
#[cfg(feature = "internals")]
pub mod bench;
pub mod cube_renderer;
mod error;
mod job;
mod made_image;
mod pages;
mod scaffold;
mod shell;
mod shot;

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use palantir::{ErrorChain, Size, WindowConfig, WinitHost};
use tracing_subscriber::EnvFilter;

use crate::error::RunError;
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
        Err(error) => {
            writeln!(stderr, "showcase: {}", ErrorChain(&error)).ok();
            ExitCode::FAILURE
        }
    }
}

/// The window, open on the page at `page`.
fn window(page: usize) -> Result<(), RunError> {
    Ok(WinitHost::builder(MAIN_WINDOW)
        .window(WindowConfig::new("palantir showcase").with_inner_size(Size::new(1600.0, 1000.0)))
        .build(move |_ui, _handle| Shell::open(page))
        .and_then(WinitHost::run)?)
}

/// What the visual tests reach: a page recorded as its card records its body.
#[cfg(feature = "internals")]
pub mod internals {
    use palantir::{ColorScheme, Configure, Palette, Panel, Ui, WidgetId};

    use crate::app_state::AppState;
    use crate::pages::materials;
    use crate::scaffold::PAGE_GAP;
    use crate::scaffold::kit::Kits;
    use crate::shell::Shell;

    /// One page of the tour, its body recorded as [`Shell`]'s card records it, in `scheme` over
    /// `palette`, with no rail or header around it.
    #[derive(Debug)]
    pub struct PageScene {
        page: usize,
        app: AppState,
        kits: Kits,
        scheme: ColorScheme,
        palette: Palette,
    }

    impl PageScene {
        /// The page labelled `label`.
        ///
        /// # Panics
        ///
        /// When no page has the label.
        pub fn new(label: &str, scheme: ColorScheme, palette: Palette) -> Self {
            Self {
                page: Shell::page_idx(label).expect("the tour has the page"),
                app: AppState::new(),
                kits: Kits::default(),
                scheme,
                palette,
            }
        }

        /// Record the page's body.
        pub fn record(&mut self, ui: &mut Ui) {
            let k = self.kits.kit(self.scheme, &self.palette);
            Panel::vstack()
                .auto_id()
                .gap(PAGE_GAP)
                .show(ui, |ui| Shell::page_body(self.page, ui, &mut self.app, k));
        }
    }

    /// The button the materials page opens its frosted popup from.
    pub fn materials_popup_button() -> WidgetId {
        materials::popup_button()
    }
}
