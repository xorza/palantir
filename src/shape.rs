use crate::layout::types::align::Align;
use crate::layout::types::span::Span;
use crate::primitives::mesh::Mesh;
use crate::primitives::{
    approx::noop_f32, color::Color, corners::Corners, rect::Rect, stroke::Stroke,
};
use glam::Vec2;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};

/// User-facing paint primitive. Pushed into the active tree via
/// [`crate::Ui::add_shape`], which copies the data into the per-frame
/// arena and converts to the internal [`ShapeRecord`] form.
///
/// The `'a` lifetime is borrowed by [`Shape::Mesh`]; the other three
/// variants infer `Shape<'static>` and behave identically at the call
/// site to today's owned variants.
#[derive(Clone, Debug)]
pub enum Shape<'a> {
    RoundedRect {
        local_rect: Option<Rect>,
        radius: Corners,
        fill: Color,
        stroke: Stroke,
    },
    /// Two-point stroked line — ergonomic shorthand for a 2-point
    /// `Polyline { Single(color) }`. Lowers to `ShapeRecord::Polyline`
    /// at authoring time; there's no `ShapeRecord::Line`.
    Line {
        a: Vec2,
        b: Vec2,
        width: f32,
        color: Color,
    },
    /// Stroked polyline with per-vertex or per-segment coloring. The
    /// framework copies `points` and `colors` into the active tree's
    /// per-frame arenas at `add_shape` time, so the borrows only have
    /// to outlive the call. `colors` length is constrained by `mode`
    /// (see [`PolylineColors`]); mismatches hard-assert.
    Polyline {
        points: &'a [Vec2],
        colors: PolylineColors<'a>,
        width: f32,
    },
    Text {
        local_rect: Option<Rect>,
        text: Cow<'static, str>,
        color: Color,
        font_size_px: f32,
        line_height_px: f32,
        wrap: TextWrap,
        align: Align,
    },
    /// User-supplied colored triangle mesh. The framework copies
    /// `mesh.vertices` / `mesh.indices` into the active `Tree`'s mesh
    /// arena at `add_shape` time, so `mesh` only has to outlive the
    /// call. `tint` multiplies every vertex color in the shader —
    /// lets the same mesh paint in different colors without rebuilding.
    Mesh {
        mesh: &'a Mesh,
        local_rect: Option<Rect>,
        tint: Color,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum ShapeRecord {
    /// Filled/stroked rounded rectangle. With `local_rect = None` it covers
    /// the owner node's full arranged rect (position/size come from layout).
    /// With `local_rect = Some(r)` it paints `r` at owner-relative coords —
    /// `r.min = (0, 0)` is the owner's top-left. The sub-rect form paints in
    /// the slot it was pushed in (interleaved with children via the slot
    /// mechanism — see `Tree::add_shape`), still under the owner's clip but
    /// outside its pan transform. Used for scrollbar tracks/thumbs (pushed
    /// after body content → slot N) and TextEdit carets (pushed after the
    /// Text shape on a leaf → slot 0, after the Text in record order).
    RoundedRect {
        local_rect: Option<Rect>,
        radius: Corners,
        fill: Color,
        stroke: Stroke,
    },
    /// Stroked polyline. `points`/`colors` index into the active
    /// tree's `polyline_points` / `polyline_colors` arenas. `colors`
    /// length depends on `color_mode`: 1 for `Single`,
    /// `points.len()` for `PerPoint`, `points.len() - 1` for
    /// `PerSegment`. `content_hash` summarizes points+colors+mode
    /// bytes for cache identity — two frames with identical content
    /// share a hash even though their span offsets differ. `bbox` is
    /// the axis-aligned bounds of `points` in owner-relative
    /// (record) coords — derived, not authoritative; the encoder
    /// translates it into cmd-buffer coords by adding the owner
    /// rect's origin. Computed at lowering time so the encoder hot
    /// path stays a single `extend(map)` over the point slice.
    Polyline {
        width: f32,
        color_mode: ColorMode,
        points: Span,
        colors: Span,
        bbox: Rect,
        content_hash: u64,
    },
    /// Shaped text run — *authoring inputs only*. Measured size and
    /// shaped-buffer key are layout outputs and live on
    /// `LayoutResult.text_shapes`, not here. `wrap` selects between "shape
    /// once and freeze" (`Single`) and "reshape if the parent commits a
    /// narrower width than the natural unbroken line" (`Wrap`). `align`
    /// positions the glyph bbox inside the owner leaf's arranged rect (or
    /// `local_rect` if set) — the encoder reads it together with the
    /// shaped run's `measured` to shift the emitted `DrawText` rect.
    /// `HAlign::Auto`/`Stretch` and `VAlign::Auto`/`Stretch` collapse to
    /// top-left for text (glyphs don't stretch).
    ///
    /// `local_rect` mirrors `RoundedRect::local_rect`: `None` paints into
    /// the owner's arranged rect (deflated by the node's `padding`);
    /// `Some(lr)` paints `lr` at owner-relative coords (`lr.min = (0, 0)`
    /// is owner top-left), with `padding` skipped and `align` positioning
    /// the run *inside `lr`*. Lets a custom widget place multiple text
    /// runs in one leaf without each clobbering the others.
    Text {
        local_rect: Option<Rect>,
        /// `Cow<'static, str>` so static-string labels (the common case via
        /// `&'static str → Into<Cow<…>>`) round-trip with only pointer-copy
        /// `Clone`s — no per-frame heap alloc. Dynamic strings still allocate
        /// once into `Cow::Owned` at the authoring boundary.
        text: Cow<'static, str>,
        color: Color,
        font_size_px: f32,
        /// Line-height in logical px, fed straight to the shaper's
        /// `Metrics::new`. Authoring-side widgets typically set this to
        /// `font_size_px * line_height_mult` where the multiplier
        /// defaults to [`crate::text::LINE_HEIGHT_MULT`] (1.2). Carrying
        /// the resolved px on the shape — instead of a multiplier the
        /// shaper would re-resolve — means the shaper doesn't have to
        /// know about widget conventions, and two `ShapeRecord::Text` runs at
        /// the same font-size but different leading correctly produce
        /// distinct cached shaped buffers (via [`TextCacheKey::lh_q`]).
        line_height_px: f32,
        wrap: TextWrap,
        align: Align,
    },
    /// User-supplied colored triangle mesh. Vertex/index data lives in
    /// the active `Tree`'s `mesh_vertices` / `mesh_indices` arenas;
    /// these spans index into them. `content_hash` summarizes
    /// vertex+index bytes for cache identity — two frames with
    /// identical mesh content share a hash even though their span
    /// offsets differ.
    Mesh {
        local_rect: Option<Rect>,
        tint: Color,
        vertices: Span,
        indices: Span,
        content_hash: u64,
    },
}

/// Color source for [`Shape::Polyline`]. Length constraints
/// enforced by hard `assert!` at `add_shape` — a mismatch is a
/// caller bug.
#[derive(Clone, Copy, Debug)]
pub enum PolylineColors<'a> {
    /// One color for the whole stroke. Broadcast to every
    /// cross-section.
    Single(Color),
    /// One color per input point. `len()` must equal `points.len()`.
    /// GPU lerps between adjacent cross-sections, giving a smooth
    /// gradient along the stroke.
    PerPoint(&'a [Color]),
    /// One color per segment. `len()` must equal
    /// `points.len() - 1`. Tessellator duplicates interior
    /// cross-sections so each segment paints as a solid block —
    /// no color bleed at joins.
    PerSegment(&'a [Color]),
}

/// Per-frame side-table arenas for shape variants that need
/// variable-length backing storage. Lives on both
/// [`crate::forest::tree::Tree`] (records reference these via
/// `Span`s) and [`crate::renderer::frontend::cmd_buffer::RenderCmdBuffer`]
/// (cmd payloads do the same). Cleared together per frame,
/// capacity retained — single struct keeps the lifecycle and
/// future-extension story (curves, etc.) in one place instead of
/// scattered fields on every container.
#[derive(Default)]
pub(crate) struct ShapeArenas {
    /// Vertex + index storage for `ShapeRecord::Mesh`.
    pub(crate) meshes: Mesh,
    /// Point storage for `ShapeRecord::Polyline`. Indexed by the
    /// record's `points` `Span`.
    pub(crate) polyline_points: Vec<Vec2>,
    /// Color storage for `ShapeRecord::Polyline`. Length per
    /// record is 1, `points.len()`, or `points.len() - 1` per
    /// `ColorMode`.
    pub(crate) polyline_colors: Vec<Color>,
}

impl ShapeArenas {
    /// Drop all per-frame contents; preserve capacity.
    pub(crate) fn clear(&mut self) {
        self.meshes.clear();
        self.polyline_points.clear();
        self.polyline_colors.clear();
    }
}

/// Storage tag for [`ShapeRecord::Polyline`]. `u8` for compactness
/// on the record; promoted to `u32` in `DrawPolylinePayload` to
/// keep that struct Pod-aligned. Discriminants are stable
/// (`Single=0`, `PerPoint=1`, `PerSegment=2`) so cache keys don't
/// shift across reorderings.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ColorMode {
    Single = 0,
    PerPoint = 1,
    PerSegment = 2,
}

/// Wrap mode for [`ShapeRecord::Text`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextWrap {
    /// ShapeRecord once at unbounded width and never reshape. Used by every text
    /// run that fits on a single line — labels, headings, anything that
    /// shouldn't wrap.
    Single,
    /// Reshape during measure if the parent commits a width narrower than
    /// the natural unbroken line. The widest unbreakable run (longest word)
    /// is the floor — text overflows rather than breaking inside a word.
    Wrap,
}

impl Hash for ShapeRecord {
    /// Discriminant tags are stable (`RoundedRect=0`, `Line=1`, `Text=2`) so
    /// cache keys don't shift if variants are reordered.
    fn hash<H: Hasher>(&self, h: &mut H) {
        match self {
            ShapeRecord::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            } => {
                h.write_u8(0);
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                radius.hash(h);
                fill.hash(h);
                stroke.hash(h);
            }
            ShapeRecord::Polyline {
                width,
                color_mode,
                points: _,
                colors: _,
                bbox: _,
                content_hash,
            } => {
                h.write_u8(1);
                h.write_u32(width.to_bits());
                h.write_u8(*color_mode as u8);
                h.write_u64(*content_hash);
            }
            ShapeRecord::Text {
                local_rect,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
            } => {
                h.write_u8(2);
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                text.hash(h);
                color.hash(h);
                h.write_u32(font_size_px.to_bits());
                h.write_u32(line_height_px.to_bits());
                h.write_u16(((align.raw() as u16) << 8) | *wrap as u8 as u16);
            }
            ShapeRecord::Mesh {
                local_rect,
                tint,
                vertices: _,
                indices: _,
                content_hash,
            } => {
                h.write_u8(3);
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                tint.hash(h);
                h.write_u64(*content_hash);
            }
        }
    }
}

/// True iff `local_rect` is set with a degenerate or negative extent
/// — paints no pixels regardless of fill/stroke/text. Broader than
/// `Size::approx_zero` (which is strict both-axes-near-zero); this
/// also catches `Rect::new(0, 0, -10, 20)` and similar from
/// authoring bugs. `None` means "paint into owner's full rect" and
/// is never paint-empty.
#[inline]
fn local_rect_paint_empty(local_rect: &Option<Rect>) -> bool {
    use crate::primitives::approx::EPS;
    local_rect.is_some_and(|r| r.size.w <= EPS || r.size.h <= EPS)
}

impl Shape<'_> {
    /// True if this shape paints nothing visible. `Ui::add_shape`
    /// filters these out so widgets can push speculatively without
    /// guarding.
    pub fn is_noop(&self) -> bool {
        match self {
            Shape::RoundedRect {
                local_rect,
                fill,
                stroke,
                ..
            } => local_rect_paint_empty(local_rect) || (fill.is_noop() && stroke.is_noop()),
            Shape::Line { width, color, .. } => noop_f32(*width) || color.is_noop(),
            Shape::Polyline {
                points,
                colors,
                width,
            } => {
                if noop_f32(*width) || points.len() < 2 {
                    return true;
                }
                match colors {
                    PolylineColors::Single(c) => c.is_noop(),
                    PolylineColors::PerPoint(cs) => cs.iter().all(|c| c.is_noop()),
                    PolylineColors::PerSegment(cs) => cs.iter().all(|c| c.is_noop()),
                }
            }
            Shape::Text {
                text,
                color,
                local_rect,
                ..
            } => local_rect_paint_empty(local_rect) || text.is_empty() || color.is_noop(),
            Shape::Mesh {
                mesh,
                local_rect,
                tint,
            } => {
                local_rect_paint_empty(local_rect)
                    || tint.is_noop()
                    || mesh.is_empty()
                    || mesh.indices.len() < 3
                    || mesh.indices.len() % 3 != 0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::hash::Hasher as FxHasher;
    use crate::primitives::color::Color;

    #[test]
    fn shape_mesh_hash_excludes_span_offsets() {
        // Same content_hash + local_rect + tint → same Shape hash even
        // when spans differ (frame-local storage offsets must not bleed
        // into identity).
        use std::hash::Hasher as _;
        let a = ShapeRecord::Mesh {
            local_rect: None,
            tint: Color {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            },
            vertices: Span::new(0, 3),
            indices: Span::new(0, 3),
            content_hash: 0xdead_beef,
        };
        let b = ShapeRecord::Mesh {
            local_rect: None,
            tint: Color {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            },
            vertices: Span::new(1234, 3),
            indices: Span::new(5678, 3),
            content_hash: 0xdead_beef,
        };
        let mut ha = FxHasher::new();
        let mut hb = FxHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }
}
