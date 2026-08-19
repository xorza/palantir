//! Baked SVG icon sets, and the rasterizer that turns one into pixels.
//!
//! An icon is a text glyph that came from an SVG. The set is
//! [`IconAtlas`](crate::icons::icon_atlas::IconAtlas) — SVG sources plus a
//! name table, either compiled into the binary by `bake-icons` or built at
//! runtime. Nothing is rasterized ahead of time: the renderer rasterizes each
//! icon at the exact physical pixel size it is about to be drawn at
//! ([`IconRasterizer`](crate::icons::icon_rasterizer::IconRasterizer)) and
//! caches the result in the same kind of atlas the glyph cache uses, so an icon
//! is pixel-exact at every display scale and every zoom level.
//!
//! [`IconHandle`](crate::icons::icon_set::IconHandle) names one icon of one
//! loaded set in twelve `Copy` bytes — two ids plus the artwork's viewBox, so
//! resolving [`IconFit`](crate::IconFit) at encode time needs no lookup. Unlike
//! [`ImageHandle`](crate::ImageHandle) it owns nothing: the set behind it is
//! kept alive by the [`IconSet`](crate::IconSet) the app holds.

pub(crate) mod icon_atlas;
pub(crate) mod icon_raster_key;
pub(crate) mod icon_rasterizer;
pub(crate) mod icon_registry;
pub(crate) mod icon_set;
