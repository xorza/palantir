use crate::forest::shapes::paint::{LoweredShadow, ShadowGeom, ShapeBrush, ShapeStroke};
use crate::layout::types::align::Align;
use crate::primitives::color::ColorF16;
use crate::primitives::corners::Corners;
use crate::primitives::image::{ImageFilter, ImageFit};
use crate::primitives::interned_str::RecordedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::span::Span;
use crate::renderer::texture_id::TextureId;
use crate::shape::{ColorMode, LineCap, LineJoin, TextWrap};
use crate::text::text_in_rect;
use crate::text::{FontFamily, FontWeight};
use glam::Vec2;

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
        /// otherwise we'd need to thread the gradient payloads into
        /// every hash call (subtree rollups, measure cache). The hash
        /// is computed once at lowering time by `record_store::grad_hash`.
        fill_grad_hash: u64,
    } = 0,
    /// Stroked polyline. `points`/`colors` index into the
    /// `RecordPayloads`' `polyline_points` / `polyline_colors`. `colors`
    /// length depends on `color_mode`: 1 for `Single`,
    /// `points.len()` for `PerPoint`, `points.len() - 1` for
    /// `PerSegment`. `content_hash` summarizes points+colors+mode
    /// +cap+join bytes for cache identity. `bbox` is the centerline
    /// AABB of `points` in owner-relative coords; damage and composition
    /// apply the shared raster-aware stroke inflation. `cap` and `join`
    /// are user-picked stroke-style enums consumed by the composer and
    /// stroke shader.
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
    /// positions the glyph bbox inside the owner node's arranged rect (or
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
        /// Lowered text storage and its pre-computed content hash. Arena-backed
        /// input is normalized into the active record store before this value
        /// is built, so its span and hash cannot belong to different passes.
        text: RecordedText,
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
        weight: FontWeight,
    } = 2,
    /// User-supplied colored triangle mesh. Vertex/index data lives on
    /// the `RecordPayloads`' `meshes` pool; these spans index into its
    /// vertex/index vecs. `content_hash` summarizes
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
    /// parameters are inline scalars; no retained payloads. With
    /// `local_rect = None` the shadow shadows the owner's full
    /// arranged rect; with `Some(r)` it shadows the owner-relative
    /// rect `r`. Encoder shifts drop-shadow paint bounds by `offset`,
    /// inflates them by `3σ + max(spread, 0)`, and routes both shadow
    /// kinds through `DrawShadow` with `FillKind::SHADOW_DROP|SHADOW_INSET`.
    Shadow {
        local_rect: Option<Rect>,
        corners: Corners,
        shadow: LoweredShadow,
    } = 4,
    /// Textured rectangle. `id` is the registration id behind an
    /// [`ImageHandle`](crate::ImageHandle) — extracted at lowering so the
    /// per-frame record carries no `Rc` (the user's held handle is what
    /// keeps the GPU texture alive). The backend looks `id` up in its
    /// texture cache and skips the draw on a miss. `local_rect = None`
    /// paints into the owner's full arranged rect; `Some(r)` paints `r`
    /// at owner-relative coords. `tint` multiplies sampled pixels in
    /// linear-RGB premultiplied space.
    Image {
        local_rect: Option<Rect>,
        tint: ColorF16,
        id: TextureId,
        /// Intrinsic dims, baked in at registration so the encoder reads
        /// them with no registry borrow.
        size: glam::U16Vec2,
        fit: ImageFit,
        min_filter: ImageFilter,
        mag_filter: ImageFilter,
    } = 5,
    /// Native GPU bezier curve. Four control points (quadratics
    /// promote to cubic at lowering, lines degenerate to one — see
    /// `shapes::lower`). Stored owner-local; the
    /// composer adds the owner origin + active transform at compose
    /// time and uploads to a per-instance buffer. No joins
    /// (single-segment primitive); `fill` and `cap` are documented on
    /// their fields. `bbox` is the tight owner-local centerline AABB;
    /// damage and composition apply the shared raster-aware stroke
    /// inflation after transforms are known.
    Curve {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        width: f32,
        /// Lowered stroke fill. Solid colour stays inline; `Linear`
        /// gradient content rides as a `RecordedGradient` indexed by
        /// `ShapeBrush::Gradient` and resolves its atlas row on encode.
        /// The gradient is sampled along the curve parameter `t` (p0 →
        /// p3) in the shader — the `LinearGradient::angle` from authoring
        /// is intentionally ignored, because the curve carries its own
        /// 1-D parameter. The authoring type cannot contain radial or
        /// conic gradients.
        fill: ShapeBrush,
        /// Pre-computed content hash of `fill` when it's a gradient,
        /// `0` for solid — same context-free-hash trick as
        /// [`ShapeRecord::RoundedRect.fill_grad_hash`].
        fill_grad_hash: u64,
        /// End-cap style. Joins are absent (single-curve primitive,
        /// no interior). `Round`/`Square` extend the painted strip by
        /// `width/2` past each endpoint along the local tangent.
        cap: LineCap,
        bbox: Rect,
    } = 6,
    /// Filled/stroked rounded triangle, rendered as an analytic SDF on the
    /// shared quad pipeline (`FillKind::TRIANGLE`). `a`/`b`/`c` are owner-local
    /// corner points; the composer transforms them to physical px, packs them
    /// into the reused `Quad` corner/axis lanes, and the shader evaluates
    /// `sdf_triangle - radius` for rounded corners + coverage AA. Solid fill
    /// only (gradients don't fit the reused lanes). `bbox` is the owner-local
    /// AABB inflated by `radius + AA fringe` for damage / cull (the stroke is
    /// inner-edge, so it adds no outward reach).
    Triangle {
        a: Vec2,
        b: Vec2,
        c: Vec2,
        radius: f32,
        /// Solid linear-RGB fill (straight alpha).
        fill: ColorF16,
        stroke: ShapeStroke,
        bbox: Rect,
    } = 8,
    /// Inverse-fill sibling of [`ShapeRecord::RoundedRect`]: same fields, same
    /// positioning rules, but the fill paints the complement of the rounded
    /// shape (the corner wedges out to the rect edge) while the interior stays
    /// transparent; the stroke keeps its inner-edge annulus. Lowered from
    /// [`crate::shape::Shape::WindowedRect`]; the encoder routes it to
    /// `draw_rect_window`, which tags the payload's `FillKind` with the window
    /// bit — downstream it rides the ordinary `DrawRect` path.
    WindowedRect {
        local_rect: Option<Rect>,
        corners: Corners,
        fill: ShapeBrush,
        stroke: ShapeStroke,
        /// See [`ShapeRecord::RoundedRect.fill_grad_hash`].
        fill_grad_hash: u64,
    } = 9,
    /// Native GPU circular arc — the exact-circle sibling of
    /// [`ShapeRecord::Curve`], sharing its pipeline, cap model, and
    /// gradient-along-t sampling. `center` is owner-local; `a0`/`a1`
    /// are the start/end angles in radians (screen convention: 0 = +x,
    /// y-down ⇒ increasing = clockwise; `a1 < a0` for a negative
    /// sweep). `bbox` is the tight owner-local centerline AABB, using
    /// the same deferred stroke-bound contract as `Curve`.
    Arc {
        center: Vec2,
        radius: f32,
        a0: f32,
        a1: f32,
        width: f32,
        /// See [`ShapeRecord::Curve::fill`] — same solid/linear-only
        /// contract, gradient sampled along the sweep.
        fill: ShapeBrush,
        fill_grad_hash: u64,
        cap: LineCap,
        bbox: Rect,
    } = 10,
    /// App-rendered GPU surface. Carries only the redraw `epoch` — the view's
    /// stable render-target [`TextureId`] + the app `paint` callback live in
    /// `Ui::gpu_views`, keyed by the owner node's `WidgetId`, which the encoder
    /// reads to look the view up (kept off the shape so the hot `records`
    /// buffer stays small and `Rc`-free). Composited exactly like
    /// [`ShapeRecord::Image`] (the encoder lowers it to the same `DrawImage`
    /// cmd over the owner's full arranged rect), so it reuses the image pipeline
    /// end to end. `epoch` is the view's damage version, folded into the shape
    /// hash (which only sees the `ShapeRecord`, so it rides here):
    /// `Ui::gpu_view` bumps it to the frame id on `repaint(true)` — the rect
    /// repaints and the texture re-renders — and holds it stable on
    /// `.repaint(false)`, so a static view stays undamaged and is culled.
    GpuView { epoch: u64 } = 7,
}

/// Owner-local paint bbox of a [`ShapeRecord::Shadow`] — a drop shadow is
/// the offset source inflated by `3σ + max(spread, 0)`; an inset shadow
/// stays inside the source. `local_rect = None` ⇒ source covers the full
/// owner; `Some(r)` ⇒ source is `r` at owner-relative coords. **Sole formula
/// source** for the shadow paint extent: the encoder (per-quad paint rect) and
/// [`ShapeRecord::bbox_local`] (cascade's per-node ink union) both call
/// this so the two views can't drift.
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
    let halo = 3.0 * blur.max(0.0) + spread.max(0.0);
    Rect {
        min: source.min + offset,
        size: source.size,
    }
    .inflated(halo)
}

impl ShapeRecord {
    /// Owner-local bbox used as the basis for cascade's screen-space paint
    /// bound. `Polyline` / `Curve` / `Arc` return their tight centerline
    /// bbox because stroke width and the physical-pixel AA fringe are applied
    /// after the bbox reaches screen space. Drop shadows include their full
    /// local extent via [`shadow_paint_rect_local`]; the remaining shapes
    /// return their paint bbox directly. Does **not** handle `Text` — its bbox
    /// depends on the shaped extent from the layout pass and is computed by
    /// [`text_paint_bbox_local`], which cascade calls directly.
    #[inline]
    pub(crate) fn bbox_local(&self, owner_size: Size) -> Rect {
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
            ShapeRecord::Polyline { bbox, .. }
            | ShapeRecord::Curve { bbox, .. }
            | ShapeRecord::Arc { bbox, .. } => *bbox,
            ShapeRecord::Triangle { bbox, .. } => *bbox,
            // A mesh's vertex hull can exceed the owner rect (rotated /
            // overflowing meshes), so it must report that hull — like
            // `Polyline` / `Curve` — or partial damage clips the overflow.
            // `local_rect` only *offsets* the mesh (its size is the vertex
            // hull, not `local_rect.size`), so translate the bbox by its min.
            ShapeRecord::Mesh {
                bbox, local_rect, ..
            } => {
                let origin = local_rect.map_or(Vec2::ZERO, |r| r.min);
                Rect {
                    min: bbox.min + origin,
                    size: bbox.size,
                }
            }
            ShapeRecord::RoundedRect { local_rect, .. }
            | ShapeRecord::WindowedRect { local_rect, .. }
            | ShapeRecord::Image { local_rect, .. } => local_rect.unwrap_or(Rect {
                min: Vec2::ZERO,
                size: owner_size,
            }),
            // Always paints the owner's full arranged rect.
            ShapeRecord::GpuView { .. } => Rect {
                min: Vec2::ZERO,
                size: owner_size,
            },
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
            ShapeRecord::GpuView { .. } => 7,
            ShapeRecord::Triangle { .. } => 8,
            ShapeRecord::WindowedRect { .. } => 9,
            ShapeRecord::Arc { .. } => 10,
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
///   [`crate::text::text_in_rect`] against the owner's padded inner
///   rect.
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

#[cfg(test)]
mod tests {
    use crate::forest::shapes::hash::compute_record_hash;
    use crate::forest::shapes::record::*;
    use crate::primitives::approx::EPS;
    use crate::primitives::color::Color;
    use crate::primitives::rect::Rect;
    use crate::primitives::size::Size;
    use crate::primitives::stroke::Stroke;
    use glam::Vec2;

    #[test]
    fn shadow_paint_bbox_tracks_shifted_drop_and_source_bounded_inset() {
        #[derive(Debug)]
        struct DropCase {
            offset: Vec2,
            blur: f32,
            spread: f32,
            expected: Rect,
        }

        let source = Rect::new(10.0, 20.0, 30.0, 40.0);
        let cases = [
            DropCase {
                offset: Vec2::new(12.0, 7.0),
                blur: 4.0,
                spread: 2.0,
                expected: Rect::new(8.0, 13.0, 58.0, 68.0),
            },
            DropCase {
                offset: Vec2::new(-9.0, -11.0),
                blur: 3.0,
                spread: 5.0,
                expected: Rect::new(-13.0, -5.0, 58.0, 68.0),
            },
            DropCase {
                offset: Vec2::new(4.0, -3.0),
                blur: 2.0,
                spread: -5.0,
                expected: Rect::new(8.0, 11.0, 42.0, 52.0),
            },
        ];

        for case in cases {
            assert_eq!(
                shadow_paint_rect_local(
                    Some(source),
                    Size::ZERO,
                    case.offset,
                    case.blur,
                    case.spread,
                    false,
                ),
                case.expected,
                "{case:?}",
            );
        }

        assert_eq!(
            shadow_paint_rect_local(
                Some(source),
                Size::ZERO,
                Vec2::new(100.0, -100.0),
                20.0,
                8.0,
                true,
            ),
            source,
            "inset paint remains clipped to its source rect",
        );
    }

    /// A mesh whose vertex hull overflows its owner box (a rotated / scaled
    /// glyph) must report that hull as its paint bbox. Returning the owner
    /// rect instead makes partial damage too small — the overflow paints with
    /// cut vertices and leaves leftover pixels when it changes. Regression for
    /// the subscription-glyph triangle.
    #[test]
    fn mesh_paint_bbox_is_vertex_hull_not_owner_rect() {
        let owner = Size::new(13.0, 13.0);
        // Hull reaches left/up past the owner origin and right/down past its
        // size — i.e. paints outside the owner box on every side.
        let hull = Rect {
            min: Vec2::new(-5.0, -4.0),
            size: Size::new(25.0, 24.0),
        };
        let mesh = |local_rect| ShapeRecord::Mesh {
            local_rect,
            tint: ColorF16::from(Color::WHITE),
            vertices: Span::new(0, 3),
            indices: Span::new(0, 3),
            bbox: hull,
            content_hash: 0,
        };

        assert_eq!(
            mesh(None).bbox_local(owner),
            hull,
            "the paint bbox is the vertex hull, not the owner rect"
        );

        // `local_rect` translates the hull (its size still comes from the
        // vertices, not `local_rect.size`).
        let offset = Rect {
            min: Vec2::new(2.0, 3.0),
            size: Size::new(99.0, 99.0),
        };
        assert_eq!(
            mesh(Some(offset)).bbox_local(owner),
            Rect {
                min: hull.min + offset.min,
                size: hull.size,
            },
            "local_rect offsets the hull; the size is unchanged"
        );
    }

    /// Same authoring fields, different shape kind: swapping a
    /// `RoundedRect` for a `WindowedRect` inverts the painted region,
    /// so a hash collision would make damage diff skip the repaint.
    /// The tag byte written first in the hash schedule is the guard.
    #[test]
    fn windowed_rect_hash_differs_from_rounded_rect() {
        let fill = ShapeBrush::Solid(ColorF16::from(Color::WHITE));
        let stroke = ShapeStroke::from(Stroke::solid(Color::BLACK, 2.0));
        let rounded = ShapeRecord::RoundedRect {
            local_rect: None,
            corners: Corners::all(8.0),
            fill,
            stroke,
            fill_grad_hash: 0,
        };
        let windowed = ShapeRecord::WindowedRect {
            local_rect: None,
            corners: Corners::all(8.0),
            fill,
            stroke,
            fill_grad_hash: 0,
        };
        assert_ne!(
            compute_record_hash(&rounded),
            compute_record_hash(&windowed)
        );
    }

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
            bbox: Rect::ZERO,
            content_hash: 0xdead_beef,
        };
        let b = ShapeRecord::Mesh {
            local_rect: None,
            tint,
            vertices: Span::new(1234, 3),
            indices: Span::new(5678, 3),
            bbox: Rect::ZERO,
            content_hash: 0xdead_beef,
        };
        assert_eq!(compute_record_hash(&a), compute_record_hash(&b));

        let with_rect = |rect| ShapeRecord::Mesh {
            local_rect: Some(rect),
            tint,
            vertices: Span::new(0, 3),
            indices: Span::new(0, 3),
            bbox: Rect::ZERO,
            content_hash: 0xdead_beef,
        };
        let zero = compute_record_hash(&with_rect(Rect::ZERO));
        assert_eq!(
            zero,
            compute_record_hash(&with_rect(Rect::new(EPS * 0.5, -EPS * 0.5, EPS, -EPS,))),
        );
        assert_ne!(
            zero,
            compute_record_hash(&with_rect(Rect::new(EPS * 2.0, 0.0, 0.0, 0.0))),
        );
    }

    #[test]
    fn shape_image_hash_distinguishes_handle_tint_and_filters() {
        let make =
            |id: TextureId, tint: Color, min_filter: ImageFilter, mag_filter: ImageFilter| {
                ShapeRecord::Image {
                    local_rect: None,
                    tint: ColorF16::from(tint),
                    id,
                    size: glam::U16Vec2::new(64, 64),
                    fit: ImageFit::Fill,
                    min_filter,
                    mag_filter,
                }
            };
        let baseline = compute_record_hash(&make(
            TextureId(0xa),
            Color::WHITE,
            ImageFilter::Linear,
            ImageFilter::Linear,
        ));
        assert_ne!(
            baseline,
            compute_record_hash(&make(
                TextureId(0xb),
                Color::WHITE,
                ImageFilter::Linear,
                ImageFilter::Linear,
            ))
        );
        assert_ne!(
            baseline,
            compute_record_hash(&make(
                TextureId(0xa),
                Color::rgba(1.0, 0.0, 0.0, 1.0),
                ImageFilter::Linear,
                ImageFilter::Linear,
            ))
        );
        assert_ne!(
            baseline,
            compute_record_hash(&make(
                TextureId(0xa),
                Color::WHITE,
                ImageFilter::Nearest,
                ImageFilter::Linear,
            ))
        );
        assert_ne!(
            baseline,
            compute_record_hash(&make(
                TextureId(0xa),
                Color::WHITE,
                ImageFilter::Linear,
                ImageFilter::Nearest,
            ))
        );
    }
}
