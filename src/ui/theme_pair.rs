//! [`ThemePair`]: a window's theme for each color scheme.

use std::rc::Rc;

use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;

/// A window's theme for each [`ColorScheme`]; the resolved scheme picks the one its widgets read.
#[derive(Debug)]
pub(crate) struct ThemePair {
    light: Rc<Theme>,
    dark: Rc<Theme>,
}

impl ThemePair {
    /// Each scheme's [`Theme::stock`].
    pub(crate) fn stock() -> Self {
        Self {
            light: Rc::new(Theme::stock(ColorScheme::Light)),
            dark: Rc::new(Theme::stock(ColorScheme::Dark)),
        }
    }

    pub(crate) const fn get(&self, scheme: ColorScheme) -> &Rc<Theme> {
        match scheme {
            ColorScheme::Light => &self.light,
            ColorScheme::Dark => &self.dark,
        }
    }

    pub(crate) const fn get_mut(&mut self, scheme: ColorScheme) -> &mut Rc<Theme> {
        match scheme {
            ColorScheme::Light => &mut self.light,
            ColorScheme::Dark => &mut self.dark,
        }
    }
}
