//! The suite's own themes, written out whole, so no change to a recipe, `Palette` or `Metrics`
//! moves a golden. A new `Theme` field fails to compile until both themes hold it.

mod dark;
mod light;

use palantir::{ColorScheme, Theme};

/// The theme every fixture renders under in `scheme`.
pub(crate) fn fixture(scheme: ColorScheme) -> Theme {
    match scheme {
        ColorScheme::Dark => dark::theme(),
        ColorScheme::Light => light::theme(),
    }
}
