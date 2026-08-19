//! Baked SVG icon sets, and the rasterizer that turns one into pixels.
//!
//! An icon is a text glyph that came from an SVG. The set is
//! [`IconAtlas`](crate::icons::icon_atlas::IconAtlas) — normalized SVG plus a
//! name table, produced by `bake-icons` and compiled into the binary. Nothing
//! is rasterized at build time: the renderer rasterizes each icon at the exact
//! physical pixel size it is about to be drawn at
//! ([`IconRasterizer`](crate::icons::icon_rasterizer::IconRasterizer)) and
//! caches the result in the same kind of atlas the glyph cache uses, so an icon
//! is pixel-exact at every display scale and every zoom level.
//!
//! [`IconHandle`](crate::icons::icon_set::IconHandle) names one icon of one
//! loaded set in four `Copy` bytes — the baked data is `'static`, so unlike
//! [`ImageHandle`](crate::ImageHandle) there is nothing to reference-count and
//! no lifetime to hold.

pub(crate) mod icon_atlas;
pub(crate) mod icon_raster_key;
pub(crate) mod icon_rasterizer;
pub(crate) mod icon_registry;
pub(crate) mod icon_set;
