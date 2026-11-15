//! Light or dark: which of a theme's two looks a window shows.

/// Light or dark, as CSS `color-scheme` and the XDG Settings portal name the two
/// (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.Settings.html>).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ColorScheme {
    /// Dark ink on a light ground.
    Light,
    /// Light ink on a dark ground.
    Dark,
}

impl ColorScheme {
    /// The scheme [`Preference::System`](crate::Preference::System) shows where nothing reports
    /// one (a harness, an offscreen host, a desktop with no reader): dark.
    pub const FALLBACK: Self = Self::Dark;

    /// Both schemes, light first, as [`ColorModel::ALL`](crate::ColorModel::ALL) lists its models.
    pub const ALL: [Self; 2] = [Self::Light, Self::Dark];
}

#[cfg(test)]
mod tests {
    use crate::theme::color_scheme::ColorScheme;

    /// `ALL` lists each scheme once, light first, and holds the fallback.
    #[test]
    fn all_lists_each_scheme_once() {
        assert_eq!(ColorScheme::ALL, [ColorScheme::Light, ColorScheme::Dark]);
        assert!(ColorScheme::ALL.contains(&ColorScheme::FALLBACK));
    }
}
