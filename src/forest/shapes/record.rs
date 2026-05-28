use crate::InternedStr;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::primitives::brush::FillAxis;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::corners::Corners;
use crate::primitives::image::{ImageFit, ImageHandle};
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::span::Span;
use crate::primitives::stroke::Stroke;
use crate::renderer::gradient_atlas::LutRow;
use crate::renderer::quad::FillKind;
use crate::shape::{ColorMode, LineCap, LineJoin, TextWrap};
use crate::text::FontFamily;
use glam::Vec2;
use half::f16;
use std::hash::Hash;

/// Frame-local handle into [`crate::common::frame_arena::FrameArena::gradients`].
/// Stable only within one frame — cleared alongside the rest of the
/// frame arena in `FrameArena::clear`.
pub(crate) type GradientId = u32;

/// Lowered fill. Solid carries 8-byte `ColorF16` (down from 16 B
/// `Color`); gradient atlas row + axis + kind are pre-baked at
/// lowering time and stored in the per-frame `FrameArena.gradients`
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

    /// True when no pixels would paint: zero/negative width or
    /// fully-transparent colour. Mirrors `Stroke::is_noop` but stays
    /// in the packed form — no f16 → f32 conversion for the colour
    /// arm (alpha is checked via the shared `noop_f16_bits` bit-trick
    /// inside `ColorF16::is_noop`).
    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        // f16 bits with sign masked, compared against the EPS bit
        // pattern. Width `≤ EPS` (or NaN, which masks above EPS) reads
        // as "noop"; matches `noop_f32`'s semantics modulo NaN.
        use crate::primitives::approx::noop_f16_bits;
        noop_f16_bits(self.width_f16) || self.color.is_noop()
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
/// `Brush`); this row keeps the same fields in their lowered forms.
/// Same lifecycle as shape records — written at `open_node` (when the
/// node carries chrome), cleared per frame. Gradient handle indexes
/// into `FrameArena.gradients` (the same arena `ShapeBrush::Gradient`
/// uses), so chrome and shape paints share storage.
///
/// `hash` is the canonical authoring fingerprint, pre-computed at
/// lowering time (`FrameArena::lower_background`) over `fill` +
/// `stroke` + `radius` + `shadow`. Read at damage diff time via the
/// chrome row of [`crate::ui::cascade::Paint`], and folded into the
/// owner node's hash in [`crate::forest::tree::Tree::compute_hashes`]
/// as a single `u64` write — no second per-chrome hash walk.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeRow {
    pub(crate) fill: ShapeBrush,
    pub(crate) stroke: ShapeStroke,
    pub(crate) corners: Corners,
    pub(crate) shadow: LoweredShadow,
    /// Canonical authoring hash. See struct docs.
    pub(crate) hash: crate::forest::rollups::NodeHash,
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

    /// Unpack the four f16 geom lanes at once via the shared SIMD
    /// helper (single `vcvtph2ps` on x86 F16C, `fcvtl` on aarch64-fp16).
    /// Bypasses `half::HalfFloatSliceExt::convert_to_f32_slice`, which
    /// gates every call on a runtime `is_x86_feature_detected!("f16c")`
    /// lookup + an out-of-line dispatch — visible at ~1.4% self in the
    /// `frame` bench.
    #[inline]
    pub(crate) fn geom(self) -> ShadowGeom {
        use crate::primitives::half_simd::f16x4_to_f32x4;
        let out = f16x4_to_f32x4(self.geom_f16);
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
    /// Quantize a user-facing `Shadow` into the packed form. Uses the
    /// shared `f16x4_from_f32x4` helper — single `vcvtps2ph` on F16C,
    /// no runtime feature dispatch per call.
    #[inline]
    fn from(s: Shadow) -> Self {
        use crate::primitives::half_simd::f16x4_from_f32x4;
        let geom_f16 = f16x4_from_f32x4([s.offset.x, s.offset.y, s.blur, s.spread]);
        Self {
            color: s.color.into(),
            geom_f16,
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

/// Pre-baked gradient stored in the per-frame arena. The stops have
/// already been registered with the gradient atlas at lowering time
/// (yielding `row`); the per-variant geometry has been packed into
/// `axis`; `kind` carries both the variant tag and the spread mode.
/// Downstream consumers (encoder, cmd buffer, composer) just pass
/// these three fields through to the GPU `Quad` — no per-encode
/// dispatch, no per-compose atlas lookup.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LoweredGradient {
    pub(crate) axis: FillAxis,
    pub(crate) row: LutRow,
    pub(crate) kind: FillKind,
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
        corners: Corners,
        fill: ShapeBrush,
        stroke: ShapeStroke,
        /// Pre-computed content hash of `fill` when it's a gradient,
        /// 0 for solid. Lets `ShapeRecord::Hash` stay context-free —
        /// otherwise we'd need to thread the `gradients` arena into
        /// every hash call (subtree rollups, measure cache). The hash
        /// is computed once at lowering time by `frame_arena::grad_hash`.
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
        /// User-facing [`InternedStr`](crate::InternedStr), moved
        /// in at lowering. No carrier is normalised away — `Borrowed`
        /// keeps the `&'static str` pointer (zero copy), `Owned`
        /// moves the `String` (no realloc, dropped at next frame's
        /// `Shapes::clear`), `Interned` carries the span+hash from
        /// [`Ui::fmt`](crate::Ui::fmt) unchanged. `text_hash` is the
        /// pre-computed FxHash for context-free `Hash for ShapeRecord`.
        text: InternedStr,
        text_hash: u64,
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
        /// Owner-local AABB of the mesh's vertex positions. Snapshot of
        /// `Mesh::bbox()` at lowering — the user-side `Mesh` does the
        /// lazy compute (and caches it across frames for retained meshes),
        /// the record just freezes the value so encoder/composer don't
        /// re-scan.
        bbox: Rect,
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
        corners: Corners,
        shadow: LoweredShadow,
    } = 4,
    /// Textured rectangle. `handle` references an entry in the shared
    /// [`ImageRegistry`](crate::ImageRegistry); the backend uploads on
    /// first sight and keeps a GPU texture across frames. `local_rect =
    /// None` paints into the owner's full arranged rect; `Some(r)`
    /// paints `r` at owner-relative coords. `tint` multiplies sampled
    /// pixels in linear-RGB premultiplied space.
    Image {
        local_rect: Option<Rect>,
        tint: ColorF16,
        /// Intrinsic dims live in `handle.size`; the encoder reads
        /// them directly with no registry borrow.
        handle: ImageHandle,
        fit: ImageFit,
    } = 5,
    /// Native GPU bezier curve. Four control points (quadratic curves
    /// promote to cubic at lowering — `p1 = p0 + 2/3(c - p0)`,
    /// `p2 = p2 + 2/3(c - p2)`). Stored owner-local; the composer adds
    /// the owner origin + active transform at compose time and uploads
    /// to a per-instance buffer. Solid stroke colour, butt caps, no
    /// joins for v1 (single-segment primitive). `bbox` is the
    /// owner-local stroked-AABB inflated by `width/2 + AA fringe` so
    /// damage / clip cull match the painted extent.
    Curve {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        width: f32,
        /// Lowered stroke fill. Solid colour stays inline; `Linear`
        /// gradient stops have been registered with the gradient atlas
        /// at lowering and ride as a `LoweredGradient` indexed by
        /// `ShapeBrush::Gradient`. The gradient is sampled along the
        /// curve parameter `t` (p0 → p3) in the shader — the
        /// `LinearGradient::angle` from authoring is intentionally
        /// ignored, because the curve carries its own 1-D parameter.
        /// `Radial`/`Conic` brushes are rejected at lowering.
        fill: ShapeBrush,
        /// Pre-computed content hash of `fill` when it's a gradient,
        /// `0` for solid — same context-free-hash trick as
        /// [`ShapeRecord::RoundedRect.fill_grad_hash`].
        fill_grad_hash: u64,
        /// End-cap style. Joins are absent (single-curve primitive,
        /// no interior). `Round`/`Square` extend the painted strip by
        /// `width/2` past each endpoint along the local tangent; the
        /// composer-time bbox already includes that slack.
        cap: LineCap,
        bbox: Rect,
        content_hash: u64,
    } = 6,
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
    /// beyond the owner via [`shadow_paint_rect_local`]; `Polyline` /
    /// `Curve` carry a pre-computed owner-relative bbox; the rest
    /// paint into `local_rect` (when set) or the owner's full rect at
    /// `(0, 0)`. Does **not** handle `Text` — its bbox depends on the
    /// shaped extent from the layout pass and is computed by
    /// [`text_paint_bbox_local`], which cascade calls directly.
    #[inline]
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
            ShapeRecord::Polyline { bbox, .. } | ShapeRecord::Curve { bbox, .. } => *bbox,
            ShapeRecord::RoundedRect { local_rect, .. }
            | ShapeRecord::Mesh { local_rect, .. }
            | ShapeRecord::Image { local_rect, .. } => local_rect.unwrap_or(Rect {
                min: Vec2::ZERO,
                size: owner_size,
            }),
            // Cascade dispatches Text to `text_paint_bbox_local`
            // before reaching this method — a direct call here would
            // silently lose the shaped extent.
            ShapeRecord::Text { .. } => {
                unreachable!("Text shapes resolve via text_paint_bbox_local in cascade")
            }
        }
    }

    /// Stable on-disk tag. Used as the discriminant byte in the
    /// `Hash` impl, which feeds subtree hashes / cache keys. The
    /// values match the `= N` annotations on the variants — never
    /// edit one without the other.
    pub(crate) const fn tag(&self) -> u8 {
        match self {
            ShapeRecord::RoundedRect { .. } => 0,
            ShapeRecord::Polyline { .. } => 1,
            ShapeRecord::Text { .. } => 2,
            ShapeRecord::Mesh { .. } => 3,
            ShapeRecord::Shadow { .. } => 4,
            ShapeRecord::Image { .. } => 5,
            ShapeRecord::Curve { .. } => 6,
        }
    }
}

/// Tight owner-local paint bbox of a [`ShapeRecord::Text`], using the
/// shaped extent the measure pass already computed (lives in
/// `LayerLayout::text_shapes`). The encoder applies the same formula
/// in screen space — `text_in_rect` is the sole source so cascade
/// damage rects and encoder draw rects can't drift.
///
/// **Damage inflation lives in cascade** (`ui::cascade`), not here —
/// the ladder-snap overshoot is in absolute screen pixels
/// (`measured × STEP/2`) regardless of the ancestor scale, so it must
/// be applied to the screen rect *after* `lift_to_screen` rather than
/// to the local rect *before* it. Inflating in local coords would
/// produce a screen pad of `measured × STEP/2 × cascade_scale`, which
/// underflows at `cascade_scale < 1` (zoomed-out content) and lets
/// long lines bleed past the damage region.
///
/// - `local_origin: Some(origin)` ⇒ widget owns positioning; rect is
///   `origin + measured`.
/// - `local_origin: None` ⇒ encoder owns positioning via
///   [`text_in_rect`] against the owner's padded inner rect.
pub(crate) fn text_paint_bbox_local(
    local_origin: Option<Vec2>,
    align: Align,
    padding: Spacing,
    owner_size: Size,
    measured: Size,
) -> Rect {
    match local_origin {
        Some(origin) => Rect {
            min: origin,
            size: measured,
        },
        None => {
            let owner_local = Rect {
                min: Vec2::ZERO,
                size: owner_size,
            };
            text_in_rect(owner_local.deflated_by(padding), measured, align)
        }
    }
}

/// Position a text run's bounding box inside `leaf` per `align`.
/// Returns a rect with `min` shifted by the alignment offset and
/// `size` set to the measured text bbox — composer takes `min` as
/// the glyph origin and `size` as the clip bounds. Glyphs don't
/// stretch, so `Auto`/`Stretch` collapse to start (top-left) —
/// matches `place_axis`'s behavior for non-stretchable content.
///
/// Coordinate-system agnostic: callers pass owner-local for the
/// cascade's damage rect and screen-space for the encoder's draw
/// rect.
pub(crate) fn text_in_rect(leaf: Rect, measured: Size, align: Align) -> Rect {
    let dx = match align.halign() {
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
        HAlign::Center => (leaf.size.w - measured.w) * 0.5,
        HAlign::Right => leaf.size.w - measured.w,
    };
    let dy = match align.valign() {
        VAlign::Auto | VAlign::Top | VAlign::Stretch => 0.0,
        VAlign::Center => (leaf.size.h - measured.h) * 0.5,
        VAlign::Bottom => leaf.size.h - measured.h,
    };
    Rect::new(
        leaf.min.x + dx.max(0.0),
        leaf.min.y + dy.max(0.0),
        measured.w,
        measured.h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forest::shapes::hash::compute_record_hash;

    #[test]
    fn shape_mesh_hash_excludes_span_offsets() {
        let tint = ColorF16::from(Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        });
        let a = ShapeRecord::Mesh {
            local_rect: None,
            tint,
            vertices: Span::new(0, 3),
            indices: Span::new(0, 3),
            bbox: crate::primitives::rect::Rect::ZERO,
            content_hash: 0xdead_beef,
        };
        let b = ShapeRecord::Mesh {
            local_rect: None,
            tint,
            vertices: Span::new(1234, 3),
            indices: Span::new(5678, 3),
            bbox: crate::primitives::rect::Rect::ZERO,
            content_hash: 0xdead_beef,
        };
        assert_eq!(compute_record_hash(&a), compute_record_hash(&b));
    }

    #[test]
    fn shape_image_hash_distinguishes_handle_and_tint() {
        let make = |handle: u64, tint: Color| ShapeRecord::Image {
            local_rect: None,
            tint: ColorF16::from(tint),
            handle: ImageHandle {
                id: handle,
                size: glam::U16Vec2::new(64, 64),
            },
            fit: ImageFit::Fill,
        };
        let baseline = compute_record_hash(&make(0xa, Color::WHITE));
        assert_ne!(baseline, compute_record_hash(&make(0xb, Color::WHITE)));
        assert_ne!(
            baseline,
            compute_record_hash(&make(0xa, Color::rgba(1.0, 0.0, 0.0, 1.0)))
        );
    }
}
