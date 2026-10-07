//! Baked SVG icon sets and the rasterizer that turns one into pixels.
//!
//! The set is an [`IconTable`](crate::icons::icon_table::IconTable): SVG sources plus a name table, compiled in as a generated `const` or built at runtime. Nothing is pre-rasterized: the renderer rasterizes each icon at its exact physical size ([`IconRasterizer`](crate::icons::icon_rasterizer::IconRasterizer)) into an atlas, so icons are pixel-exact at every scale and zoom.
//!
//! [`IconHandle`](crate::icons::icon_set::IconHandle) names one icon of one loaded set in sixteen `Copy` bytes (two ids plus the viewBox, so [`IconFit`](crate::widget::IconFit) resolves without a lookup). It owns nothing: the set lives while an [`IconSet`](crate::IconSet) clone does.

pub(crate) mod error;
pub(crate) mod icon_raster_key;
pub(crate) mod icon_rasterizer;
pub(crate) mod icon_registry;
pub(crate) mod icon_set;
pub(crate) mod icon_table;
pub(crate) mod svg_facts;

/// SVG documents the icon tests share.
#[cfg(test)]
pub(crate) mod internals {
    /// One fill colour over a 24 × 12 viewBox: a tintable icon.
    pub(crate) const ONE_COLOUR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 12"><rect width="24" height="12" fill="#4080c0"/><circle cx="6" cy="6" r="3" fill="#4080c0"/></svg>"##;

    /// Two fill colours over a 16 × 16 viewBox: a multi-colour icon.
    pub(crate) const TWO_COLOURS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="8" height="16" fill="#f00"/><rect x="8" width="8" height="16" fill="#00f"/></svg>"##;

    /// A document cut off after its opening; a parse rejects it.
    pub(crate) const BROKEN: &str = "<svg";
}
