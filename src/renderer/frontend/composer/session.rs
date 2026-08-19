//! One frame being composed, and the sink the paint calls arrive through.

use crate::display::Display;
use crate::icons::icon_raster_key::IconRasterKey;
use crate::primitives::approx::{EPS, noop_f32};
use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::color::ColorU8;
use crate::primitives::corners::Corners;
use crate::primitives::fill_wire::FillKind;
use crate::primitives::spacing::Spacing;
use crate::primitives::span::Span;
use crate::primitives::{
    num::{F32Ext, Vec2Ext},
    rect::Rect,
    transform::TranslateScale,
    urect::URect,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::{
    DrawCurvePayload, DrawIconPayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload,
    DrawQuadPayload, DrawTextPayload, PushClipPayload, QuadGeom,
};
use crate::renderer::gpu_view::GpuPaintRef;
use crate::renderer::quad::{AA_RADIUS, Quad};
use crate::renderer::render_buffer::batch::PaintTier;
use crate::renderer::render_buffer::curve::{
    CURVE_KIND_ARC, CURVE_KIND_CUBIC, CURVE_KIND_SEGMENT, CurveInstance, cap_lanes,
};
use crate::renderer::render_buffer::icon::IconDrawRow;
use crate::renderer::render_buffer::image::{ImageDrawRow, ImageInstance, RenderTargetDraw};
use crate::renderer::render_buffer::mesh::{MeshDraw, MeshDrawRow, MeshInstance};
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::renderer::render_buffer::{MAX_ROUNDED_CLIP_DEPTH, RenderBuffer, RoundedClip};
use crate::scene::record_store::RecordPayloads;
use crate::scene::shapes::paint::CurveBasis;
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::LineCap;
use glam::{UVec2, Vec2};

use crate::renderer::frontend::composer::geometry::POLYLINE_COINCIDENT_EPS_SQ;
use crate::renderer::frontend::composer::geometry::{
    cubic_is_flat, polyline_join_kind, push_sub_instances, rounded_clip_depth_overflow,
    scissor_from_logical, snap_text_scale, stroke_bbox_urect, sub_instance_count, urect_from_phys,
};
use crate::renderer::frontend::composer::{ClipFrame, Composer, PolylineScratch};

/// One compose pass in flight: the running walk transform bound to the
/// `Composer`'s retained scratch, the record payloads variable-length
/// draws read from, and the buffer being filled.
///
/// Paint streams in through [`PaintSink`], one call per lowered draw, in
/// authoring order. Clip and transform *stacks* stay on the `Composer`
/// so their capacity survives across frames; only the live transform
/// product rides here, because it is per-pass state with no allocation.
#[derive(Debug)]
pub(crate) struct ComposeSession<'a> {
    pub(super) composer: &'a mut Composer,
    pub(super) payloads: &'a RecordPayloads,
    pub(super) out: &'a mut RenderBuffer,
    pub(super) display: Display,
    pub(super) current_transform: TranslateScale,
}

/// A quad-tier draw reduced to physical space — everything
/// [`ComposeSession::quad`]'s per-shape half derives, and its shared
/// half consumes. `corners` and `fill_axis` are the two `Quad` lanes
/// whose meaning depends on the shape: corner radii + brush/shadow axis
/// for a rect, packed corner points + third point/radius for a triangle.
#[derive(Debug)]
struct PackedQuad {
    phys_rect: Rect,
    /// Viewport-clamped integer bounds — what culling and the group's
    /// overlap tracking test against.
    urect: URect,
    corners: Corners,
    fill_axis: FillAxis,
    stroke_width: f32,
}

/// A draw's rect in the two forms every rect-shaped handler needs, from
/// [`ComposeSession::scaled_rect`].
#[derive(Debug)]
struct ScaledRect {
    /// Physical px — what the emitted instance carries.
    phys: Rect,
    /// Viewport-clamped integer bounds — what culling and the group's
    /// overlap tracking test against.
    urect: URect,
}

impl ComposeSession<'_> {
    /// Apply the walk transform to a payload's logical rect, scale it to
    /// physical px, and derive its integer bounds — the opening move of
    /// `rect`, `shadow`, `image`, and `text`. Scaling happens once
    /// because the cull bounds and the emitted instance share the
    /// result, so a culled draw costs the same as an emitted one.
    fn scaled_rect(&self, rect: Rect) -> ScaledRect {
        let world = self.current_transform.apply_rect(rect);
        let phys = world.scaled_by(self.display.scale_factor, self.display.pixel_snap);
        ScaledRect {
            phys,
            urect: urect_from_phys(phys.min, phys.max(), self.display.physical),
        }
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
        let surface = self.display.physical;
        let mut clipped = whole.clamp_to(Rect::new(0.0, 0.0, surface.x as f32, surface.y as f32));
        if let Some(scissor) = self.composer.clip_scissor() {
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
        self.composer.close_batch(self.out);
        self.composer.flush(self.out);
    }
}

impl PaintSink for ComposeSession<'_> {
    fn clip(&mut self, p: PushClipPayload) {
        let scale = self.display.scale_factor;
        let snap = self.display.pixel_snap;
        let viewport_phys = self.display.physical;
        let logical_radius = (!p.corners.approx_zero()).then_some(p.corners);
        let world = self.current_transform.apply_rect(p.rect);
        let me = scissor_from_logical(world, scale, snap, viewport_phys);
        let parent = self.composer.clip_top();
        let scissor = match parent {
            Some(parent) => me.clamp_to(parent.scissor),
            None => me,
        };
        let parent_chain = parent.map_or(Span::default(), |f| f.chain);
        let chain = if let Some(logical_radius) = logical_radius {
            // Combine current transform's uniform scale with DPR
            // so radii match the painted SDF's physical size.
            let phys_scale = self.current_transform.scale * scale;
            // `mask_rect` stays unclamped — the SDF needs the
            // rect's true edges, otherwise corner curves
            // would shift inward when the clip partially
            // leaves the viewport.
            let rc = RoundedClip {
                mask_rect: world.scaled_by(scale, snap),
                corners: logical_radius.scaled_by(phys_scale),
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
                    rounded_clip_depth_overflow(depth);
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
        self.composer
            .push_clip(ClipFrame { scissor, chain }, self.out);
    }

    fn pop_clip(&mut self) {
        self.composer.pop_clip(self.out);
    }

    fn push_transform(&mut self, t: TranslateScale) {
        self.composer.transform_stack.push(self.current_transform);
        self.current_transform = self.current_transform.compose(t);
    }

    fn pop_transform(&mut self) {
        self.current_transform = self
            .composer
            .transform_stack
            .pop()
            .expect("PopTransform without matching PushTransform");
    }

    fn quad(&mut self, p: DrawQuadPayload) {
        let phys_scale = self.current_transform.scale * self.display.scale_factor;
        // Reduce the geometry to physical space. Each arm owns both
        // reused `Quad` lanes: a rect fills them with scaled corner
        // radii and its brush/shadow axis, a triangle with its packed
        // corner points. Everything after this point is shared.
        let packed = match p.geom {
            QuadGeom::Rect { rect, corners } => {
                let ScaledRect {
                    phys: phys_rect,
                    urect,
                } = self.scaled_rect(rect);
                // Clear fold: an opaque solid sharp unclipped quad
                // covering the whole viewport paints exactly what
                // `LoadOp::Clear(fill)` would — every covered pixel
                // is deep inside the SDF (coverage exactly 1.0), so
                // the outputs are bit-identical. And being opaque
                // over every pixel, it hides *everything painted
                // before it*. So: discard the whole scene composed
                // so far and record the fill as the pass clear —
                // the frame effectively starts at the last such
                // cover. The root window background is the common
                // case (cover at position 0, nothing to discard); a
                // fullscreen page/panel painted over an underlay
                // drops the entire hidden underlay too. The active
                // clip must be empty: a scissored cover only hides
                // its scissor, and an empty scissor state also
                // guarantees no group in flight references
                // `rounded_clips` state that `discard` wipes.
                //
                // Only a rect can reach this: the `SOLID` test rules
                // out shadows and triangles, which carry their own
                // `FillKind`.
                if self.composer.clip_scissor().is_none()
                    && self.composer.clip_chain().len == 0
                    && p.fill_kind == FillKind::SOLID
                    && p.fill.is_opaque()
                    && noop_f32(p.stroke.width)
                    && corners.approx_zero()
                    && phys_rect.min.x <= EPS
                    && phys_rect.min.y <= EPS
                    && phys_rect.max().x >= self.out.viewport_phys_f.x - EPS
                    && phys_rect.max().y >= self.out.viewport_phys_f.y - EPS
                {
                    self.composer.discard_composed(self.out);
                    self.out.clear_override = Some(p.fill.unpack());
                    return;
                }
                PackedQuad {
                    phys_rect,
                    urect,
                    corners: corners.scaled_by(phys_scale),
                    // Live shadow parameters are logical-px scalars;
                    // scale them so the shader's `local` coords line
                    // up. A gradient axis is already unit-space and
                    // passes through untouched.
                    fill_axis: if p.fill_kind.is_shadow() {
                        p.fill_axis.scaled(phys_scale)
                    } else {
                        p.fill_axis
                    },
                    stroke_width: p.stroke.width * phys_scale,
                }
            }
            QuadGeom::Triangle {
                origin,
                a,
                b,
                c,
                radius,
            } => {
                let scale = self.display.scale_factor;
                // Fold owner origin + active transform, scale to physical
                // px. No pixel-snap — the SDF handles sub-pixel placement;
                // snapping the covering rect would only shift the AA band.
                let xf = |q: Vec2| self.current_transform.apply_point(q + origin) * scale;
                let (a, b, c) = (xf(a), xf(b), xf(c));
                let radius_phys = (radius * phys_scale).max(0.0);
                // Covering AABB: the rounded shape (the SDF offsets the
                // triangle outward by `radius` to round its corners) plus
                // the ½px AA fringe. The stroke sits on the *inner* edge
                // (like a rounded rect), so it adds no outward reach.
                let lo = a.min(b).min(c);
                let hi = a.max(b).max(c);
                let phys_rect = Rect::from_min_max(lo, hi).inflated(radius_phys + 0.5);
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
                    urect: urect_from_phys(phys_rect.min, phys_rect.max(), self.display.physical),
                    phys_rect,
                    corners: Corners::from_array([al.x, al.y, bl.x, bl.y]),
                    fill_axis: FillAxis::from_lanes(cl.x, cl.y, radius_phys, 0.0),
                    stroke_width: (p.stroke.width * phys_scale).max(0.0),
                }
            }
        };
        // Clip-cull: skip emitting the quad when it sits
        // entirely outside the active scissor. The GPU
        // would scissor it away anyway; this saves the
        // `quads.push` + per-quad math.
        if self.composer.cull_bounds(packed.urect) {
            return;
        }
        self.composer.quad_forces_flush(packed.urect, self.out);
        // Fragment fast path: a solid, sharp, stroke-less
        // quad whose physical rect is pixel-aligned
        // rasterizes only interior fragments (SDF coverage
        // exactly 1.0) — flag the instance so the shader
        // returns the premultiplied fill directly, skipping
        // the SDF + composite path. Alignment is exact, not
        // approx: exactness is what makes the skip
        // bitwise-identical (host pixel snapping yields
        // exact integers when active; unsnapped fractional
        // rects keep the full SDF for edge AA). `SOLID`
        // again keeps shadows and triangles out.
        let pmax = packed.phys_rect.max();
        let fast = p.fill_kind == FillKind::SOLID
            && noop_f32(packed.stroke_width)
            && packed.corners.approx_zero()
            && packed.phys_rect.min.x.is_integral()
            && packed.phys_rect.min.y.is_integral()
            && pmax.x.is_integral()
            && pmax.y.is_integral();
        let fill_kind = if fast {
            p.fill_kind.with_fast()
        } else {
            p.fill_kind
        };
        self.out.quads.push(Quad {
            rect: packed.phys_rect,
            fill: p.fill,
            corners: packed.corners,
            stroke_color: p.stroke.color,
            stroke_width: packed.stroke_width,
            fill_kind,
            fill_lut_row: p.fill_lut_row,
            fill_axis: packed.fill_axis,
        });
        // Opaque-cover annotation, again `SOLID`-only — which is what
        // keeps it off the two shapes that would be wrong to record: a
        // shadow's blur reaches past its rect, and a triangle covers
        // only its interior, not the whole `rect`.
        if p.fill_kind == FillKind::SOLID && p.fill.is_opaque() {
            let inscribed = packed.phys_rect.inscribed_for_corners(packed.corners);
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
    }

    fn mesh(&mut self, p: DrawMeshPayload) {
        let scale = self.display.scale_factor;
        let viewport_phys = self.display.physical;
        // `draw_mesh` already gated empty/degenerate meshes
        // (`draw_mesh` applies its no-op gate), so `v_len >= 1` here.
        // Inflate by 0.5 phys-px to match polyline's AA-fringe
        // policy. Mesh today paints inside its vertex hull,
        // but a future AA edge or displacement shader would
        // silently produce false negatives — and false
        // negatives in the overlap test reorder paint. The
        // same inflated rect feeds the clip cull below.
        let world_bbox = self.current_transform.apply_rect(Rect {
            min: p.bbox.min + p.origin,
            size: p.bbox.size,
        });
        // Mesh skips snapping (matches polyline/curve), so it cannot use
        // `scaled_rect` — but the *scaling* still goes through
        // `Rect::scaled_by`, the same call `scaled_rect` makes, rather
        // than open-coding `* scale`. That is what keeps the cull
        // tracking the quad tier. Snap, transform and the AA fringe are
        // per-tier policy; the integer bounds below are `urect_from_phys`
        // like every other tier's.
        let phys_bbox = world_bbox.scaled_by(scale, false);
        let fringe = Vec2::splat(0.5);
        let mesh_urect = urect_from_phys(
            phys_bbox.min - fringe,
            phys_bbox.max() + fringe,
            viewport_phys,
        );
        // Clip-cull + batch-close: a mesh fully outside the
        // active scissor (e.g. scrolled out of an ancestor clip)
        // is skipped; a surviving one closes the open text batch
        // so its text emits before this above-text geometry.
        if !self
            .composer
            .enter_higher_kind(PaintTier::Mesh, mesh_urect, self.out)
        {
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
        let phys_scale = self.current_transform.scale * scale;
        let phys_translate =
            (self.current_transform.scale * p.origin + self.current_transform.translation) * scale;
        self.out.meshes.push(MeshDrawRow {
            draw: MeshDraw {
                vertices: (p.v_start..p.v_start + p.v_len).into(),
                indices: (p.i_start..p.i_start + p.i_len).into(),
            },
            instance: MeshInstance {
                translate: phys_translate,
                scale: phys_scale,
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
        if !self
            .composer
            .enter_higher_kind(PaintTier::Icon, urect, self.out)
        {
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
        let size = key.size.as_vec2();
        let centred = phys_rect.min + (Vec2::new(phys_rect.size.w, phys_rect.size.h) - size) * 0.5;
        self.out.icons.push(IconDrawRow {
            key,
            origin: centred.fast_round().as_ivec2(),
            color: p.tint.into(),
            desaturate: p.desaturate,
        });
    }

    fn image(&mut self, p: DrawImagePayload, paint: Option<&GpuPaintRef>) {
        let ScaledRect {
            phys: phys_rect,
            urect: image_urect,
        } = self.scaled_rect(p.rect);
        // Clip-cull + batch-close: image sits above text in the
        // kind order (same as mesh), so a surviving draw closes
        // the open text batch first.
        if !self
            .composer
            .enter_higher_kind(PaintTier::Image, image_urect, self.out)
        {
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
            let scale = self.display.scale_factor;
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
                raster_scale: self.current_transform.scale * scale * downsample,
                paint: paint.clone(),
            });
        }
    }

    fn curve(&mut self, p: DrawCurvePayload) {
        let scale = self.display.scale_factor;
        let xform = self.current_transform;
        let width_phys = p.width * xform.scale * scale;
        let cap = p.cap;
        let bbox_urect = stroke_bbox_urect(
            xform,
            p.bounds.cull_rect(),
            p.origin,
            width_phys,
            cap,
            None,
            self.display,
        );
        // Clip-cull + batch-close: a curve sits above text in the
        // kind order (same as mesh/image), so a surviving draw
        // closes the open text batch first.
        if !self
            .composer
            .enter_higher_kind(PaintTier::Curve, bbox_urect, self.out)
        {
            return;
        }
        // Owner origin folds in here so the record stays owner-local
        // (cross-frame stable). No pixel snapping — snapping geometry
        // would warp the traced shape; the AA fringe lives in the
        // shader.
        let to_phys = |q: Vec2| xform.apply_point(q + p.origin) * scale;
        // Both bases below rotate about the pivot exactly — a Bézier by
        // affine invariance, a circle by moving its centre and shifting
        // both angles.
        let spin = p.bounds.spin();
        // Style lanes are basis-independent; each arm below fills in
        // the geometry and its own `kind`.
        let color: ColorU8 = p.color.into();
        let proto = CurveInstance {
            width: width_phys,
            color0: color,
            color1: color,
            cap: cap_lanes(cap as u32, cap as u32),
            fill_kind: p.fill_kind,
            fill_lut_row: p.fill_lut_row,
            ..bytemuck::Zeroable::zeroed()
        };
        let (proto, n) = match p.basis {
            CurveBasis::Cubic { p0, p1, p2, p3 } => {
                let mut ctrl = [p0, p1, p2, p3];
                if let Some(spin) = spin {
                    let rotor = Vec2::from_angle(spin.angle);
                    for q in &mut ctrl {
                        *q = rotor.rotate(*q - spin.pivot) + spin.pivot;
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
                let n = if cubic_is_flat(p0, p1, p2, p3) {
                    1
                } else {
                    let l = (p1 - p0).length() + (p2 - p1).length() + (p3 - p2).length();
                    sub_instance_count(l)
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
                    center = Vec2::from_angle(spin.angle).rotate(center - spin.pivot) + spin.pivot;
                    a0 += spin.angle;
                    a1 += spin.angle;
                }
                // The transform stack is translate + uniform scale (no
                // rotation/skew — see `TranslateScale`), so a circle
                // maps to a circle: transform the centre, scale the
                // radius. Angles pass through untouched.
                let radius_phys = radius * xform.scale * scale;
                // Adaptive sub-instance count from the *exact* arc
                // length `r·|sweep|` — no control-polygon overshoot.
                // Same ~1.5 px chord target as the cubic path; at that
                // density the chord sagitta is `≈ c²/(8r)` ≤ 0.3 px
                // even at r = 1, buried under the AA fringe.
                let n = sub_instance_count(radius_phys * (a1 - a0).abs());
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
        push_sub_instances(self.out, n, proto);
    }

    fn polyline(&mut self, p: DrawPolylinePayload) {
        let scale = self.display.scale_factor;
        let display = self.display;
        let mode = p.color_mode;
        let cap = p.cap;
        let join = p.join;
        let width_phys = p.width * self.current_transform.scale * scale;

        // Compute the inflated physical-px AABB once and
        // reuse it for cull and overlap tracking. Inflating
        // by the stroke's outer fringe means the cull never
        // trims a pixel the stroke would reach, and it
        // short-circuits before transforming the full point
        // list — the win for long dense point runs.
        let bbox_urect = stroke_bbox_urect(
            self.current_transform,
            p.bounds.cull_rect(),
            p.origin,
            width_phys,
            cap,
            (p.points_len > 2).then_some(join),
            display,
        );
        if self.composer.cull_bounds(bbox_urect) {
            return;
        }

        let pts_start = p.points_start as usize;
        let pts_end = pts_start + p.points_len as usize;
        let cs_start = p.colors_start as usize;
        let cs_end = cs_start + p.colors_len as usize;
        let src_points = &self.payloads.polyline_points[pts_start..pts_end];
        let src_colors = &self.payloads.polyline_colors[cs_start..cs_end];

        // Transform points into physical-px. Owner-local
        // origin is folded in here so points stay owner-
        // local in the record store (cross-frame stable). No
        // pixel-snap — snapping stroke verts shifts thin
        // lines off-axis. Hairline regime (<1 phys px) is
        // the shader's trapezoid-plateau coverage.
        self.composer.polyline.points.clear();
        if let Some(spin) = p.bounds.spin() {
            // Spin: rotate each owner-local point about the pivot
            // before placing it via the ancestor transform, so the
            // shape rotates in place.
            let pivot = spin.pivot;
            let rotor = Vec2::from_angle(spin.angle);
            self.composer
                .polyline
                .points
                .extend(src_points.iter().map(|&q| {
                    let local = rotor.rotate(q - pivot) + pivot;
                    self.current_transform.apply_point(local + p.origin) * scale
                }));
        } else {
            self.composer.polyline.points.extend(
                src_points
                    .iter()
                    .map(|&q| self.current_transform.apply_point(q + p.origin) * scale),
            );
        }

        // Keep only points beyond the coincidence threshold
        // from their predecessor — degenerate segments
        // contribute no geometry and their colors drop
        // with them.
        self.composer.polyline.kept.clear();
        let mut prev: Option<Vec2> = None;
        for (i, &q) in self.composer.polyline.points.iter().enumerate() {
            if prev.is_none_or(|p| (q - p).length_squared() > POLYLINE_COINCIDENT_EPS_SQ) {
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
        if !self
            .composer
            .enter_higher_kind(PaintTier::Curve, bbox_urect, self.out)
        {
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
                kind: polyline_join_kind(d_a, d_b, join),
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
        let bounds = match self.composer.clip_scissor() {
            Some(scissor) => unclipped.clamp_to(scissor),
            None => unclipped,
        };
        if bounds.is_paint_empty() {
            return;
        }
        // Text sits below mesh/image/curve/polyline in the
        // kind order — flush if any prior higher-kind draw in
        // the group overlaps so this text doesn't get
        // reordered above it. (No need to check quads: text
        // paints over quads anyway.)
        if self.composer.any_higher_kind_overlap(bounds) {
            self.composer.flush(self.out);
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
            self.composer.close_batch(self.out);
        }
        // open_batch must run BEFORE the text push so the
        // batch's `texts_start` captures this run's index.
        let b = self.composer.open_batch(self.out);
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
            scale: snap_text_scale(self.current_transform.scale),
        });
        self.composer.batch.open_grid.push(bounds);
    }
}
