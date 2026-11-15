//! [`PerScheme`]: one value for each color scheme.

use palantir::ColorScheme;

/// One `T` for each color scheme, read by the scheme that shows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PerScheme<T> {
    light: T,
    dark: T,
}

impl<T> PerScheme<T> {
    /// Each scheme's value, as `of` makes it.
    pub(crate) fn of(mut of: impl FnMut(ColorScheme) -> T) -> Self {
        Self {
            light: of(ColorScheme::Light),
            dark: of(ColorScheme::Dark),
        }
    }

    pub(crate) const fn get(&self, scheme: ColorScheme) -> &T {
        match scheme {
            ColorScheme::Light => &self.light,
            ColorScheme::Dark => &self.dark,
        }
    }

    pub(crate) const fn get_mut(&mut self, scheme: ColorScheme) -> &mut T {
        match scheme {
            ColorScheme::Light => &mut self.light,
            ColorScheme::Dark => &mut self.dark,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::scaffold::per_scheme::PerScheme;
    use palantir::ColorScheme;

    /// Each scheme reads and writes its own value: made from the scheme, a write to dark leaves
    /// light's.
    #[test]
    fn each_scheme_holds_its_own_value() {
        let mut values = PerScheme::of(|scheme| scheme == ColorScheme::Dark);
        assert!(!*values.get(ColorScheme::Light));
        assert!(*values.get(ColorScheme::Dark));
        *values.get_mut(ColorScheme::Dark) = false;
        assert!(!*values.get(ColorScheme::Dark));
        *values.get_mut(ColorScheme::Light) = true;
        assert!(*values.get(ColorScheme::Light));
    }
}
