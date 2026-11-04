//! Light or dark: which of a theme's two looks a window shows.

/// Light or dark, as CSS `color-scheme` and the XDG Settings portal name the two
/// (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.Settings.html>).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ColorScheme {
    /// Dark ink on a light ground.
    Light,
    /// Light ink on a dark ground.
    Dark,
}

impl ColorScheme {
    /// The scheme [`Preference::System`](crate::Preference::System) shows where nothing reports
    /// one (a harness, an offscreen host, a desktop with no reader): dark, the look palantir had
    /// before it read the system.
    pub const FALLBACK: Self = Self::Dark;
}
