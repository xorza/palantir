use crate::layout::types::align::{self, Align};
use crate::primitives::color::ColorF16;
use crate::primitives::image::{ImageFilter, ImageFit};
use crate::primitives::interned_str::RecordedText;
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::span::Span;
use crate::scene::shapes::paint::{CurveBasis, ImageSource, QuadShape, ShapeBrush};
use crate::shape::style::{LineCap, LineJoin};
use crate::text::wrap::TextWrap;
use crate::text::{FontFamily, FontWeight};
use glam::Vec2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum ColorMode {
    /// One colour for the whole polyline — the default and the only
    /// mode whose colour span is a single entry.
    #[default]
    Single = 0,
    PerPoint = 1,
    PerSegment = 2,
}

/// The stable tag cache keys are built from is [`Self::tag`], **not**
/// this enum's `repr` discriminant — nothing reads the latter. Reorder
/// variants freely; a hash can only move when `tag` does. Adding a
/// variant forces the `tag` and
/// [`compute_record_hash`](crate::scene::shapes::hash::compute_record_hash)
/// matches to grow, since both are exhaustive.
///
/// `#[repr(u8)]` stays for **layout**, not identity: it holds the
/// discriminant to one byte, which the `ShapeRecord` entry in
/// `hot_struct_sizes` pins.
#[repr(u8)]
#[derive(Clone, Debug)]
pub(crate) enum ShapeRecord {
    /// Rounded rectangle, box-shadow, or rounded triangle, per
    /// [`QuadShape`]. One record kind, because everything outside the
    /// shape is shared: the same quad pipeline, the same `Quad`
    /// instance, and the same cull / group-flush / occlusion handling
    /// from the payload down.
    Quad(QuadShape),
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
    },
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
        /// defaults to [`LINE_HEIGHT_MULT`](crate::widgets::theme::text_style::LINE_HEIGHT_MULT) (1.2). Carrying
        /// the resolved px on the shape — instead of a multiplier the
        /// shaper would re-resolve — means the shaper doesn't have to
        /// know about widget conventions, and two `ShapeRecord::Text` runs at
        /// the same font-size but different leading correctly produce
        /// distinct cached shaped buffers (via
        /// [`TextShapeKey::lh_q`](crate::text::key::TextShapeKey::lh_q)).
        line_height_px: f32,
        wrap: TextWrap,
        align: Align,
        family: FontFamily,
        weight: FontWeight,
    },
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
    },
    /// Textured rectangle — a registered image or an app-rendered
    /// `GpuView`'s off-screen target, per [`ImageSource`]. One record
    /// kind, because everything outside `source` is shared: the same
    /// paint-rect resolution, fit, sampling, tint, and image pipeline.
    /// `local_rect = None` paints into the owner's full arranged rect;
    /// `Some(r)` paints `r` at owner-relative coords. `tint` multiplies
    /// sampled pixels in linear-RGB premultiplied space.
    Image {
        local_rect: Option<Rect>,
        tint: ColorF16,
        source: ImageSource,
        fit: ImageFit,
        min_filter: ImageFilter,
        mag_filter: ImageFilter,
    },
    /// Native GPU stroke — a cubic Bézier or an exact circular arc, per
    /// [`CurveBasis`] (quadratics promote to cubic at lowering, lines
    /// degenerate to one — see `shapes::lower`). One record kind, because
    /// everything outside `basis` is shared: the same pipeline, cap model,
    /// gradient-along-`t` sampling, and deferred stroke bound. Stored
    /// owner-local; the composer adds the owner origin + active transform
    /// at compose time and uploads to a per-instance buffer. No joins
    /// (single-segment primitive); `fill` and `cap` are documented on
    /// their fields. `bbox` is the tight owner-local centerline AABB;
    /// damage and composition apply the shared raster-aware stroke
    /// inflation after transforms are known.
    Curve {
        basis: CurveBasis,
        width: f32,
        /// Lowered stroke fill. Solid colour stays inline; `Linear`
        /// gradient content rides as a `RecordedGradient` indexed by
        /// `ShapeBrush::Gradient` and resolves its atlas row on encode.
        /// The gradient is sampled in the shader along the curve
        /// parameter `t` — p0 → p3 for a cubic, a0 → a1 for an arc — so
        /// the `LinearGradient::angle` from authoring is intentionally
        /// ignored: the stroke carries its own 1-D parameter. The
        /// authoring type cannot contain radial or conic gradients.
        fill: ShapeBrush,
        /// Pre-computed content hash of `fill` when it's a gradient,
        /// `0` for solid — same context-free-hash trick as
        /// [`QuadShape::Rect`]'s own `fill_grad_hash`.
        fill_grad_hash: u64,
        /// End-cap style. Joins are absent (single-curve primitive,
        /// no interior). `Round`/`Square` extend the painted strip by
        /// `width/2` past each endpoint along the local tangent.
        cap: LineCap,
        bbox: Rect,
    },
}

impl ShapeRecord {
    /// Owner-local bbox used as the basis for cascade's screen-space paint
    /// bound. `Polyline` / `Curve` return their tight centerline
    /// bbox because stroke width and the physical-pixel AA fringe are applied
    /// after the bbox reaches screen space. `Quad` defers to
    /// [`QuadShape::bbox_local`], which is where the shadow halo is
    /// accounted for; the remaining shapes return their paint bbox
    /// directly. Does **not** handle `Text` — its bbox
    /// depends on the shaped extent from the layout pass and is computed by
    /// [`text_paint_bbox_local`], which cascade calls directly.
    #[inline]
    pub(crate) fn bbox_local(&self, owner_size: Size) -> Rect {
        match self {
            ShapeRecord::Quad(shape) => shape.bbox_local(owner_size),
            ShapeRecord::Polyline { bbox, .. } | ShapeRecord::Curve { bbox, .. } => *bbox,
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
            ShapeRecord::Image { local_rect, .. } => local_rect.unwrap_or(Rect {
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

    /// Stable hash tag — the discriminant byte
    /// [`crate::scene::shapes::hash::compute_record_hash`] writes ahead
    /// of the per-variant fields, and so an input to every subtree hash
    /// and measure-cache key.
    ///
    /// **This match is the sole source of those numbers**; the enum's
    /// own `repr` discriminant is unread, so reordering variants cannot
    /// move a hash. A number is frozen once it has shipped in a saved
    /// document — give a new variant the next free one.
    ///
    /// Five are retired, all to variants folded into a surviving one,
    /// whose numbers would now collide with hashes cached from before
    /// the merge. Each merge left a nested tag doing the separating the
    /// record tag used to do:
    ///
    /// | retired | was | folded into | told apart by |
    /// |---|---|---|---|
    /// | 4 | `Shadow` | `Quad` (0) | [`QuadShape::tag`] |
    /// | 7 | `GpuView` | `Image` (5) | [`ImageSource::tag`] |
    /// | 8 | `Triangle` | `Quad` (0) | [`QuadShape::tag`] |
    /// | 9 | `WindowedRect` | `Quad` (0) | [`QuadShape::Rect`]'s `kind` |
    /// | 10 | `Arc` | `Curve` (6) | [`CurveBasis::tag`] |
    ///
    /// [`CurveBasis::tag`]: crate::scene::shapes::paint::CurveBasis::tag
    pub(crate) const fn tag(&self) -> u8 {
        match self {
            ShapeRecord::Quad(_) => 0,
            ShapeRecord::Polyline { .. } => 1,
            ShapeRecord::Text { .. } => 2,
            ShapeRecord::Mesh { .. } => 3,
            ShapeRecord::Image { .. } => 5,
            ShapeRecord::Curve { .. } => 6,
        }
    }
}

/// Tight owner-local paint bbox of a [`ShapeRecord::Text`], using the
/// shaped extent the measure pass already computed (lives in
/// `LayerLayout::text_shapes`). The encoder applies the same formula
/// in screen space — `align_in_rect` is the sole source so cascade
/// damage rects and encoder draw rects can't drift.
///
/// **Damage inflation lives in cascade** (`scene::cascade`), not here —
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
///   [`crate::layout::types::align::align_in_rect`] against the owner's padded inner
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
            align::align_in_rect(owner_local.deflated_by(padding), measured, align)
        }
    }
}

/// **The single NaN gate's predicate.** `Shapes::add` runs this on the
/// lowered record — one place, one `O(1)` test per shape, in release as
/// well as debug.
///
/// Lowered is the right tier for it. Every bulk input (polyline points,
/// mesh vertices, curve control points) has by then been folded into a
/// `bbox` under the AABB NaN contract, so a NaN in any of them shows up
/// as a NaN bbox — which means one `Rect` test replaces an `O(n)` scan
/// of the data that produced it, and the check is cheap enough to keep
/// in release rather than compiling out with the assert.
///
/// The one input that does *not* survive lowering is a gradient's
/// geometry, which is interned into the record store behind a
/// `GradientId`. `lower::brush` screens it there instead.
impl NanCheck for ShapeRecord {
    fn has_nan(&self) -> bool {
        match self {
            ShapeRecord::Quad(shape) => shape.has_nan(),
            // `bbox` carries the points; `content_hash` and the spans
            // are frame-local indices and can't be NaN.
            ShapeRecord::Polyline { width, bbox, .. } => width.is_nan() || bbox.has_nan(),
            ShapeRecord::Text {
                local_origin,
                color,
                font_size_px,
                line_height_px,
                ..
            } => {
                local_origin.has_nan()
                    || color.has_nan()
                    || font_size_px.is_nan()
                    || line_height_px.is_nan()
            }
            ShapeRecord::Mesh {
                local_rect,
                tint,
                bbox,
                ..
            } => local_rect.has_nan() || tint.has_nan() || bbox.has_nan(),
            ShapeRecord::Image {
                local_rect,
                tint,
                fit,
                ..
            } => local_rect.has_nan() || tint.has_nan() || fit.has_nan(),
            // `bbox` is derived from `basis`, so it stands in for the
            // control points / centre / radius / angles.
            ShapeRecord::Curve {
                width, fill, bbox, ..
            } => width.is_nan() || fill.has_nan() || bbox.has_nan(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::approx::EPS;
    use crate::primitives::color::Color;
    use crate::primitives::corners::Corners;
    use crate::primitives::interned_str::TextSource;
    use crate::primitives::rect::Rect;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::size::Size;
    use crate::primitives::stroke::Stroke;
    use crate::primitives::texture_id::TextureId;
    use crate::scene::shapes::hash::compute_record_hash;
    use crate::scene::shapes::paint::{LoweredShadow, ShapeStroke, shadow_paint_rect_local};
    use crate::scene::shapes::record::*;
    use crate::shape::rect::RectKind;
    use glam::Vec2;

    /// [`ShapeRecord::tag`] is the only source of the hash's leading
    /// discriminant byte, so its numbers have to be pairwise distinct
    /// and frozen once shipped. Two variants sharing a tag would differ
    /// only by whatever their field schedules don't have in common, which
    /// for the stroke kinds is very little — the reason
    /// [`CurveBasis::tag`] exists now that arcs hash under `Curve`.
    ///
    /// The `repr` discriminants that used to sit on the variants looked
    /// like they enforced this. They never did: they pin their *own*
    /// uniqueness, which says nothing about the hand-written `tag`
    /// match. This is where the property is actually checked.
    ///
    /// A brand-new variant still has to be added to the table below by
    /// hand; what the exhaustive `tag` match guarantees is only that
    /// someone had to pick a number for it.
    #[test]
    fn shape_record_tags_are_distinct_and_pinned() {
        let fill = ShapeBrush::Solid(ColorF16::from(Color::WHITE));
        let stroke = ShapeStroke::from(Stroke::solid(Color::BLACK, 1.0));
        // 4, 7, 8, 9 and 10 are absent on purpose — retired with
        // `Shadow`, `GpuView`, `Triangle`, `WindowedRect` and `Arc`
        // when each folded into a surviving variant.
        let table = [
            (
                0,
                ShapeRecord::Quad(QuadShape::Rect {
                    kind: RectKind::Rounded,
                    local_rect: None,
                    corners: Corners::ZERO,
                    fill,
                    stroke,
                    fill_grad_hash: 0,
                }),
            ),
            (
                1,
                ShapeRecord::Polyline {
                    width: 1.0,
                    color_mode: ColorMode::Single,
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                    points: Span::new(0, 2),
                    colors: Span::new(0, 1),
                    bbox: Rect::ZERO,
                    content_hash: 0,
                },
            ),
            (
                2,
                ShapeRecord::Text {
                    local_origin: None,
                    text: RecordedText {
                        source: TextSource {
                            span: Span::new(0, 1),
                        },
                        hash: 0,
                    },
                    color: ColorF16::from(Color::WHITE),
                    font_size_px: 12.0,
                    line_height_px: 14.0,
                    wrap: TextWrap::SingleLine,
                    align: Align::CENTER,
                    family: FontFamily::Sans,
                    weight: FontWeight::Regular,
                },
            ),
            (
                3,
                ShapeRecord::Mesh {
                    local_rect: None,
                    tint: ColorF16::from(Color::WHITE),
                    vertices: Span::new(0, 3),
                    indices: Span::new(0, 3),
                    bbox: Rect::ZERO,
                    content_hash: 0,
                },
            ),
            (
                5,
                ShapeRecord::Image {
                    local_rect: None,
                    tint: ColorF16::from(Color::WHITE),
                    source: ImageSource::Texture {
                        id: TextureId(1),
                        size: glam::UVec2::new(1, 1),
                    },
                    fit: ImageFit::Fill,
                    min_filter: ImageFilter::Linear,
                    mag_filter: ImageFilter::Linear,
                },
            ),
            (
                6,
                ShapeRecord::Curve {
                    basis: CurveBasis::Cubic {
                        p0: Vec2::ZERO,
                        p1: Vec2::ZERO,
                        p2: Vec2::ZERO,
                        p3: Vec2::ZERO,
                    },
                    width: 1.0,
                    fill,
                    fill_grad_hash: 0,
                    cap: LineCap::Butt,
                    bbox: Rect::ZERO,
                },
            ),
        ];

        let mut seen = Vec::with_capacity(table.len());
        for (expected, record) in &table {
            assert_eq!(
                record.tag(),
                *expected,
                "{record:?} moved off its shipped tag",
            );
            assert!(
                !seen.contains(expected),
                "tag {expected} is claimed by two variants",
            );
            seen.push(*expected);
        }

        // Three shapes share the `Quad` tag — the biggest merge. The
        // shape byte, not the record tag, is what keeps their hashes
        // apart (see `quad_shapes_hash_apart`).
        let quad = |shape| ShapeRecord::Quad(shape);
        let shadow_shape = QuadShape::Shadow {
            local_rect: None,
            corners: Corners::ZERO,
            shadow: LoweredShadow::from(Shadow::default()),
        };
        let triangle_shape = QuadShape::Triangle {
            a: Vec2::ZERO,
            b: Vec2::ZERO,
            c: Vec2::ZERO,
            radius: 0.0,
            fill: ColorF16::from(Color::WHITE),
            stroke,
            bbox: Rect::ZERO,
        };
        let rect_shape = QuadShape::Rect {
            kind: RectKind::Rounded,
            local_rect: None,
            corners: Corners::ZERO,
            fill,
            stroke,
            fill_grad_hash: 0,
        };
        for shape in [rect_shape, shadow_shape, triangle_shape] {
            assert_eq!(quad(shape).tag(), 0, "{shape:?} tags as `Quad`");
        }
        let mut shape_tags = Vec::new();
        for tag in [rect_shape.tag(), shadow_shape.tag(), triangle_shape.tag()] {
            assert!(
                !shape_tags.contains(&tag),
                "quad-shape tag {tag} is claimed twice",
            );
            shape_tags.push(tag);
        }

        // Both bases share the `Curve` tag — same merge, same
        // guarantee. The basis byte, not the record tag, is what keeps
        // their hashes apart (see `curve_and_arc_bases_hash_apart`).
        let arc = ShapeRecord::Curve {
            basis: CurveBasis::Arc {
                center: Vec2::ZERO,
                radius: 1.0,
                a0: 0.0,
                a1: 1.0,
            },
            width: 1.0,
            fill,
            fill_grad_hash: 0,
            cap: LineCap::Butt,
            bbox: Rect::ZERO,
        };
        assert_eq!(arc.tag(), 6, "an arc-basis curve tags as `Curve`");
        assert_ne!(
            CurveBasis::Cubic {
                p0: Vec2::ZERO,
                p1: Vec2::ZERO,
                p2: Vec2::ZERO,
                p3: Vec2::ZERO,
            }
            .tag(),
            CurveBasis::Arc {
                center: Vec2::ZERO,
                radius: 1.0,
                a0: 0.0,
                a1: 1.0,
            }
            .tag(),
        );

        // Same merge, same guarantee: a view composite shares `Image`'s
        // record tag, so `ImageSource::tag` is what keeps it apart from
        // a texture draw (see `image_source_hashes_apart_by_source`).
        let view = ShapeRecord::Image {
            local_rect: None,
            tint: ColorF16::from(Color::WHITE),
            source: ImageSource::GpuView { epoch: 0 },
            fit: ImageFit::Fill,
            min_filter: ImageFilter::Linear,
            mag_filter: ImageFilter::Linear,
        };
        assert_eq!(view.tag(), 5, "a `GpuView`-source image tags as `Image`");
        assert_ne!(
            ImageSource::Texture {
                id: TextureId(1),
                size: glam::UVec2::new(1, 1),
            }
            .tag(),
            ImageSource::GpuView { epoch: 0 }.tag(),
        );
    }

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

    /// Same rectangle payload, different paint kind: switching to a
    /// windowed rect inverts the painted region, so a hash collision
    /// would make damage diff skip the repaint.
    ///
    /// The same risk, one level up: all three quad shapes share
    /// [`ShapeRecord::Quad`]'s tag byte, so [`QuadShape::tag`] is the
    /// only thing separating a rectangle, a shadow, and a triangle over
    /// the same box — and each shape's own fields have to reach the
    /// hasher through the merged arm. Every case here is a repaint that
    /// damage diff would skip on a collision.
    #[test]
    fn quad_shapes_hash_apart() {
        let fill = ShapeBrush::Solid(ColorF16::from(Color::WHITE));
        let stroke = ShapeStroke::from(Stroke::solid(Color::BLACK, 2.0));
        let corners = Corners::all(8.0);
        let rect = |kind| {
            ShapeRecord::Quad(QuadShape::Rect {
                kind,
                local_rect: None,
                corners,
                fill,
                stroke,
                fill_grad_hash: 0,
            })
        };

        // Same rectangle payload, different paint kind.
        assert_ne!(
            compute_record_hash(&rect(RectKind::Rounded)),
            compute_record_hash(&rect(RectKind::Windowed)),
        );

        // Same rounded box, three different shapes.
        let shadow = ShapeRecord::Quad(QuadShape::Shadow {
            local_rect: None,
            corners,
            shadow: LoweredShadow::from(Shadow::default()),
        });
        let triangle = ShapeRecord::Quad(QuadShape::Triangle {
            a: Vec2::ZERO,
            b: Vec2::ZERO,
            c: Vec2::ZERO,
            radius: 0.0,
            fill: ColorF16::from(Color::WHITE),
            stroke,
            bbox: Rect::ZERO,
        });
        let mut seen = Vec::new();
        for (label, record) in [
            ("rounded", rect(RectKind::Rounded)),
            ("windowed", rect(RectKind::Windowed)),
            ("shadow", shadow),
            ("triangle", triangle),
        ] {
            let hash = compute_record_hash(&record);
            assert!(
                !seen.contains(&hash),
                "quad shape `{label}` collided with an earlier shape's hash",
            );
            seen.push(hash);
        }
    }

    /// Cubics and arcs share [`ShapeRecord::Curve`]'s tag byte, so the
    /// basis byte is the only thing separating their hashes — and the
    /// arc's own fields have to reach the hasher through the merged
    /// arm. A collision either way would make damage diff skip a
    /// repaint when a stroke changes shape.
    #[test]
    fn curve_and_arc_bases_hash_apart() {
        let fill = ShapeBrush::Solid(ColorF16::from(Color::WHITE));
        let curve = |basis| ShapeRecord::Curve {
            basis,
            width: 2.0,
            fill,
            fill_grad_hash: 0,
            cap: LineCap::Butt,
            bbox: Rect::ZERO,
        };
        let arc = |center, radius, a0, a1| {
            curve(CurveBasis::Arc {
                center,
                radius,
                a0,
                a1,
            })
        };
        let baseline = arc(Vec2::ZERO, 4.0, 0.0, 1.0);

        // Every field the two bases don't share is identical here, so
        // only `CurveBasis::tag` can tell these two apart.
        assert_ne!(
            compute_record_hash(&baseline),
            compute_record_hash(&curve(CurveBasis::Cubic {
                p0: Vec2::ZERO,
                p1: Vec2::ZERO,
                p2: Vec2::ZERO,
                p3: Vec2::ZERO,
            })),
            "a degenerate cubic must not collide with an arc",
        );

        for (label, other) in [
            ("center", arc(Vec2::new(1.0, 0.0), 4.0, 0.0, 1.0)),
            ("radius", arc(Vec2::ZERO, 5.0, 0.0, 1.0)),
            ("a0", arc(Vec2::ZERO, 4.0, 0.5, 1.0)),
            ("a1", arc(Vec2::ZERO, 4.0, 0.0, 1.5)),
        ] {
            assert_ne!(
                compute_record_hash(&baseline),
                compute_record_hash(&other),
                "arc `{label}` escaped the hash schedule",
            );
        }
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

    /// A view composite and a texture draw share `Image`'s record tag,
    /// so only [`ImageSource::tag`] separates their hashes — and the
    /// view's `epoch` has to reach the hasher through the merged arm.
    /// A collision either way makes damage diff skip a repaint: a view
    /// that bumped its epoch would keep its stale texture on screen.
    #[test]
    fn image_source_hashes_apart_by_source() {
        let image = |source| ShapeRecord::Image {
            local_rect: None,
            tint: ColorF16::from(Color::WHITE),
            source,
            fit: ImageFit::Fill,
            min_filter: ImageFilter::Linear,
            mag_filter: ImageFilter::Linear,
        };
        // Both sources carry one u64-shaped payload of the same value,
        // so the source tag is the only thing telling these two apart.
        let view = compute_record_hash(&image(ImageSource::GpuView { epoch: 7 }));
        assert_ne!(
            view,
            compute_record_hash(&image(ImageSource::Texture {
                id: TextureId(7),
                size: glam::UVec2::ZERO,
            })),
            "a texture id must not collide with an epoch of the same value",
        );
        assert_ne!(
            view,
            compute_record_hash(&image(ImageSource::GpuView { epoch: 8 })),
            "a bumped epoch must move the hash, or the view never repaints",
        );
        assert_eq!(
            view,
            compute_record_hash(&image(ImageSource::GpuView { epoch: 7 })),
            "a held epoch must hold the hash, or a static view never culls",
        );
    }

    #[test]
    fn shape_image_hash_distinguishes_handle_dimensions_tint_and_filters() {
        let make = |id: TextureId,
                    size: glam::UVec2,
                    tint: Color,
                    min_filter: ImageFilter,
                    mag_filter: ImageFilter| {
            ShapeRecord::Image {
                local_rect: None,
                tint: ColorF16::from(tint),
                source: ImageSource::Texture { id, size },
                fit: ImageFit::Fill,
                min_filter,
                mag_filter,
            }
        };
        let size = glam::UVec2::new(64, 64);
        let baseline = compute_record_hash(&make(
            TextureId(0xa),
            size,
            Color::WHITE,
            ImageFilter::Linear,
            ImageFilter::Linear,
        ));
        assert_ne!(
            baseline,
            compute_record_hash(&make(
                TextureId(0xb),
                size,
                Color::WHITE,
                ImageFilter::Linear,
                ImageFilter::Linear,
            ))
        );
        for changed_size in [
            glam::UVec2::new(size.x + (1 << 16), size.y),
            glam::UVec2::new(size.x, size.y + (1 << 16)),
        ] {
            assert_ne!(
                baseline,
                compute_record_hash(&make(
                    TextureId(0xa),
                    changed_size,
                    Color::WHITE,
                    ImageFilter::Linear,
                    ImageFilter::Linear,
                ))
            );
        }
        assert_ne!(
            baseline,
            compute_record_hash(&make(
                TextureId(0xa),
                size,
                Color::rgba(1.0, 0.0, 0.0, 1.0),
                ImageFilter::Linear,
                ImageFilter::Linear,
            ))
        );
        assert_ne!(
            baseline,
            compute_record_hash(&make(
                TextureId(0xa),
                size,
                Color::WHITE,
                ImageFilter::Nearest,
                ImageFilter::Linear,
            ))
        );
        assert_ne!(
            baseline,
            compute_record_hash(&make(
                TextureId(0xa),
                size,
                Color::WHITE,
                ImageFilter::Linear,
                ImageFilter::Nearest,
            ))
        );
    }
}
