//! One frame being composed, and the sink the paint calls arrive through.

use crate::icons::icon_raster_key::IconRasterKey;
use crate::primitives::approx::{EPS, noop_f32};
use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::color::ColorU8;
use crate::primitives::corners::Corners;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::spacing::Spacing;
use crate::primitives::span::Span;
use crate::primitives::{
    num::{F32Ext, Vec2Ext},
    rect::Rect,
    translate_scale::TranslateScale,
    urect::URect,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::ImageDraw;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_quad_payload::QuadGeom;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::quad::{AA_RADIUS, Quad};
use crate::renderer::render_buffer::curve::{
    CURVE_KIND_ARC, CURVE_KIND_CUBIC, CURVE_KIND_SEGMENT, CurveInstance, cap_lanes,
};
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::group_batch::GroupBatch;
use crate::renderer::render_buffer::icon::IconDrawRow;
use crate::renderer::render_buffer::image::{ImageDrawRow, ImageInstance, RenderTargetDraw};
use crate::renderer::render_buffer::mesh::{MeshDraw, MeshDrawRow, MeshInstance};
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::renderer::render_buffer::text_batch::TextBatch;
use crate::renderer::render_buffer::{MAX_ROUNDED_CLIP_DEPTH, RenderBuffer, RoundedClip};
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::paint::CurveBasis;
use crate::scene::shapes::record::ColorMode;
use crate::shape::stroke_bounds::HALF_FRINGE;
use crate::shape::style::LineCap;
use glam::{UVec2, Vec2};

use crate::renderer::frontend::composer::clip_stack::ClipFrame;
use crate::renderer::frontend::composer::geometry;
use crate::renderer::frontend::composer::geometry::StrokeBbox;
use crate::renderer::frontend::composer::{Composer, GroupCursors, OpenBatch, PolylineScratch};

/// One compose pass in flight: the [`Composer`]'s retained scratch bound
/// to the buffer being filled, the record payloads variable-length draws
/// read from, and the frame's display.
///
/// **This is the algorithm.** Paint streams in through [`PaintSink`], one
/// call per lowered draw, in authoring order; the group and batch state
/// machine those handlers drive — what decides where one draw's output
/// lands relative to the last one's — lives here too, because it reads
/// and writes the same buffer they do. The `Composer` behind `composer`
/// is the arena the whole pass is built in — nothing on it takes an
/// output buffer, and nothing here has to hand one back to it.
#[derive(Debug)]
pub(crate) struct ComposeSession<'a> {
    pub(super) composer: &'a mut Composer,
    pub(super) store: &'a RecordStore,
    pub(super) out: &'a mut RenderBuffer,
}

/// A quad-tier draw reduced to physical space — everything
/// [`ComposeSession::quad`]'s per-shape half derives, and its shared
/// half consumes. `corners` and `fill_axis` are the two `Quad` lanes
/// whose meaning depends on the shape: corner radii + brush/shadow axis
/// for a rect, packed corner points + third point/radius for a triangle.
#[derive(Debug)]
struct PackedQuad {
    rect: ScaledRect,
    corners: Corners,
    fill_axis: FillAxis,
    stroke_width: f32,
}

impl PackedQuad {
    /// Nothing rounded off its corners and nothing painted outside its
    /// rect — the shape both the clear fold and the fragment fast path
    /// start from.
    fn is_sharp(&self) -> bool {
        noop_f32(self.stroke_width) && self.corners.approx_zero()
    }

    /// [`Self::is_sharp`] plus a rect whose physical edges land on whole
    /// pixels. Alignment is exact, not approximate: exactness is what
    /// makes the fragment fast path bitwise-identical to the SDF (host
    /// pixel snapping yields exact integers when active; unsnapped
    /// fractional rects keep the full SDF for edge AA).
    fn is_pixel_aligned(&self) -> bool {
        let phys = self.rect.phys;
        let max = phys.max();
        self.is_sharp()
            && phys.min.x.is_integral()
            && phys.min.y.is_integral()
            && max.x.is_integral()
            && max.y.is_integral()
    }
}

/// A draw's rect in the two forms every rect-shaped handler needs, from
/// [`ComposeSession::scaled_rect`]. [`PackedQuad`] carries one rather
/// than a second pair of fields meaning the same two things.
#[derive(Debug)]
struct ScaledRect {
    /// Physical px — what the emitted instance carries.
    phys: Rect,
    /// Viewport-clamped integer bounds — what culling and the group's
    /// overlap tracking test against.
    urect: URect,
}

impl ScaledRect {
    /// The bounds of a rect that is already physical — a covering AABB
    /// the composer computed rather than transformed.
    fn from_phys(phys: Rect, viewport: UVec2) -> Self {
        Self {
            phys,
            urect: geometry::urect_from_phys(phys.min, phys.max(), viewport),
        }
    }
}

impl ComposeSession<'_> {
    /// Tile `t ∈ [0, 1]` into `n` contiguous ranges (the last ending at
    /// exactly `1.0`, so the shader's trailing-cap test fires) and push
    /// one instance per range; `proto` supplies every other lane.
    fn push_sub_instances(&mut self, n: u32, proto: CurveInstance) {
        let inv_n = 1.0 / n as f32;
        for i in 0..n {
            let t1 = if i + 1 == n {
                1.0
            } else {
                (i + 1) as f32 * inv_n
            };
            self.out.curves.push(CurveInstance {
                t0: i as f32 * inv_n,
                t1,
                ..proto
            });
        }
    }

    /// Apply the walk transform to a payload's logical rect, scale it to
    /// physical px, and derive its integer bounds — the opening move of
    /// `icon`, `image`, `text` and `pack_quad`. Scaling happens once
    /// because the cull bounds and the emitted instance share the
    /// result, so a culled draw costs the same as an emitted one.
    fn scaled_rect(&self, rect: Rect) -> ScaledRect {
        let world = self.composer.transform.apply_rect(rect);
        let phys = world.scaled_by(self.out.display.scale_factor, self.out.display.pixel_snap);
        ScaledRect::from_phys(phys, self.out.display.physical)
    }

    /// The part of `whole` that can be seen — inside the surface and inside the
    /// active clip — in physical pixels, or `None` where that is all of it.
    ///
    /// Two things cut a rect down, and both have to be here: the surface, since
    /// layout may hand back a rect larger than the window, and the scissor, for
    /// a view scrolled partly out of its pane.
    ///
    /// `None` where nothing was cut, so that the usual view — which nothing
    /// clips — stays on the path it was always on. A fast path rather than a
    /// distinction that has to hold: an intersection reconstructs its size as
    /// `(min + size) - min`, which is exact at the coordinates a surface reaches
    /// but is not exact in general, and answering `Some` where it drifted would
    /// take the other branch to the same numbers.
    fn seen(&self, whole: Rect) -> Option<Rect> {
        let surface = self.out.display.physical;
        let mut clipped = whole.clamp_to(Rect::new(0.0, 0.0, surface.x as f32, surface.y as f32));
        if let Some(scissor) = self.composer.clip.scissor() {
            clipped = clipped.clamp_to(scissor.into());
        }
        (clipped != whole).then_some(clipped)
    }
}

impl Drop for ComposeSession<'_> {
    /// Close the trailing text batch and draw group.
    ///
    /// Finalization is a destructor rather than a `finish()` the caller
    /// must remember: a session dropped un-closed leaves a
    /// `RenderBuffer` that *looks* populated but whose trailing group
    /// and batch were never emitted, so the backend schedules neither —
    /// missing pixels, nothing failing loudly. Since the session holds
    /// `&mut RenderBuffer`, that borrow also ends exactly here.
    fn drop(&mut self) {
        self.close_batch();
        self.flush();
    }
}

impl PaintSink for ComposeSession<'_> {
    fn push_clip(&mut self, p: PushClipPayload) {
        let scale = self.out.display.scale_factor;
        let snap = self.out.display.pixel_snap;
        let viewport_phys = self.out.display.physical;
        let logical_radius = (!p.corners.approx_zero()).then_some(p.corners);
        let world = self.composer.transform.apply_rect(p.rect);
        // Scaled once: the scissor is the integer cover of this rect and
        // the rounded mask below is the rect itself, so deriving them
        // from two calls meant scaling the same rectangle twice per clip
        // push.
        let phys = world.scaled_by(scale, snap);
        let me = geometry::urect_from_phys(phys.min, phys.max(), viewport_phys);
        let parent = self.composer.clip.top();
        let scissor = match parent {
            Some(parent) => me.clamp_to(parent.scissor),
            None => me,
        };
        let parent_chain = parent.map_or(Span::default(), |f| f.chain);
        let chain = if let Some(logical_radius) = logical_radius {
            // Combine current transform's uniform scale with DPR
            // so radii match the painted SDF's physical size.
            let scale_phys = geometry::phys_scale(self.composer.transform.current(), scale);
            // `mask_rect` stays unclamped — the SDF needs the
            // rect's true edges, otherwise corner curves
            // would shift inward when the clip partially
            // leaves the viewport.
            let rc = RoundedClip {
                mask_rect: phys,
                corners: logical_radius.scaled_by(scale_phys),
            };
            // A rounded push nested in rounded ancestors
            // STACKS: child chain = ancestor chain + own
            // mask, copied so every chain is one contiguous
            // span the stencil path can stamp outer→inner.
            // Re-pushing the innermost mask verbatim adds no
            // depth (a redundant stamp would test/write the
            // same pixels).
            if self.out.rounded_clips[parent_chain.range()].last() == Some(&rc) {
                parent_chain
            } else {
                let depth = parent_chain.len + 1;
                if depth > MAX_ROUNDED_CLIP_DEPTH {
                    geometry::rounded_clip_depth_overflow(depth);
                }
                let chain_start = self.out.rounded_clips.len() as u32;
                self.out
                    .rounded_clips
                    .extend_from_within(parent_chain.range());
                self.out.rounded_clips.push(rc);
                Span::new(chain_start, depth)
            }
        } else {
            // Rect clip nested inside rounded ancestors: inherit
            // the ancestor chain so children stay stencil-tested
            // against the active masks. Without this, the child
            // group would draw with ref=0 over pixels already
            // stenciled nonzero by the ancestors' masks, and the
            // stencil_test pipeline would discard every fragment.
            parent_chain
        };
        self.enter_clip(ClipFrame { scissor, chain });
    }

    fn pop_clip(&mut self) {
        self.leave_clip();
    }

    fn push_transform(&mut self, t: TranslateScale) {
        self.composer.transform.push(t);
    }

    fn pop_transform(&mut self) {
        self.composer.transform.pop();
    }

    fn quad(&mut self, p: DrawQuadPayload) {
        let packed = self.pack_quad(&p);
        // The clear fold sits here, at the top level, because what it does
        // is frame-global — it drops everything composed so far. Reducing
        // one shape to physical space is [`Self::pack_quad`]'s job and
        // ends at [`PackedQuad`]; folding a whole frame away is not part
        // of that and must not hide inside one of its arms.
        if self.fold_into_clear(&p, &packed) {
            return;
        }
        // Clip-cull: skip emitting the quad when it sits entirely outside
        // the active scissor. The GPU would scissor it away anyway; this
        // saves the `quads.push` + per-quad math.
        //
        // The clipped rect is also what the overlap test wants, and what
        // text tests with: the pixels this draw can reach are what a
        // later draw has to be ordered against, so a quad whose ancestor
        // clip cut it does not force a flush over ground it never paints.
        let visible = self.composer.clip.clamped(packed.rect.urect);
        if visible.is_paint_empty() {
            return;
        }
        self.quad_forces_flush(visible);
        // Fragment fast path: a solid, sharp, stroke-less quad whose
        // physical rect is pixel-aligned rasterizes only interior
        // fragments (SDF coverage exactly 1.0) — flag the instance so the
        // shader returns the premultiplied fill directly, skipping the SDF
        // + composite path. `SOLID` keeps shadows and triangles out.
        let fast = p.fill.kind == FillKind::SOLID && packed.is_pixel_aligned();
        let fill_kind = if fast {
            p.fill.kind.with_fast()
        } else {
            p.fill.kind
        };
        self.out.quads.push(Quad {
            rect: packed.rect.phys,
            fill: p.fill.color,
            corners: packed.corners,
            stroke_color: p.stroke.color,
            stroke_width: packed.stroke_width,
            fill_kind,
            fill_lut_row: p.fill.lut_row,
            fill_axis: packed.fill_axis,
        });
        self.record_opaque_cover(&p, &packed, fast);
    }

    fn mesh(&mut self, p: DrawMeshPayload) {
        let scale = self.out.display.scale_factor;
        let viewport_phys = self.out.display.physical;
        // `draw_mesh` already gated empty/degenerate meshes
        // (`draw_mesh` applies its no-op gate), so `v_len >= 1` here.
        // Inflate by 0.5 phys-px to match polyline's AA-fringe
        // policy. Mesh today paints inside its vertex hull,
        // but a future AA edge or displacement shader would
        // silently produce false negatives — and false
        // negatives in the overlap test reorder paint. The
        // same inflated rect feeds the clip cull below.
        // Mesh skips snapping (matches polyline/curve), so it cannot use
        // `scaled_rect`; the fold and the scale still go through the same
        // `phys_bbox` the stroked pair uses, which is what keeps the cull
        // tracking the other tiers. The integer bounds below are
        // `urect_from_phys` like every other tier's.
        let xform = self.composer.transform.current();
        let phys = geometry::phys_bbox(xform, p.bbox, p.origin, scale);
        let fringe = Vec2::splat(HALF_FRINGE);
        let mesh_urect =
            geometry::urect_from_phys(phys.min - fringe, phys.max() + fringe, viewport_phys);
        // Clip-cull + batch-close: a mesh fully outside the
        // active scissor (e.g. scrolled out of an ancestor clip)
        // is skipped; a surviving one closes the open text batch
        // so its text emits before this above-text geometry.
        if !self.admit_higher_kind(PaintTier::Mesh, mesh_urect) {
            return;
        }
        // Verts already live in RecordStore owner-local;
        // span passes through to `MeshDraw` verbatim. The
        // per-instance translate folds in both the owner
        // origin and the active push-transform stack so the
        // shader produces physical coords. Phase 1's
        // transform/tint move plus this slice eliminates
        // both the per-vertex CPU multiply and the
        // per-frame vertex copy.
        let scale_phys = geometry::phys_scale(xform, scale);
        let phys_translate = (xform.scale * p.origin + xform.translation) * scale;
        self.out.meshes.push(MeshDrawRow {
            draw: MeshDraw {
                vertices: (p.v_start..p.v_start + p.v_len).into(),
                indices: (p.i_start..p.i_start + p.i_len).into(),
            },
            instance: MeshInstance {
                translate: phys_translate,
                scale: scale_phys,
                tint: p.tint.into(),
                ..bytemuck::Zeroable::zeroed()
            },
        });
    }

    fn icon(&mut self, p: DrawIconPayload) {
        let ScaledRect {
            phys: phys_rect,
            urect,
        } = self.scaled_rect(p.rect);
        if !self.admit_higher_kind(PaintTier::Icon, urect) {
            return;
        }
        // The raster size is decided here, not upstream: this is the first
        // point that knows the display scale and every ancestor transform, and
        // so the first point that knows how many device pixels the icon covers.
        let key = IconRasterKey::for_box(p.icon, Vec2::new(phys_rect.size.w, phys_rect.size.h));
        // Whole-pixel origin, with the raster centred in the box it was sized
        // from. The ladder can round the raster a pixel or two off that box
        // (§ `IconRasterKey`), and centring spreads the difference instead of
        // piling it on one edge; the `Nearest` atlas sampler is why the origin
        // itself must land on integers.
        let size = key.size().as_vec2();
        let centred = phys_rect.min + (Vec2::new(phys_rect.size.w, phys_rect.size.h) - size) * 0.5;
        self.out.icons.push(IconDrawRow {
            key,
            origin: centred.fast_round().as_ivec2(),
            color: p.tint.into(),
            desaturate: p.desaturate,
        });
    }

    fn image(&mut self, draw: ImageDraw<'_>) {
        let ImageDraw { payload: p, paint } = draw;
        let ScaledRect {
            phys: phys_rect,
            urect: image_urect,
        } = self.scaled_rect(p.rect);
        // Clip-cull + batch-close: image sits above text in the
        // kind order (same as mesh), so a surviving draw closes
        // the open text batch first.
        if !self.admit_higher_kind(PaintTier::Image, image_urect) {
            return;
        }
        // A `GpuView` is drawn over the part of itself that can be seen rather
        // than over the whole rect, because that is all its target holds — see
        // the scheduling below. Its UV stays whole, since the target *is* the
        // visible part; every other image keeps the rect and UV the encoder
        // resolved.
        let seen = paint.and_then(|_| self.seen(phys_rect));
        let composite = seen.unwrap_or(phys_rect);
        self.out.images.push(ImageDrawRow {
            // Just the registration id — the backend looks it
            // up in its texture cache; the encoder already
            // resolved fit into `rect` + UV. A `GpuView` row is
            // identical (its `id` is the off-screen target's),
            // so the draw stays uniform; `target` below only
            // schedules the off-screen paint.
            id: p.handle,
            instance: ImageInstance {
                rect: composite,
                uv_min: p.uv_min,
                uv_size: p.uv_size,
                tint: p.tint.into(),
                flags: p.flags,
                ..bytemuck::Zeroable::zeroed()
            },
        });
        // A `GpuView` also needs its off-screen target painted: list it with
        // the size it will be allocated at, where that sits in the view, the
        // display and raster scales, and the app paint callback riding
        // alongside the payload. The draw above already composites the result
        // by `id`.
        //
        // **Sized to what is on screen, not to the rect.** Layout is allowed to
        // hand back a rect larger than the surface — the contains-content rule
        // says a node overflows its parent rather than clipping its own content
        // — and a scroll can put most of a view outside its viewport. Following
        // the rect would allocate, and ask the app to draw, pixels that are
        // then thrown away: a status line long enough to widen the window's
        // root is enough to do it, which is how this was found.
        if let Some(paint) = paint {
            let scale = self.out.display.scale_factor;
            let cap = i64::from(self.composer.max_texture_dim.get());
            let whole = phys_rect.size;
            // The cap is measured against the *whole* view, so how much a view
            // is downsampled does not change with how much of it happens to be
            // scrolled into sight — a target that resampled itself as the view
            // slid past would shimmer.
            let downsample =
                (self.composer.max_texture_dim.get() as f32 / whole.w.max(whole.h)).min(1.0);
            let px = |v: f32| ((v * downsample).ceil() as i64).clamp(1, cap) as u32;
            // Floored at zero rather than at one, unlike a size: a target has
            // to have a pixel in it, and a corner has to be allowed to be the
            // origin — which it is for every view nothing clips.
            let at = |v: f32| ((v * downsample).floor() as i64).clamp(0, cap) as u32;
            let (used, offset) = match seen {
                Some(seen) => (seen.size, seen.min - phys_rect.min),
                None => (whole, Vec2::ZERO),
            };
            // The window lands inside the view without being made to: an origin
            // that rounds down and a size that rounds up sum to less than
            // `offset + used + 1`, and the only integer that can be is at most
            // the rounded-up whole. So `offset + used <= full` holds of the
            // arithmetic rather than of a clamp, which is why there is none —
            // it could never fire. Pinned by
            // `compose_gpu_view_caps_wide_and_tall_targets_uniformly`.
            self.out.frame_targets.push(RenderTargetDraw {
                id: p.handle,
                used: UVec2::new(px(used.w), px(used.h)),
                full: UVec2::new(px(whole.w), px(whole.h)),
                offset: UVec2::new(at(offset.x), at(offset.y)),
                display_scale: scale,
                raster_scale: geometry::phys_scale(self.composer.transform.current(), scale)
                    * downsample,
                paint: paint.clone(),
            });
        }
    }

    fn curve(&mut self, p: DrawCurvePayload) {
        let scale = self.out.display.scale_factor;
        let xform = self.composer.transform.current();
        let width_phys = p.width * geometry::phys_scale(xform, scale);
        let cap = p.cap;
        let bbox_urect = StrokeBbox {
            xform,
            bbox: p.bounds.cull_rect(),
            origin: p.origin,
            width_phys,
            cap,
            join: None,
            display: self.out.display,
        }
        .urect();
        // Clip-cull + batch-close: a curve sits above text in the
        // kind order (same as mesh/image), so a surviving draw
        // closes the open text batch first.
        if !self.admit_higher_kind(PaintTier::Curve, bbox_urect) {
            return;
        }
        // Owner origin folds in here so the record stays owner-local
        // (cross-frame stable). No pixel snapping — snapping geometry
        // would warp the traced shape; the AA fringe lives in the
        // shader.
        let to_phys = geometry::phys_point_map(xform, p.origin, scale);
        // Both bases below rotate about the pivot exactly — a Bézier by
        // affine invariance, a circle by moving its centre and shifting
        // both angles.
        let spin = p.bounds.spin();
        // Style lanes are basis-independent; each arm below fills in
        // the geometry and its own `kind`.
        let color: ColorU8 = p.fill.color.into();
        let proto = CurveInstance {
            width: width_phys,
            color0: color,
            color1: color,
            cap: cap_lanes(cap as u32, cap as u32),
            fill_kind: p.fill.kind,
            fill_lut_row: p.fill.lut_row,
            ..bytemuck::Zeroable::zeroed()
        };
        let (proto, n) = match p.basis {
            CurveBasis::Cubic { p0, p1, p2, p3 } => {
                let mut ctrl = [p0, p1, p2, p3];
                if let Some(spin) = spin {
                    let rotor = spin.rotor();
                    for q in &mut ctrl {
                        *q = rotor.apply(*q);
                    }
                }
                let [p0, p1, p2, p3] = ctrl.map(to_phys);
                // Adaptive sub-instance count from the post-transform
                // control-polygon length. Polygon length bounds arc
                // length from above — slight overshoot, but never
                // undershoots → no faceting from too-coarse sampling.
                // Near-straight cubics (`Shape::line` lowers as one;
                // graph wires often relax to one) short-circuit to a
                // single instance: every chord of a flat curve lies on
                // the segment, so the 16 baked chords render it exactly
                // at any length.
                let n = if geometry::cubic_is_flat(p0, p1, p2, p3) {
                    1
                } else {
                    let l = (p1 - p0).length() + (p2 - p1).length() + (p3 - p2).length();
                    geometry::sub_instance_count(l)
                };
                let proto = CurveInstance {
                    p0,
                    p1,
                    p2,
                    p3,
                    kind: CURVE_KIND_CUBIC,
                    ..proto
                };
                (proto, n)
            }
            CurveBasis::Arc {
                center,
                radius,
                mut a0,
                mut a1,
            } => {
                let mut center = center;
                if let Some(spin) = spin {
                    center = spin.rotor().apply(center);
                    a0 += spin.angle;
                    a1 += spin.angle;
                }
                // The transform stack is translate + uniform scale (no
                // rotation/skew — see `TranslateScale`), so a circle
                // maps to a circle: transform the centre, scale the
                // radius. Angles pass through untouched.
                let radius_phys = radius * geometry::phys_scale(xform, scale);
                // Adaptive sub-instance count from the *exact* arc
                // length `r·|sweep|` — no control-polygon overshoot.
                // Same ~1.5 px chord target as the cubic path; at that
                // density the chord sagitta is `≈ c²/(8r)` ≤ 0.3 px
                // even at r = 1, buried under the AA fringe.
                let n = geometry::sub_instance_count(radius_phys * (a1 - a0).abs());
                let proto = CurveInstance {
                    p0: to_phys(center),
                    p1: Vec2::new(radius_phys, 0.0),
                    p2: Vec2::new(a0, a1),
                    p3: Vec2::ZERO,
                    kind: CURVE_KIND_ARC,
                    ..proto
                };
                (proto, n)
            }
        };
        self.push_sub_instances(n, proto);
    }

    fn polyline(&mut self, p: DrawPolylinePayload) {
        let scale = self.out.display.scale_factor;
        let display = self.out.display;
        let mode = p.color_mode;
        let cap = p.cap;
        let join = p.join;
        let xform = self.composer.transform.current();
        let width_phys = p.width * geometry::phys_scale(xform, scale);

        // Compute the inflated physical-px AABB once and
        // reuse it for cull and overlap tracking. Inflating
        // by the stroke's outer fringe means the cull never
        // trims a pixel the stroke would reach, and it
        // short-circuits before transforming the full point
        // list — the win for long dense point runs.
        // Clamped where it is built: the early return below is what skips
        // the kept-point walk, and `admit_higher_kind` wants this same
        // clipped rect rather than a second answer to the same question.
        let visible = self.composer.clip.clamped(
            StrokeBbox {
                xform,
                bbox: p.bounds.cull_rect(),
                origin: p.origin,
                width_phys,
                cap,
                join: (p.points_len > 2).then_some(join),
                display,
            }
            .urect(),
        );
        if visible.is_paint_empty() {
            return;
        }

        let pts_start = p.points_start as usize;
        let pts_end = pts_start + p.points_len as usize;
        let cs_start = p.colors_start as usize;
        let cs_end = cs_start + p.colors_len as usize;
        let src_points = &self.store.polyline_points[pts_start..pts_end];
        let src_colors = &self.store.polyline_colors[cs_start..cs_end];

        // Transform points into physical-px. Owner-local
        // origin is folded in here so points stay owner-
        // local in the record store (cross-frame stable). No
        // pixel-snap — snapping stroke verts shifts thin
        // lines off-axis. Hairline regime (<1 phys px) is
        // the shader's trapezoid-plateau coverage.
        self.composer.polyline.points.clear();
        let to_phys = geometry::phys_point_map(xform, p.origin, scale);
        // The spin is lifted out of the run rather than tested per point:
        // it rotates each owner-local point about the pivot before the
        // ancestor transform places it, so the shape turns in place.
        if let Some(spin) = p.bounds.spin() {
            let rotor = spin.rotor();
            self.composer
                .polyline
                .points
                .extend(src_points.iter().map(|&q| to_phys(rotor.apply(q))));
        } else {
            self.composer
                .polyline
                .points
                .extend(src_points.iter().map(|&q| to_phys(q)));
        }

        // Keep only points beyond the coincidence threshold
        // from their predecessor — degenerate segments
        // contribute no geometry and their colors drop
        // with them.
        self.composer.polyline.kept.clear();
        let mut prev: Option<Vec2> = None;
        for (i, &q) in self.composer.polyline.points.iter().enumerate() {
            if prev.is_none_or(|p| (q - p).length_squared() > geometry::POLYLINE_COINCIDENT_EPS_SQ)
            {
                self.composer.polyline.kept.push(i as u32);
                prev = Some(q);
            }
        }
        if self.composer.polyline.kept.len() < 2 {
            return;
        }
        // Only now that the polyline will actually emit
        // geometry — an empty or culled polyline must not
        // split the batch or the group.
        if !self.admit_higher_kind(PaintTier::Curve, visible) {
            return;
        }
        let PolylineScratch {
            points,
            kept,
            directions,
        } = &mut self.composer.polyline;
        directions.clear();
        directions.extend(
            kept.windows(2)
                .map(|pair| (points[pair[1] as usize] - points[pair[0] as usize]).normalize()),
        );
        let pts = points.as_slice();
        let kept = kept.as_slice();
        let directions = directions.as_slice();
        let pt = |k: usize| pts[kept[k] as usize];
        // Segment color(s) for the kept segment `k → k+1`, indexed
        // through `kept` so the lookup lands on the *original* point
        // index — a coincident point dropped above takes its color
        // with it.
        let seg_colors = |k: usize| -> (ColorU8, ColorU8) {
            match mode {
                ColorMode::Single => (src_colors[0], src_colors[0]),
                ColorMode::PerPoint => (
                    src_colors[kept[k] as usize],
                    src_colors[kept[k + 1] as usize],
                ),
                ColorMode::PerSegment => {
                    let c = src_colors[kept[k + 1] as usize - 1];
                    (c, c)
                }
            }
        };
        let user_cap = cap as u32;
        let n_segs = directions.len();
        for k in 0..n_segs {
            // Pre-oriented bisector clip planes for the
            // joint ends, riding the neighbor lanes ("keep"
            // is `dot(x - endpoint, n) <= 0` in the shader);
            // zero = cap end, no clip.
            let n_start = if k > 0 {
                -(directions[k - 1] + directions[k])
            } else {
                Vec2::ZERO
            };
            let n_end = if k + 1 < n_segs {
                directions[k] + directions[k + 1]
            } else {
                Vec2::ZERO
            };
            let butt = LineCap::Butt as u32;
            let start_cap = if k == 0 { user_cap } else { butt };
            let end_cap = if k + 1 == n_segs { user_cap } else { butt };
            let (color, color1) = seg_colors(k);
            self.out.curves.push(CurveInstance {
                p0: pt(k),
                p1: n_start,
                p2: n_end,
                p3: pt(k + 1),
                t0: 0.0,
                t1: 1.0,
                width: width_phys,
                color0: color,
                color1,
                cap: cap_lanes(start_cap, end_cap),
                kind: CURVE_KIND_SEGMENT,
                ..bytemuck::Zeroable::zeroed()
            });
        }
        // One chrome instance per interior joint fills the
        // convex wedge between the two segment end faces.
        // The face-plane normals ride the neighbor lanes
        // pre-oriented for the shader's keep test
        // (`p1 = -d_a`, `p2 = d_b`). Chrome paints with the
        // average of the adjacent colors.
        for k in 1..n_segs {
            let d_a = directions[k - 1];
            let d_b = directions[k];
            let (_, ca) = seg_colors(k - 1);
            let (cb, _) = seg_colors(k);
            let color = ca.midpoint(cb);
            self.out.curves.push(CurveInstance {
                p0: pt(k),
                p1: -d_a,
                p2: d_b,
                t0: 0.0,
                t1: 1.0,
                width: width_phys,
                color0: color,
                color1: color,
                kind: geometry::polyline_join_kind(d_a, d_b, join),
                ..bytemuck::Zeroable::zeroed()
            });
        }
    }

    fn text(&mut self, t: DrawTextPayload) {
        let ScaledRect {
            phys: phys_rect,
            urect: unclipped,
        } = self.scaled_rect(t.rect);
        // `bounds` feeds the batch GPU scissor (union of the
        // batch's runs — see the strict-bounds rule below) and
        // the backend's per-line y-cull; there is no per-glyph
        // clip. Intersect with the active clip-stack top so
        // ancestor `clip = true` panels actually clip glyphs;
        // an empty intersection means the run can't reach
        // pixels — skip the push entirely (cull).
        let bounds = self.composer.clip.clamped(unclipped);
        if bounds.is_paint_empty() {
            return;
        }
        // Text sits below mesh/image/curve/polyline in the
        // kind order — flush if any prior higher-kind draw in
        // the group overlaps so this text doesn't get
        // reordered above it. (No need to check quads: text
        // paints over quads anyway.)
        if self.composer.higher_kinds.any_overlap(bounds) {
            self.flush();
        }
        // Batch GPU scissor = `open_grid.union` (union of every
        // run's `bounds` in the batch). The text shader has
        // no per-instance clip, so a "strict" run — one
        // whose ancestor clip cuts the unclipped extent —
        // can only batch with peers whose `bounds` matches
        // exactly; anything wider would let the strict
        // run's glyphs paint past their intended clip.
        // Non-strict-with-non-strict coalesces freely.
        let new_strict = bounds != unclipped;
        if let Some(b) = self.composer.batch.open.as_ref()
            && (b.strict || new_strict)
            && self.composer.batch.open_grid.union != bounds
        {
            self.close_batch();
        }
        // open_batch must run BEFORE the text push so the
        // batch's `texts_start` captures this run's index.
        let b = self.open_batch();
        b.strict |= new_strict;
        self.out.texts.push(TextDrawRow {
            origin: phys_rect.min,
            bounds,
            // Linear ColorU8 straight to the text backend.
            // Palantir's native text shader (see
            // `src/renderer/backend/text/`) consumes linear
            // bytes and premultiplies at output — matching
            // the rest of the renderer's pipelines. No sRGB
            // roundtrip.
            color: t.color.into(),
            text: t.text,
            // Snap the ancestor-transform component of the
            // text scale to discrete 0.5% steps. Continuous
            // zoom would otherwise mint a fresh glyph
            // cache key every frame (subpixel font size +
            // bin shift), forcing swash to re-rasterize
            // every glyph. Snapping stabilizes the key
            // across small zoom deltas so the atlas hits.
            // Quads/meshes keep continuous scale — only
            // text glyph crispness "steps."
            scale: geometry::snap_text_scale(self.composer.transform.scale()),
        });
        self.composer.batch.open_grid.push(bounds);
    }
}

impl ComposeSession<'_> {
    /// Reduce a quad-tier draw's geometry to physical space. Each arm owns
    /// both reused `Quad` lanes: a rect fills them with scaled corner
    /// radii and its brush/shadow axis, a triangle with its packed corner
    /// points. Everything past this point is shape-blind.
    fn pack_quad(&self, p: &DrawQuadPayload) -> PackedQuad {
        let xform = self.composer.transform.current();
        let scale_phys = geometry::phys_scale(xform, self.out.display.scale_factor);
        match p.geom {
            QuadGeom::Rect { rect, corners } => {
                PackedQuad {
                    rect: self.scaled_rect(rect),
                    corners: corners.scaled_by(scale_phys),
                    // Live shadow parameters are logical-px scalars;
                    // scale them so the shader's `local` coords line
                    // up. A gradient axis is already unit-space and
                    // passes through untouched.
                    fill_axis: if p.fill.kind.is_shadow() {
                        p.fill_axis.scaled(scale_phys)
                    } else {
                        p.fill_axis
                    },
                    stroke_width: p.stroke.width * scale_phys,
                }
            }
            QuadGeom::Triangle {
                origin,
                a,
                b,
                c,
                radius,
            } => {
                let scale = self.out.display.scale_factor;
                // Fold owner origin + active transform, scale to physical
                // px. No pixel-snap — the SDF handles sub-pixel placement;
                // snapping the covering rect would only shift the AA band.
                let xf = geometry::phys_point_map(xform, origin, scale);
                let (a, b, c) = (xf(a), xf(b), xf(c));
                let radius_phys = (radius * scale_phys).max(0.0);
                // Covering AABB: the rounded shape (the SDF offsets the
                // triangle outward by `radius` to round its corners) plus
                // the ½px AA fringe. The stroke sits on the *inner* edge
                // (like a rounded rect), so it adds no outward reach.
                let lo = a.min(b).min(c);
                let hi = a.max(b).max(c);
                let phys_rect = Rect::from_min_max(lo, hi).inflated(radius_phys + HALF_FRINGE);
                // Pack the three points in rect-local coords (0..size,
                // matching the shader's `in.local`) + the corner radius
                // into the reused `corners` / `fill_axis` lanes;
                // `FillKind::TRIANGLE` tells the shader to read them as a
                // triangle SDF rather than rounded-rect radii / gradient
                // axis.
                let al = a - phys_rect.min;
                let bl = b - phys_rect.min;
                let cl = c - phys_rect.min;
                PackedQuad {
                    rect: ScaledRect::from_phys(phys_rect, self.out.display.physical),
                    corners: Corners::from_array([al.x, al.y, bl.x, bl.y]),
                    fill_axis: FillAxis::from_lanes(cl.x, cl.y, radius_phys, 0.0),
                    stroke_width: (p.stroke.width * scale_phys).max(0.0),
                }
            }
        }
    }

    /// Clear fold: an opaque solid sharp unclipped quad covering the whole
    /// viewport paints exactly what `LoadOp::Clear(fill)` would — every
    /// covered pixel is deep inside the SDF (coverage exactly 1.0), so the
    /// outputs are bit-identical. And being opaque over every pixel, it
    /// hides *everything painted before it*. So: discard the whole scene
    /// composed so far and record the fill as the pass clear — the frame
    /// effectively starts at the last such cover.
    ///
    /// The root window background is the common case (cover at position 0,
    /// nothing to discard); a fullscreen page/panel painted over an
    /// underlay drops the entire hidden underlay too. The active clip must
    /// be empty: a scissored cover only hides its scissor, and an empty
    /// clip state also guarantees no group in flight references
    /// `rounded_clips` state the discard wipes.
    ///
    /// Only a rect can reach this: the `SOLID` test rules out shadows and
    /// triangles, which carry their own `FillKind`. Sharpness is read off
    /// the *packed* values, the same ones the fragment fast path reads —
    /// scaling by a positive factor cannot make a zero radius nonzero, so
    /// the test only ever tightens.
    ///
    /// Returns `true` when the quad was folded and must not be emitted.
    fn fold_into_clear(&mut self, p: &DrawQuadPayload, packed: &PackedQuad) -> bool {
        let phys = packed.rect.phys;
        let covers_viewport = phys.min.x <= EPS
            && phys.min.y <= EPS
            && phys.max().x >= self.out.display.physical.as_vec2().x - EPS
            && phys.max().y >= self.out.display.physical.as_vec2().y - EPS;
        if !covers_viewport
            // Any frame at all, not just one with a rounded chain: a
            // non-empty chain implies a frame, so testing both asked one
            // question twice.
            || self.composer.clip.top().is_some()
            || p.fill.kind != FillKind::SOLID
            || !p.fill.color.is_opaque()
            || !packed.is_sharp()
        {
            return false;
        }
        self.discard_composed();
        self.out.clear_override = Some(p.fill.color.unpack());
        true
    }

    /// Opaque-cover annotation for the occlusion pruner, `SOLID`-only —
    /// which is what keeps it off the two shapes that would be wrong to
    /// record: a shadow's blur reaches past its rect, and a triangle
    /// covers only its interior, not the whole `rect`.
    fn record_opaque_cover(&mut self, p: &DrawQuadPayload, packed: &PackedQuad, fast: bool) {
        if p.fill.kind != FillKind::SOLID || !p.fill.color.is_opaque() {
            return;
        }
        let inscribed = packed.rect.phys.inscribed_for_corners(packed.corners);
        let stroke_inset = if noop_f32(packed.stroke_width) || p.stroke.color.is_opaque() {
            0.0
        } else {
            packed.stroke_width
        };
        let aa_inset = if fast { 0.0 } else { AA_RADIUS };
        let cover = inscribed.deflated_by(Spacing::all(stroke_inset + aa_inset));
        if !cover.is_paint_empty() {
            let idx = self.out.quads.len() as u32 - 1 - self.composer.cursors.quads;
            self.composer.occlusion.record_opaque(idx, cover);
        }
    }

    /// Close the in-flight group: if anything was emitted into it,
    /// push a `DrawGroup` covering the open slice; either way advance
    /// the per-kind cursors and clear the overlap scratches. Scissor
    /// + rounded clip are preserved for the next group.
    fn flush(&mut self) {
        let composer = &mut *self.composer;
        composer.occlusion.prune(self.out, composer.cursors.quads);
        let q_end = self.out.quads.len() as u32;
        let t_end = self.out.texts.len() as u32;
        let higher_end = PaintTier::ALL.map(|tier| self.out.draws_len(tier));
        if q_end > composer.cursors.quads
            || t_end > composer.cursors.texts
            || PaintTier::ALL
                .iter()
                .any(|&t| higher_end[t.idx()] > composer.cursors.higher[t.idx()])
        {
            // Push the higher-kind batches BEFORE the group itself so
            // their `last_group` matches the in-flight group's
            // eventual index (= current `out.groups.len()`).
            let last_group = self.out.groups.len() as u32;
            for tier in PaintTier::ALL {
                let start = composer.cursors.higher[tier.idx()];
                let end = higher_end[tier.idx()];
                if end > start {
                    self.out.batches_mut(tier).push(GroupBatch {
                        items: (start..end).into(),
                        last_group,
                    });
                }
            }
            self.out.groups.push(DrawGroup {
                scissor: composer.clip.scissor(),
                rounded_clips: composer.clip.chain(),
                quads: (composer.cursors.quads..q_end).into(),
            });
        }
        composer.cursors = GroupCursors {
            quads: q_end,
            texts: t_end,
            higher: higher_end,
        };
        composer.higher_kinds.clear();
        composer.occlusion.clear();
        // Closed-batch text is group-scoped: once we cross a group
        // boundary, any batch closed *in* this group has rendered (it
        // drains at its `last_group`), so its rects no longer gate quads.
        // The open-batch grid is NOT cleared here — it spans groups with
        // its (still-open) batch.
        composer.batch.closed_grid.clear();
        composer.batch.pending_batch_cursor = self.out.text_batches.len();
    }

    /// Finalize the open text batch (if any): push a [`TextBatch`]
    /// entry covering `batch_texts_start..out.texts.len()`. No-op when no
    /// batch is active. Called at batch-split events — rounded-clip
    /// change, a higher-kind append, or a strict-bounds mismatch. The
    /// finalized output remains pending for the group-scoped closed
    /// check, so a later quad still flushes for already-closed text that
    /// shares this group. The grid fill is deferred to [`Self::closed_hit`].
    fn close_batch(&mut self) {
        let Some(b) = self.composer.batch.open.take() else {
            return;
        };
        let texts_end = self.out.texts.len() as u32;
        let scissor = self.composer.batch.open_grid.union;
        self.composer.batch.open_grid.clear();
        // Invariants the schedule cursor relies on: batches are pushed
        // in walk order so `last_group` is monotonically non-decreasing
        // (multiple batches can anchor to the same group when a mesh
        // splits mid-group), and their `texts` spans concatenate
        // without gaps in `out.texts`.
        debug_assert!(
            self.out
                .text_batches
                .last()
                .is_none_or(|prev| prev.last_group <= b.last_group),
        );
        debug_assert!(
            self.out
                .text_batches
                .last()
                .is_none_or(|prev| prev.texts.start + prev.texts.len == b.texts_start),
        );
        self.out.text_batches.push(TextBatch {
            texts: (b.texts_start..texts_end).into(),
            last_group: b.last_group,
            // `scissor` is already in physical pixels and clamped to
            // every contributing run's clip-stack-narrowed bounds, so it
            // is the GPU scissor for this batch. It has to be: the text
            // backend implements no per-run shader clipping, so a
            // scissor any wider than this would let a clipped run's
            // glyphs paint past their intended bound.
            scissor,
            // Every close site runs while the outgoing clip is still the
            // stack top (`break_for_clip` closes ahead of the push/pop),
            // so this is the chain all the batch's runs were recorded
            // under.
            rounded_clips: self.composer.clip.chain(),
        });
    }

    /// Return a mutable handle to the open batch, opening a fresh one
    /// when none exists. Idempotent within a batch — repeated calls
    /// reuse the same `OpenBatch` and only refresh `last_group` to
    /// the in-flight group's eventual index.
    fn open_batch(&mut self) -> &mut OpenBatch {
        // Read before the borrow of `composer.batch`, which is what keeps
        // a fresh batch's own group index reachable here.
        let last_group = self.out.groups.len() as u32;
        let texts_start = self.out.texts.len() as u32;
        let b = self.composer.batch.open.get_or_insert(OpenBatch {
            texts_start,
            last_group,
            strict: false,
        });
        b.last_group = last_group;
        b
    }

    /// Cull a higher-kind (mesh / image / curve) draw against the active
    /// clip and, if it survives, close any open text batch. Higher-kind
    /// geometry paints above text under the backend's kind reorder, and a
    /// batch renders at the END of its last group — past this draw if left
    /// open — so the batch must close here for its text to emit first. Done
    /// only after the cull: a culled draw must not split the batch. Also
    /// flushes the group when the draw cross-kind-conflicts with an earlier
    /// higher-kind draw (see [`HigherKindRects::conflicts`]), and then
    /// records the draw's own rect for the group's overlap tracking (after
    /// the flush, so it isn't wiped with the previous group's rects).
    /// Returns `false` when culled — the caller should `continue`.
    ///
    /// Polyline calls this only after its kept-point walk proves the
    /// stroke emits geometry (an all-coincident polyline must not split
    /// the batch), gated behind an early cull.
    ///
    /// [`HigherKindRects::conflicts`]: crate::renderer::frontend::composer::higher_kind::HigherKindRects::conflicts
    fn admit_higher_kind(&mut self, tier: PaintTier, bounds: URect) -> bool {
        // Clipped first, so what this tier registers as occupied is what
        // it paints — the same rect the quad tier and the text tier test
        // and record.
        let bounds = self.composer.clip.clamped(bounds);
        if bounds.is_paint_empty() {
            return false;
        }
        self.close_batch();
        if self.composer.higher_kinds.conflicts(tier, bounds) {
            self.flush();
        }
        self.composer.higher_kinds.push(tier, bounds);
        true
    }

    /// Force a flush / batch-close if a quad-tier draw at `overlap`
    /// overlaps something in the group that would be reordered above it.
    /// Quad is the lowest paint kind, so any higher-kind draw it overlaps
    /// would paint *under* it after the backend's intra-group reorder —
    /// flush to keep record order. Text overlap is checked against both
    /// the open batch's grid (which may span groups) and
    /// batches already closed in this group ([`Self::closed_hit`]);
    /// an open-batch hit additionally closes the batch so its text can't
    /// coalesce forward and re-cover this quad. The open check goes
    /// straight to the tiled grid — `any_overlap` pre-rejects on its
    /// internal union AABB, so no caller-side pre-reject is needed.
    fn quad_forces_flush(&mut self, overlap: URect) {
        // Text painted in (or scheduled after) this group sits in two
        // places: the open batch (`open_grid`, spans groups with its
        // batch) and batches already closed within this group
        // (`closed_grid`). A quad overlapping either would be painted
        // *under* that text by the backend's quads→text order, so flush so
        // the text renders first.
        //
        // An open-batch hit additionally *closes* the batch: leaving it
        // open would let the overlapping run coalesce forward and schedule
        // at a later `last_group`, re-covering this quad. A closed-grid
        // hit needs no close — that text's batch is already finalized at
        // this group; flushing alone puts the quad in the next group.
        if self.composer.batch.open_grid.any_overlap(overlap) {
            self.close_batch();
            self.flush();
        } else if self.closed_hit(overlap) || self.composer.higher_kinds.any_overlap(overlap) {
            self.flush();
        }
    }

    /// `true` if `q` overlaps text of a batch closed within the
    /// in-flight group. Finalized batches remain pending in
    /// `out.text_batches`; the first query whose `q` hits a pending
    /// batch scissor drains every pending batch into the closed grid.
    /// Later queries use the grid, and groups nothing probes near
    /// closed text never pay the per-rect fill.
    fn closed_hit(&mut self, q: URect) -> bool {
        let batch = &mut self.composer.batch;
        let pending = &self.out.text_batches[batch.pending_batch_cursor..];
        if pending.iter().any(|b| b.scissor.intersects(q)) {
            for b in pending {
                for ti in b.texts.range() {
                    batch.closed_grid.push(self.out.texts[ti].bounds);
                }
            }
            batch.pending_batch_cursor = self.out.text_batches.len();
        }
        batch.closed_grid.any_overlap(q)
    }

    /// Push `frame` as the clip in force, closing the batch and group
    /// first if it differs from the one it replaces.
    ///
    /// The break runs **before** the stack moves, because [`Self::flush`]
    /// stamps the closing group with the stack top: the outgoing clip has
    /// to still be on top when it does.
    ///
    /// Named apart from the [`PaintSink`] pair that calls this one and
    /// [`Self::leave_clip`]. Sharing a name would leave those trait bodies
    /// terminating only on inherent methods winning resolution, and a
    /// later rename here would turn `pop_clip` into unbounded recursion
    /// that still compiles.
    fn enter_clip(&mut self, frame: ClipFrame) {
        self.break_for_clip(Some(frame));
        self.composer.clip.push(frame);
    }

    /// Restore the parent clip. Named apart from the trait pair for the
    /// reason [`Self::enter_clip`] gives.
    fn leave_clip(&mut self) {
        let parent = self.composer.clip.parent();
        self.break_for_clip(parent);
        self.composer.clip.pop();
    }

    /// Close what the clip in force owns, if `next` differs from it.
    /// Chains compare by value, so a same-clip push/pop is a no-op and
    /// accumulated overlap state persists through redundant transitions.
    fn break_for_clip(&mut self, next: Option<ClipFrame>) {
        let next_chain = next.map_or(Span::default(), |frame| frame.chain);
        let chain_changed = !self
            .out
            .chains_equal(next_chain, self.composer.clip.chain());
        if chain_changed {
            // The stencil mask stack is tied to the active chain; batched
            // text under the wrong masks would either over- or
            // under-clip. Close before the group transition, while the
            // stack top still names the batch's chain.
            self.close_batch();
        }
        if next.map(|frame| frame.scissor) != self.composer.clip.scissor() || chain_changed {
            self.flush();
        }
    }

    /// Clear-fold discard: a fullscreen opaque cover proved everything
    /// composed so far invisible — drop the scene output and every piece of
    /// scratch that describes it. The *walk* state survives: the clip stack
    /// is empty by the fold's precondition, and the transform stack stays
    /// untouched (the cover may sit under an active transform whose pops
    /// are still ahead in the stream).
    fn discard_composed(&mut self) {
        self.out.discard_scene();
        self.composer.reset_group_scratch(self.out.display.physical);
    }
}
