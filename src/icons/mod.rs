//! Baked SVG icon sets, and the rasterizer that turns one into pixels.
//!
//! An icon is a text glyph that came from an SVG. The set is
//! [`IconTable`](crate::icons::icon_table::IconTable) — SVG sources plus a
//! name table, either compiled in as a generated `const` or built at runtime
//! from the sources themselves. Nothing is rasterized ahead of time: the
//! renderer rasterizes each
//! icon at the exact physical pixel size it is about to be drawn at
//! ([`IconRasterizer`](crate::icons::icon_rasterizer::IconRasterizer)) and
//! caches the result in the same kind of atlas the glyph cache uses, so an icon
//! is pixel-exact at every display scale and every zoom level.
//!
//! [`IconHandle`](crate::icons::icon_set::IconHandle) names one icon of one
//! loaded set in sixteen `Copy` bytes — two ids plus the artwork's viewBox, so
//! resolving [`IconFit`](crate::widget::IconFit) at encode time needs no lookup. Unlike
//! [`ImageHandle`](crate::ImageHandle) it owns nothing: the set behind it is
//! kept alive by the [`IconSet`](crate::IconSet) the app holds, and unloaded
//! when the last clone of that goes.

pub(crate) mod icon_raster_key;
pub(crate) mod icon_rasterizer;
pub(crate) mod icon_registry;
pub(crate) mod icon_set;
pub(crate) mod icon_table;
pub(crate) mod svg_facts;

/// SVG documents the icon tests share.
#[cfg(test)]
pub(crate) mod internals {
    /// One fill colour over a 24 × 12 viewBox — a tintable icon.
    pub(crate) const ONE_COLOUR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 12"><rect width="24" height="12" fill="#4080c0"/><circle cx="6" cy="6" r="3" fill="#4080c0"/></svg>"##;

    /// Two fill colours over a 16 × 16 viewBox — a multi-colour icon.
    pub(crate) const TWO_COLOURS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="8" height="16" fill="#f00"/><rect x="8" width="8" height="16" fill="#00f"/></svg>"##;

    /// A document cut off after its opening — what a parse rejects.
    pub(crate) const BROKEN: &str = "<svg";
}
