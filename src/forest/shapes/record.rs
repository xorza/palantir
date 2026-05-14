use crate::layout::types::align::Align;
use crate::layout::types::span::Span;
use crate::primitives::brush::{ConicGradient, LinearGradient, RadialGradient};
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use crate::primitives::stroke::Stroke;
use crate::shape::{ColorMode, LineCap, LineJoin, TextWrap};
use crate::text::FontFamily;
use glam::Vec2;
use half::f16;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};

/// Frame-local handle into [`crate::forest::shapes::Shapes::gradients`].
/// Stable only within one frame — cleared alongside the rest of the
/// shape buffer in `Shapes::clear`.
pub(crate) type GradientId = u32;

/// Lowered fill. Solid carries 8-byte `ColorF16` (down from 16 B
/// `Color`); gradient geometry lives in the per-frame `gradients`
/// arena via an index. The encoder/composer pipe `ColorF16` straight
/// to the GPU vertex attribute (`Uint32x2` + `unpack2x16float`) —
/// stays linear end-to-end, no sRGB cubic anywhere.
#[derive(Clone, Copy, Debug, Hash)]
pub(crate) enum ShapeBrush {
    Solid(ColorF16),
    Gradient(GradientId),
}

/// Lowered stroke. Gradient strokes are a non-goal — every downstream
/// consumer already calls `Brush::expect_solid()`. Storage is packed:
/// `ColorF16` (8 B linear-RGB) + f16 width (2 B) = **10 B**, align 2.
/// Lossy storage is fine: strokes don't animate inside the row
/// (frame-local snapshot of the user-space animation output) and
/// f16 precision is well below display quantization.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShapeStroke {
    pub(crate) color: ColorF16,
    pub(crate) width_f16: u16,
}

impl ShapeStroke {
    #[inline]
    pub(crate) fn width(self) -> f32 {
        f16::from_bits(self.width_f16).to_f32()
    }
}

impl From<Stroke> for ShapeStroke {
    #[inline]
    fn from(s: Stroke) -> Self {
        Self {
            color: ColorF16::from(s.brush.expect_solid()),
            width_f16: f16::from_f32(s.width).to_bits(),
        }
    }
}

impl From<ShapeStroke> for Stroke {
    #[inline]
    fn from(s: ShapeStroke) -> Self {
        Stroke::solid(Color::from(s.color), s.width())
    }
}

/// Lowered chrome row stored in `Tree.chrome_table`. The user-facing
/// `Background` is ~232 B (inline `Brush` + `Stroke` with inline
/// `Brush`); this row keeps the same fields in their lowered forms,
/// shrinking the per-chrome footprint to ~96 B. Same lifecycle as
/// shape records — written at `open_node_with_chrome`, cleared per
/// frame. Gradient handle indexes into the same `Shapes.gradients`
/// arena `ShapeBrush::Gradient` uses, so chrome and shape paints
/// share storage.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeRow {
    pub(crate) fill: ShapeBrush,
    pub(crate) stroke: ShapeStroke,
    pub(crate) radius: Corners,
    pub(crate) shadow: LoweredShadow,
    /// Pre-computed content hash for `fill` when it's a gradient, 0
    /// for solid — same context-free-Hash trick as
    /// `ShapeRecord::RoundedRect.fill_grad_hash`. Lets
    /// `ChromeRow::Hash` work without threading the gradient arena.
    pub(crate) fill_grad_hash: u64,
}

impl Hash for ChromeRow {
    fn hash<H: Hasher>(&self, h: &mut H) {
        match self.fill {
            ShapeBrush::Solid(c) => {
                h.write_u8(0);
                c.hash(h);
            }
            ShapeBrush::Gradient(_) => {
                h.write_u8(1);
                h.write_u64(self.fill_grad_hash);
            }
        }
        h.write(bytemuck::bytes_of(&self.stroke));
        self.radius.hash(h);
        self.shadow.hash(h);
    }
}

/// Lowered shadow. The user-facing `Shadow` is 36 B (linear `Color` +
/// 2 `Vec2`s + 2 `f32`s + bool); this stores the same authoring data
/// in 18 B via `ColorF16` and a 4-lane f16 geom block. Used in
/// `ChromeRow.shadow` and `ShapeRecord::Shadow`. Same lifecycle as
/// the rest of the row.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LoweredShadow {
    pub(crate) color: ColorF16,
    /// `[offset.x, offset.y, blur, spread]` as f16 lanes. Unpacked
    /// at read time via the batched slice path — single SIMD on
    /// targets with hardware f16 support.
    pub(crate) geom_f16: [u16; 4],
    /// Inset flag stored as u16 (not bool) so the struct has no
    /// padding bytes — required for `Pod`.
    pub(crate) inset_flag: u16,
}

/// Unpacked f16 geom lanes from `LoweredShadow`. Single batched-SIMD
/// unpack feeds all three named fields; consumers destructure rather
/// than juggle a `[f32; 4]`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowGeom {
    pub(crate) offset: Vec2,
    pub(crate) blur: f32,
    pub(crate) spread: f32,
}

impl LoweredShadow {
    /// True when no pixels would paint — same gate as `Shadow::is_noop`,
    /// keyed on the alpha lane via `approx::noop_f16_bits` (shared
    /// bit-trick — see that fn for the IEEE 754 rationale).
    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        crate::primitives::approx::noop_f16_bits(self.color.0[3])
    }

    /// Unpack the four f16 geom lanes at once via the batched slice
    /// path and return them as named fields.
    #[inline]
    pub(crate) fn geom(self) -> ShadowGeom {
        use half::slice::HalfFloatSliceExt;
        let arr: &[half::f16; 4] = bytemuck::cast_ref(&self.geom_f16);
        let mut out = [0.0f32; 4];
        arr.as_slice().convert_to_f32_slice(&mut out);
        ShadowGeom {
            offset: Vec2::new(out[0], out[1]),
            blur: out[2],
            spread: out[3],
        }
    }

    #[inline]
    pub(crate) fn inset(self) -> bool {
        self.inset_flag != 0
    }
}

impl From<Shadow> for LoweredShadow {
    /// Quantize a user-facing `Shadow` into the packed form. f16 pack
    /// is one batched SIMD on F16C/fp16 targets.
    #[inline]
    fn from(s: Shadow) -> Self {
        use half::slice::HalfFloatSliceExt;
        let src = [s.offset.x, s.offset.y, s.blur, s.spread];
        let mut out = [half::f16::ZERO; 4];
        out.as_mut_slice().convert_from_f32_slice(&src);
        Self {
            color: s.color.into(),
            geom_f16: bytemuck::cast(out),
            inset_flag: s.inset as u16,
        }
    }
}

impl std::hash::Hash for LoweredShadow {
    /// One `write()` over the 18 storage bytes. Pod-friendly, single
    /// hasher dispatch.
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

/// Gradient variant stored in the per-frame arena. Same three shapes
/// as `Brush::{Linear,Radial,Conic}` — kept as an enum (rather than
/// three separate arenas) so a single `GradientId` indexes any kind
/// and downstream consumers (`pack_brush`, atlas registration) branch
/// on the variant.
#[derive(Clone, Copy, Debug)]
pub(crate) enum GradientPayload {
    Linear(LinearGradient),
    Radial(RadialGradient),
    Conic(ConicGradient),
}

impl GradientPayload {
    /// Stable content hash for cache identity. Calls each gradient's
    /// own `Hash` impl (`canon_bits` on the f32 fields), so two frames
    /// with identical gradient authoring inputs produce the same hash
    /// even though their `GradientId`s differ across frames.
    pub(crate) fn content_hash(&self) -> u64 {
        use crate::common::hash::Hasher as FxHasher;
        let mut h = FxHasher::new();
        match self {
            GradientPayload::Linear(g) => {
                h.write_u8(0);
                g.hash(&mut h);
            }
            GradientPayload::Radial(g) => {
                h.write_u8(1);
                g.hash(&mut h);
            }
            GradientPayload::Conic(g) => {
                h.write_u8(2);
                g.hash(&mut h);
            }
        }
        h.finish()
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
        fill: ShapeBrush,
        stroke: ShapeStroke,
        /// Pre-computed content hash of `fill` when it's a gradient,
        /// 0 for solid. Lets `ShapeRecord::Hash` stay context-free —
        /// otherwise we'd need to thread the `gradients` arena into
        /// every hash call (subtree rollups, measure cache). The hash
        /// is computed once at lowering time via
        /// `GradientPayload::content_hash`.
        fill_grad_hash: u64,
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
    /// `None` paints into the owner's arranged rect (deflated by the
    /// node's `padding`) and `align` positions the glyph bbox inside
    /// it. `Some(origin)` paints at `owner.min + origin` with the
    /// shaped measurement as the bbox — the encoder is a passthrough
    /// for positioning. Lets a widget shift the run by
    /// scroll/alignment offsets that depend on shaped-buffer state.
    Text {
        local_origin: Option<Vec2>,
        /// `Cow<'static, str>` so static-string labels (the common case via
        /// `&'static str → Into<Cow<…>>`) round-trip with only pointer-copy
        /// `Clone`s — no per-frame heap alloc. Dynamic strings still allocate
        /// once into `Cow::Owned` at the authoring boundary.
        text: Cow<'static, str>,
        color: ColorF16,
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
        family: FontFamily,
    } = 2,
    /// User-supplied colored triangle mesh. Vertex/index data lives in
    /// the active `Tree`'s `mesh_vertices` / `mesh_indices` arenas;
    /// these spans index into them. `content_hash` summarizes
    /// vertex+index bytes for cache identity — two frames with
    /// identical mesh content share a hash even though their span
    /// offsets differ.
    Mesh {
        local_rect: Option<Rect>,
        tint: ColorF16,
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
        shadow: LoweredShadow,
    } = 4,
}

/// Owner-local paint bbox of a [`ShapeRecord::Shadow`] — drop shadow
/// inflated by `|offset| + 3σ + max(spread, 0)` per axis from the
/// source rect; inset shadow stays inside the source. `local_rect =
/// None` ⇒ source covers the full owner; `Some(r)` ⇒ source is `r` at
/// owner-relative coords. **Sole formula source** for the shadow paint
/// extent: the encoder (per-quad paint rect) and [`ShapeRecord::paint_bbox_local`]
/// (cascade's per-node ink union) both call this so the two views can't drift.
pub(crate) fn shadow_paint_rect_local(
    local_rect: Option<Rect>,
    owner_size: Size,
    offset: Vec2,
    blur: f32,
    spread: f32,
    inset: bool,
) -> Rect {
    let source = match local_rect {
        None => Rect {
            min: Vec2::ZERO,
            size: owner_size,
        },
        Some(r) => r,
    };
    if inset {
        return source;
    }
    let dx = offset.x.abs() + 3.0 * blur.max(0.0) + spread.max(0.0);
    let dy = offset.y.abs() + 3.0 * blur.max(0.0) + spread.max(0.0);
    Rect {
        min: Vec2::new(source.min.x - dx, source.min.y - dy),
        size: Size::new(source.size.w + 2.0 * dx, source.size.h + 2.0 * dy),
    }
}

impl ShapeRecord {
    /// Owner-local paint bbox this shape draws into — cascade unions
    /// across siblings to derive `Cascade.paint_rect`, encoder reads
    /// the world-space form for damage culling. Drop shadows extend
    /// beyond the owner via [`shadow_paint_rect_local`]; other variants
    /// paint into `local_rect` (when set) or the owner's full rect at
    /// `(0, 0)`. `Polyline` carries a pre-computed owner-relative bbox
    /// from `lower_polyline`.
    pub(crate) fn paint_bbox_local(&self, owner_size: Size) -> Rect {
        match self {
            ShapeRecord::Shadow {
                local_rect, shadow, ..
            } => {
                let ShadowGeom {
                    offset,
                    blur,
                    spread,
                } = shadow.geom();
                shadow_paint_rect_local(
                    *local_rect,
                    owner_size,
                    offset,
                    blur,
                    spread,
                    shadow.inset(),
                )
            }
            ShapeRecord::Polyline { bbox, .. } => *bbox,
            ShapeRecord::RoundedRect { local_rect, .. } | ShapeRecord::Mesh { local_rect, .. } => {
                local_rect.unwrap_or(Rect {
                    min: Vec2::ZERO,
                    size: owner_size,
                })
            }
            // `Text` carries an origin-only override — glyph extent
            // depends on shaped-buffer measurement which paint_bbox
            // doesn't have. Width reports zero on the `Some(origin)`
            // path; cascade falls back to the leaf's own arranged
            // rect for the damage union.
            ShapeRecord::Text { local_origin, .. } => match local_origin {
                None => Rect {
                    min: Vec2::ZERO,
                    size: owner_size,
                },
                Some(origin) => Rect {
                    min: *origin,
                    size: Size::ZERO,
                },
            },
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
                fill_grad_hash,
            } => {
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                radius.hash(h);
                match fill {
                    ShapeBrush::Solid(c) => {
                        h.write_u8(0);
                        c.hash(h);
                    }
                    ShapeBrush::Gradient(_) => {
                        h.write_u8(1);
                        h.write_u64(*fill_grad_hash);
                    }
                }
                // Pod-byte hash: one `write()` call for `(color, width)` —
                // 20 bytes in, single hasher dispatch.
                h.write(bytemuck::bytes_of(stroke));
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
                local_origin,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
                family,
            } => {
                match local_origin {
                    None => h.write_u8(0),
                    Some(o) => {
                        h.write_u8(1);
                        h.write_u32(o.x.to_bits());
                        h.write_u32(o.y.to_bits());
                    }
                }
                text.hash(h);
                color.hash(h);
                h.write_u32(font_size_px.to_bits());
                h.write_u32(line_height_px.to_bits());
                h.write_u16(((align.raw() as u16) << 8) | *wrap as u8 as u16);
                h.write_u8(*family as u8);
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
                shadow,
            } => {
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                radius.hash(h);
                shadow.hash(h);
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
            tint: ColorF16::from(Color {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            }),
            vertices: Span::new(0, 3),
            indices: Span::new(0, 3),
            content_hash: 0xdead_beef,
        };
        let b = ShapeRecord::Mesh {
            local_rect: None,
            tint: ColorF16::from(Color {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            }),
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
