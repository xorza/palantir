use crate::layout::types::align::Align;
use crate::layout::types::span::Span;
use crate::primitives::brush::Brush;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::stroke::Stroke;
use crate::shape::{ColorMode, LineCap, LineJoin, TextWrap};
use glam::Vec2;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};

/// Per-side ink overhang in owner-local px — how far this shape paints
/// **beyond** the owner's arranged rect on each side. Non-zero only for
/// drop shadows today; everything else paints within the owner rect (or
/// inside an explicit `local_rect` that itself sits inside the owner).
/// Cascade folds these per-node into `paint_rect` so damage tracking
/// covers the pixels the encoder actually touches.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Overhang {
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) right: f32,
    pub(crate) bottom: f32,
}

impl Overhang {
    pub(crate) const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    pub(crate) fn is_zero(self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }
}

/// Discriminants pinned via `#[repr(u8)]` + explicit `= N` so cache
/// keys (which write the discriminant into the hash) stay stable
/// across variant reordering. Reorder freely — the on-disk tag
/// follows the `= N`, not the source order. Adding a variant forces
/// the [`Self::tag`] match and the [`Hash`] match to grow; pick the
/// next free number.
#[repr(u8)]
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
        fill: Brush,
        stroke: Stroke,
    } = 0,
    /// Stroked polyline. `points`/`colors` index into the active
    /// tree's `polyline_points` / `polyline_colors` arenas. `colors`
    /// length depends on `color_mode`: 1 for `Single`,
    /// `points.len()` for `PerPoint`, `points.len() - 1` for
    /// `PerSegment`. `content_hash` summarizes points+colors+mode
    /// +cap+join bytes for cache identity. `bbox` is the
    /// axis-aligned bounds of `points` in owner-relative coords —
    /// the encoder translates it into cmd-buffer coords by adding
    /// the owner rect origin. `cap` and `join` are user-picked
    /// stroke-style enums; tessellator branches on them.
    Polyline {
        width: f32,
        color_mode: ColorMode,
        cap: LineCap,
        join: LineJoin,
        points: Span,
        colors: Span,
        bbox: Rect,
        content_hash: u64,
    } = 1,
    /// Shaped text run — *authoring inputs only*. Measured size and
    /// shaped-buffer key are layout outputs and live on
    /// `Layout.text_shapes`, not here. `wrap` selects between "shape
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
    } = 2,
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
    } = 3,
    /// Gaussian-blurred rounded rect — drop / inset shadow. All
    /// parameters are inline scalars; no payload arena. With
    /// `local_rect = None` the shadow shadows the owner's full
    /// arranged rect; with `Some(r)` it shadows the owner-relative
    /// rect `r`. Encoder inflates the paint bbox by
    /// `|offset| + 3σ + spread` and routes through the existing
    /// `DrawRect` cmd with `FillKind::SHADOW_DROP|SHADOW_INSET`.
    Shadow {
        local_rect: Option<Rect>,
        radius: Corners,
        color: Color,
        offset: Vec2,
        blur: f32,
        spread: f32,
        inset: bool,
    } = 4,
}

impl ShapeRecord {
    /// Per-side overhang beyond the owner's arranged rect (owner-local
    /// px). Only drop shadows extend outside; everything else returns
    /// [`Overhang::ZERO`]. The drop-shadow formula matches the
    /// `paint_rect` the encoder hands to `draw_shadow`
    /// (`offset.abs() + 3σ + max(spread, 0)` per axis), so damage and
    /// paint can't drift.
    pub(crate) fn paint_overhang_local(&self, owner_size: Size) -> Overhang {
        match self {
            ShapeRecord::Shadow {
                local_rect,
                offset,
                blur,
                spread,
                inset: false,
                ..
            } => {
                let dx = offset.x.abs() + 3.0 * blur.max(0.0) + spread.max(0.0);
                let dy = offset.y.abs() + 3.0 * blur.max(0.0) + spread.max(0.0);
                let (sx, sy, sw, sh) = match local_rect {
                    None => (0.0, 0.0, owner_size.w, owner_size.h),
                    Some(r) => (r.min.x, r.min.y, r.size.w, r.size.h),
                };
                Overhang {
                    left: (dx - sx).max(0.0),
                    top: (dy - sy).max(0.0),
                    right: (sx + sw + dx - owner_size.w).max(0.0),
                    bottom: (sy + sh + dy - owner_size.h).max(0.0),
                }
            }
            _ => Overhang::ZERO,
        }
    }

    /// Stable on-disk tag. Used as the discriminant byte in the
    /// `Hash` impl, which feeds subtree hashes / cache keys. The
    /// values match the `= N` annotations on the variants — never
    /// edit one without the other.
    const fn tag(&self) -> u8 {
        match self {
            ShapeRecord::RoundedRect { .. } => 0,
            ShapeRecord::Polyline { .. } => 1,
            ShapeRecord::Text { .. } => 2,
            ShapeRecord::Mesh { .. } => 3,
            ShapeRecord::Shadow { .. } => 4,
        }
    }
}

impl Hash for ShapeRecord {
    /// Discriminant tags come from [`ShapeRecord::tag`] and are pinned
    /// via `#[repr(u8)]` + explicit `= N` on each variant, so cache
    /// keys don't shift if variants are reordered.
    fn hash<H: Hasher>(&self, h: &mut H) {
        h.write_u8(self.tag());
        match self {
            ShapeRecord::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            } => {
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
            ShapeRecord::Polyline { content_hash, .. } => {
                // `content_hash` already covers width + color_mode +
                // cap + join + points + colors (computed in
                // `ShapePayloads::lower_polyline` / `lower_bezier`).
                // bbox is derived from points; spans are frame-local —
                // neither belongs in cache identity.
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
            ShapeRecord::Shadow {
                local_rect,
                radius,
                color,
                offset,
                blur,
                spread,
                inset,
            } => {
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                radius.hash(h);
                color.hash(h);
                h.write_u32(offset.x.to_bits());
                h.write_u32(offset.y.to_bits());
                h.write_u32(blur.to_bits());
                h.write_u32(spread.to_bits());
                h.write_u8(*inset as u8);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::hash::Hasher as FxHasher;

    #[test]
    fn shape_mesh_hash_excludes_span_offsets() {
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
