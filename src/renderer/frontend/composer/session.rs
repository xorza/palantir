//! One frame being composed, and the sink paint calls arrive through.

use crate::common::span::Span;
use crate::icons::icon_raster_key::IconRasterKey;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::geometry::urect::URect;
use crate::primitives::math::domain::{EPS, is_invisible};
use crate::primitives::math::num::{F32Px, Vec2Ext};
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::packed::half_simd::{self, F16x4};
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
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
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::curve::CurveInstance;
use crate::renderer::render_buffer::curve_caps::CurveCaps;
use crate::renderer::render_buffer::curve_kind::CurveKind;
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
use crate::shape::paint::curve_basis::CurveBasis;
use crate::shape::paint::lowered_shadow::ShadowGeom;
use crate::shape::record::ColorMode;
use crate::text::TEXT_SCALE_STEP;
use glam::{U16Vec2, UVec2, Vec2};

use crate::renderer::frontend::composer::clip_stack::ClipFrame;
use crate::renderer::frontend::composer::geometry;
use crate::renderer::frontend::composer::geometry::StrokeBbox;
use crate::renderer::frontend::composer::{Composer, GroupCursors, OpenBatch, PolylineScratch};

/// One compose pass in flight: the [`Composer`]'s scratch bound to the buffer
/// being filled, the record payloads, and the display. Paint arrives through
/// [`PaintSink`] in authoring order; the group and batch state machine lives here.
#[derive(Debug)]
pub(crate) struct ComposeSession<'a> {
    pub(super) composer: &'a mut Composer,
    pub(super) store: &'a RecordStore,
    pub(super) out: &'a mut RenderBuffer,
}

/// A quad-tier draw reduced to physical space. `corners` and `fill_axis` mean different things for
/// a rect and a triangle.
#[derive(Debug)]
struct PackedQuad {
    rect: ScaledRect,
    corners: Corners,
    fill_axis: FillAxis,
    stroke_width: f32,
}

impl PackedQuad {
    const fn is_sharp(&self) -> bool {
        is_invisible(self.stroke_width) && self.corners.is_approx_zero()
    }

    /// [`Self::is_sharp`] plus physical edges on whole pixels. Exactness makes the
    /// fragment fast path bitwise-identical to the SDF.
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

#[derive(Debug)]
struct ScaledRect {
    phys: Rect,
    urect: URect,
}

impl ScaledRect {
    fn from_phys(phys: Rect, viewport: UVec2) -> Self {
        Self {
            phys,
            urect: geometry::urect_from_phys(phys.min, phys.max(), viewport),
        }
    }
}

impl ComposeSession<'_> {
    /// Tile `t` in `[0, 1]` into `n` contiguous ranges, the last ending at exactly `1.0` so the
    /// shader's trailing-cap test fires.
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

    /// Apply the walk transform to a logical rect and scale it to physical px with integer bounds,
    /// once, for cull and instance alike.
    fn scaled_rect(&self, rect: Rect) -> ScaledRect {
        let world = self.composer.transform.apply_rect(rect);
        let phys = world.scaled_by(self.out.display.scale_factor(), self.out.display.pixel_snap);
        ScaledRect::from_phys(phys, self.out.display.physical)
    }

    /// The part of `whole` that can be seen (inside the surface and the active clip),
    /// or `None` where nothing was cut. `None` keeps the common unclipped view on its
    /// exact path, since an intersection's `(min + size) - min` is not exact in general.
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
    /// Close the trailing text batch and draw group. A destructor so a dropped session cannot leave
    /// a `RenderBuffer` that looks populated but is missing its last group.
    fn drop(&mut self) {
        self.close_batch();
        self.flush();
    }
}

impl PaintSink for ComposeSession<'_> {
    fn push_clip(&mut self, p: PushClipPayload) {
        let scale = self.out.display.scale_factor();
        let snap = self.out.display.pixel_snap;
        let viewport_phys = self.out.display.physical;
        let logical_radius = (!p.corners.is_approx_zero()).then_some(p.corners);
        let world = self.composer.transform.apply_rect(p.rect);
        let phys = world.scaled_by(scale, snap);
        let me = geometry::urect_from_phys(phys.min, phys.max(), viewport_phys);
        let parent = self.composer.clip.top();
        let scissor = match parent {
            Some(parent) => me.clamp_to(parent.scissor),
            None => me,
        };
        let parent_chain = parent.map_or(Span::default(), |f| f.chain);
        let chain = if let Some(logical_radius) = logical_radius {
            let scale_phys = geometry::phys_scale(self.composer.transform.current(), scale);
            // `mask_rect` stays unclamped: the SDF needs the true edges or corners shift inward
            // when the clip leaves the viewport.
            let rc = RoundedClip {
                mask_rect: phys,
                corners: logical_radius.fit_to(phys.size, scale_phys),
            };
            // A rounded push nested in rounded ancestors stacks: child chain = ancestor chain + own
            // mask. Re-pushing the innermost mask adds no depth.
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
            // A rect clip inside rounded ancestors inherits their chain; else the child group draws
            // with ref=0 and the stencil test discards every fragment.
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
        if self.fold_into_clear(&p, &packed) {
            return;
        }
        // Skip the quad when entirely outside the scissor. The clipped rect is also the
        // overlap test's rect, so an ancestor-clipped quad does not force a needless flush.
        let visible = self.composer.clip.clamped(packed.rect.urect);
        if visible.is_paint_empty() {
            return;
        }
        self.quad_forces_flush(visible);
        // Fragment fast path: a solid, sharp, stroke-less, pixel-aligned quad has
        // coverage exactly 1.0, so the shader returns the fill directly. `SOLID` excludes
        // shadows and triangles.
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
        let scale = self.out.display.scale_factor();
        let viewport_phys = self.out.display.physical;
        // `draw_mesh` already gated empty meshes, so `v_len >= 1`. Inflate by 0.5 phys px
        // like polyline's AA fringe: overlap-test false negatives reorder paint. Mesh
        // skips snapping, so it cannot use `scaled_rect`.
        let xform = self.composer.transform.current();
        let phys = geometry::phys_bbox(xform, p.bbox, p.origin, scale);
        let fringe = Vec2::splat(AA_HALF_WIDTH);
        let mesh_urect =
            geometry::urect_from_phys(phys.min - fringe, phys.max() + fringe, viewport_phys);
        // A surviving mesh closes the open text batch so its text emits before this above-text
        // geometry.
        if !self.admit_higher_kind(PaintTier::Mesh, mesh_urect) {
            return;
        }
        // Verts stay owner-local; the per-instance translate folds in owner origin and transform
        // stack, so the shader produces physical coords.
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
                tint: p.tint,
                ..bytemuck::Zeroable::zeroed()
            },
        });
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "every value is clamped to a non-negative range before the cast"
    )]
    fn icon(&mut self, p: DrawIconPayload) {
        let ScaledRect {
            phys: phys_rect,
            urect,
        } = self.scaled_rect(p.rect);
        if !self.admit_higher_kind(PaintTier::Icon, urect) {
            return;
        }
        let box_px = Vec2::new(phys_rect.size.w, phys_rect.size.h);
        let key = IconRasterKey::for_box(p.icon, box_px);
        let (origin, size) = if key.is_exact() {
            // Whole-pixel origin with the raster centred in its box; the `Nearest` atlas sampler
            // needs integer origins.
            let centred = phys_rect.min + (box_px - key.size().as_vec2()) * 0.5;
            (centred.fast_round().as_ivec2(), key.size())
        } else {
            // Above the exact band the raster is a nearby or capped rung; at its own size it
            // would stick out of or leave a gap in the box the cull and damage rects use. So
            // the quad is the box and the raster is resampled.
            let min = phys_rect.min.fast_round();
            let max = phys_rect.max().fast_round();
            let size = (max - min).max(Vec2::ONE);
            (min.as_ivec2(), U16Vec2::new(size.x as u16, size.y as u16))
        };
        self.out.icons.push(IconDrawRow {
            key,
            origin,
            size,
            color: p.tint,
            desaturate: p.desaturate,
        });
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "every value is clamped to a non-negative range before the cast"
    )]
    fn image(&mut self, draw: ImageDraw<'_>) {
        let ImageDraw { payload: p, view } = draw;
        let ScaledRect {
            phys: phys_rect,
            urect: image_urect,
        } = self.scaled_rect(p.rect);
        if !self.admit_higher_kind(PaintTier::Image, image_urect) {
            return;
        }
        // A `GpuView` draws over only its visible part, since that is all its target holds; its UV
        // stays whole.
        let seen = view.and_then(|_| self.seen(phys_rect));
        let composite = seen.unwrap_or(phys_rect);
        self.out.images.push(ImageDrawRow {
            // Only the registration id: the encoder already resolved fit into `rect` + UV. `target`
            // only schedules the off-screen paint.
            id: p.handle,
            instance: ImageInstance {
                rect: composite,
                uv_min: p.uv_min,
                uv_size: p.uv_size,
                tint: p.tint,
                flags: p.flags,
                ..bytemuck::Zeroable::zeroed()
            },
        });
        // A `GpuView` also needs its off-screen target painted. Sized to what is on
        // screen, not the rect: layout may return a rect larger than the surface, and
        // following it would allocate and draw discarded pixels.
        if let Some(view) = view {
            let scale = self.out.display.scale_factor();
            let cap = i64::from(self.composer.max_texture_dim.get());
            let whole = phys_rect.size;
            // The cap is measured against the whole view so the downsample does not change as the
            // view scrolls (it would shimmer).
            let downsample =
                (self.composer.max_texture_dim.get() as f32 / whole.w.max(whole.h)).min(1.0);
            let px = |v: f32| ((v * downsample).ceil() as i64).clamp(1, cap) as u32;
            // Floored at zero, unlike a size: a target needs a pixel, and a corner may be the
            // origin.
            let at = |v: f32| ((v * downsample).floor() as i64).clamp(0, cap) as u32;
            let (used, offset) = match seen {
                Some(seen) => (seen.size, seen.min - phys_rect.min),
                None => (whole, Vec2::ZERO),
            };
            // The window lands inside the view by arithmetic, not a clamp: origin rounds
            // down and size rounds up, so `offset + used <= full`. Pinned by
            // `compose_gpu_view_caps_wide_and_tall_targets_uniformly`.
            self.out.frame_targets.push(RenderTargetDraw {
                id: p.handle,
                used: UVec2::new(px(used.w), px(used.h)),
                full: UVec2::new(px(whole.w), px(whole.h)),
                offset: UVec2::new(at(offset.x), at(offset.y)),
                raster_scale: geometry::phys_scale(self.composer.transform.current(), scale)
                    * downsample,
                paint: view.paint.clone(),
                epoch: view.epoch,
            });
        }
    }

    fn curve(&mut self, p: DrawCurvePayload) {
        let scale = self.out.display.scale_factor();
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
        if !self.admit_higher_kind(PaintTier::Curve, bbox_urect) {
            return;
        }
        // Owner origin folds in here so the record stays owner-local. No snapping: it would warp
        // the traced shape.
        let to_phys = geometry::phys_point_map(xform, p.origin, scale);
        let spin = p.bounds.spin();
        let proto = CurveInstance {
            width: width_phys,
            color0: p.fill.color,
            color1: p.fill.color,
            caps: CurveCaps::new(cap, true, true),
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
                // Adaptive sub-instance count from the control-polygon length, which bounds arc
                // length from above (no faceting). Near-straight cubics use one instance: every
                // chord of a flat curve lies on the segment.
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
                    kind: CurveKind::CUBIC,
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
                // The transform is translate + uniform scale, so a circle stays a circle: transform
                // the centre, scale the radius.
                let radius_phys = radius * geometry::phys_scale(xform, scale);
                // Sub-instance count from the exact arc length; chord sagitta stays under 0.3 px
                // even at r = 1.
                let n = geometry::sub_instance_count(radius_phys * (a1 - a0).abs());
                let proto = CurveInstance {
                    p0: to_phys(center),
                    p1: Vec2::new(radius_phys, 0.0),
                    p2: Vec2::new(a0, a1),
                    p3: Vec2::ZERO,
                    kind: CurveKind::ARC,
                    ..proto
                };
                (proto, n)
            }
        };
        self.push_sub_instances(n, proto);
    }

    fn polyline(&mut self, p: DrawPolylinePayload) {
        let scale = self.out.display.scale_factor();
        let display = self.out.display;
        let mode = p.color_mode;
        let cap = p.cap;
        let join = p.join;
        let xform = self.composer.transform.current();
        let width_phys = p.width * geometry::phys_scale(xform, scale);

        // Inflated physical AABB, computed once for cull and overlap. Inflating by the
        // stroke fringe means the cull never trims reachable pixels. Clamped here because
        // `admit_higher_kind` wants the same clipped rect.
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

        // Transform points to physical px with owner origin folded in. No snapping: it would shift
        // thin lines off-axis.
        self.composer.polyline.points.clear();
        let to_phys = geometry::phys_point_map(xform, p.origin, scale);
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

        // Keep only points beyond the coincidence threshold from their predecessor; dropped points
        // take their colors with them.
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
        // Only now that the polyline will emit geometry: an empty or culled one must not split the
        // batch or group.
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
        // Segment colors for `k -> k+1`, indexed through `kept`. A paint animation's
        // alpha is folded in here, where each colour is read once, so fading costs no copy.
        let seg_colors = |k: usize| -> (RgbaF16, RgbaF16) {
            let (a, b) = match mode {
                ColorMode::Single => (src_colors[0], src_colors[0]),
                ColorMode::PerPoint => (
                    src_colors[kept[k] as usize],
                    src_colors[kept[k + 1] as usize],
                ),
                ColorMode::PerSegment => {
                    let c = src_colors[kept[k + 1] as usize - 1];
                    (c, c)
                }
            };
            if p.alpha == 1.0 {
                (a, b)
            } else {
                (a.faded(p.alpha), b.faded(p.alpha))
            }
        };
        let n_segs = directions.len();
        for k in 0..n_segs {
            // Pre-oriented bisector clip planes for the joint ends; zero = cap end, no clip.
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
                caps: CurveCaps::new(cap, k == 0, k + 1 == n_segs),
                kind: CurveKind::SEGMENT,
                ..bytemuck::Zeroable::zeroed()
            });
        }
        // One chrome instance per interior joint fills the wedge between segment end
        // faces, in the premultiplied linear average of the adjacent colors.
        for k in 1..n_segs {
            let d_a = directions[k - 1];
            let d_b = directions[k];
            let (_, ca) = seg_colors(k - 1);
            let (cb, _) = seg_colors(k);
            let color = if ca == cb {
                ca
            } else {
                RgbaF16::from(premultiplied_midpoint(ca.into(), cb.into()))
            };
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
        let world = self.composer.transform.apply_rect(t.rect);
        let scale = self.out.display.scale_factor();
        let phys_rect = world.scaled_by(scale, self.out.display.pixel_snap);
        // What the glyphs can reach: ink from the placed origin at true size, padded by
        // the scale-step fraction like the run's damage rect (`inflate_text_damage`).
        // Covered, never rounded in, so the last AA column is not cut.
        let unclipped = {
            let inked = self
                .composer
                .transform
                .apply_rect(t.rect.inflated_by(t.ink));
            let lead = (world.min - inked.min) * scale;
            let size = Vec2::new(inked.size.w, inked.size.h) * scale;
            let pad = size * (TEXT_SCALE_STEP * 0.5);
            let min = phys_rect.min - lead;
            geometry::urect_from_phys(min - pad, min + size + pad, self.out.display.physical)
        };
        // `bounds` feeds the batch GPU scissor and the backend's per-line y-cull; there is no
        // per-glyph clip. An empty intersection with the clip skips the push.
        let bounds = self.composer.clip.clamped(unclipped);
        if bounds.is_paint_empty() {
            return;
        }
        // Text sits below mesh/image/curve/polyline in kind order: flush if a prior higher-kind
        // draw overlaps. Quads need no check.
        if self.composer.higher_kinds.any_overlap(bounds) {
            self.flush();
        }
        // Batch scissor = `open_grid.union`. The text shader has no per-instance clip, so
        // a strict run (cut by an ancestor clip) batches only with peers of identical
        // `bounds`; non-strict runs coalesce freely.
        let new_strict = bounds != unclipped;
        if let Some(b) = self.composer.batch.open.as_ref()
            && (b.strict || new_strict)
            && self.composer.batch.open_grid.union != bounds
        {
            self.close_batch();
        }
        // `open_batch` must run before the text push so `texts_start` captures this run's index.
        let b = self.open_batch();
        b.strict |= new_strict;
        self.out.texts.push(TextDrawRow {
            origin: phys_rect.min,
            bounds,
            color: t.color,
            text: t.text,
            // Snap the ancestor-transform part of the text scale to 0.5% steps; continuous zoom
            // would mint a new glyph cache key every frame.
            scale: geometry::snap_text_scale(self.composer.transform.scale()),
        });
        self.composer.batch.open_grid.push(bounds);
    }
}

impl ComposeSession<'_> {
    /// Reduce a quad-tier draw to physical space; everything past this is shape-blind.
    fn pack_quad(&self, p: &DrawQuadPayload) -> PackedQuad {
        let xform = self.composer.transform.current();
        let scale_phys = geometry::phys_scale(xform, self.out.display.scale_factor());
        match p.geom {
            QuadGeom::Rect { rect, corners } => {
                let source = self.scaled_rect(rect);
                // Shadow scalars are logical px, so scale them; a gradient axis is unit-space.
                let fill_axis = if p.fill.kind.is_shadow() {
                    p.fill_axis.scaled(scale_phys)
                } else {
                    p.fill_axis
                };
                let corners = corners.fit_to(source.phys.size, scale_phys);
                // A drop shadow's quad is its snapped source moved and grown, not re-snapped, so
                // the source inside it lies on the same pixels as the fill.
                let rect = if p.fill.kind == FillKind::SHADOW_DROP {
                    let geom = ShadowGeom::from_lanes(fill_axis.lanes());
                    let phys = Rect {
                        min: source.phys.min + geom.offset,
                        size: source.phys.size,
                    }
                    .inflated(geom.halo());
                    ScaledRect::from_phys(phys, self.out.display.physical)
                } else {
                    source
                };
                PackedQuad {
                    rect,
                    corners,
                    fill_axis,
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
                let scale = self.out.display.scale_factor();
                // No snapping: the SDF handles sub-pixel placement.
                let xf = geometry::phys_point_map(xform, origin, scale);
                let (a, b, c) = (xf(a), xf(b), xf(c));
                let radius_phys = (radius * scale_phys).max(0.0);
                // Covering AABB of the rounded shape; the stroke sits on the inner edge, so it adds
                // no outward reach.
                let lo = a.min(b).min(c);
                let hi = a.max(b).max(c);
                let phys_rect = Rect::from_min_max(lo, hi).inflated(radius_phys);
                // Pack the points as unorm16 shares of the covering rect and the radius as f16;
                // `FillKind::TRIANGLE` tells the shader to decode them.
                let share = |p: Vec2| {
                    let at = (p - phys_rect.min) / Vec2::new(phys_rect.size.w, phys_rect.size.h);
                    [unorm16(at.x), unorm16(at.y)]
                };
                let ([ax, ay], [bx, by], [cx, cy]) = (share(a), share(b), share(c));
                let [_, _, radius_f16, _] =
                    half_simd::f16x4_from_f32x4([0.0, 0.0, radius_phys, 0.0]);
                PackedQuad {
                    rect: ScaledRect::from_phys(phys_rect, self.out.display.physical),
                    corners: Corners::from_bits([ax, ay, bx, by]),
                    fill_axis: FillAxis::from(F16x4::from_bits([cx, cy, radius_f16, 0])),
                    stroke_width: (p.stroke.width * scale_phys).max(0.0),
                }
            }
        }
    }

    /// Clear fold: an opaque, solid, sharp, unclipped quad covering the viewport is
    /// bit-identical to `LoadOp::Clear(fill)` and hides everything before it, so
    /// discard the scene so far and record the fill as the pass clear. The clip must be
    /// empty: a scissored cover hides only its scissor, and an empty clip guarantees no
    /// group in flight references the `rounded_clips` state the discard wipes.
    ///
    /// Returns `true` when folded; the quad must not be emitted.
    fn fold_into_clear(&mut self, p: &DrawQuadPayload, packed: &PackedQuad) -> bool {
        let phys = packed.rect.phys;
        let covers_viewport = phys.min.x <= EPS
            && phys.min.y <= EPS
            && phys.max().x >= self.out.display.physical.as_vec2().x - EPS
            && phys.max().y >= self.out.display.physical.as_vec2().y - EPS;
        if !covers_viewport
            || self.composer.clip.top().is_some()
            || p.fill.kind != FillKind::SOLID
            || !p.fill.color.is_opaque()
            || !packed.is_sharp()
        {
            return false;
        }
        self.discard_composed();
        self.out.clear_override = Some(p.fill.color);
        true
    }

    /// Opaque-cover annotation for the occlusion pruner, `SOLID` only: a shadow's blur and a
    /// triangle's empty corners don't cover the whole `rect`.
    fn record_opaque_cover(&mut self, p: &DrawQuadPayload, packed: &PackedQuad, fast: bool) {
        if p.fill.kind != FillKind::SOLID || !p.fill.color.is_opaque() {
            return;
        }
        let inscribed = packed.rect.phys.inscribed_for_corners(packed.corners);
        let stroke_inset = if is_invisible(packed.stroke_width) || p.stroke.color.is_opaque() {
            0.0
        } else {
            packed.stroke_width
        };
        let aa_inset = if fast { 0.0 } else { AA_HALF_WIDTH };
        let cover = inscribed.deflated_by(Spacing::all(stroke_inset + aa_inset));
        if !cover.is_paint_empty() {
            let idx = self.out.quads.len() as u32 - 1 - self.composer.cursors.quads;
            self.composer.occlusion.record_opaque(idx, cover);
        }
    }

    /// Close the in-flight group: push a `DrawGroup` if anything was emitted, advance
    /// the per-kind cursors and clear the overlap scratches. Scissor and rounded clip
    /// carry over.
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
            // Push higher-kind batches before the group so their `last_group` matches its eventual
            // index.
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
        // Closed-batch text is group-scoped: past a group boundary those batches have rendered and
        // no longer gate quads. The open-batch grid spans groups.
        composer.batch.closed_grid.clear();
        composer.batch.pending_batch_cursor = self.out.text_batches.len();
    }

    /// Finalize the open text batch, if any. Called at batch-split events (rounded-clip
    /// change, higher-kind append, strict-bounds mismatch). The grid fill is deferred to
    /// [`Self::closed_hit`].
    fn close_batch(&mut self) {
        let Some(b) = self.composer.batch.open.take() else {
            return;
        };
        let texts_end = self.out.texts.len() as u32;
        let scissor = self.composer.batch.open_grid.union;
        self.composer.batch.open_grid.clear();
        // Schedule-cursor invariants: `last_group` is non-decreasing in walk order, and `texts`
        // spans concatenate without gaps.
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
            // Already physical and clamped to every run's clip-narrowed bounds, so it is the
            // batch's GPU scissor (the text backend has no per-run clipping).
            scissor,
            // Every close site runs while the outgoing clip is still the stack top.
            rounded_clips: self.composer.clip.chain(),
        });
    }

    /// Return the open batch, opening one if none exists; refreshes `last_group` to the in-flight
    /// group's eventual index.
    fn open_batch(&mut self) -> &mut OpenBatch {
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

    /// Cull a higher-kind (mesh / image / curve) draw against the active clip, then
    /// close the open text batch if this draw covers text already in it. A culled draw
    /// must neither split the batch nor register a rect.
    ///
    /// The close is conditional: a batch renders at the end of its last group, so text
    /// recorded before this draw can paint over it, but only where rects meet. Closing
    /// on every draw would cost one text batch per draw (e.g. per toolbar button). The
    /// other half is in [`Self::text`]: a run recorded after this draw flushes the group
    /// when it overlaps `higher_kinds`. Neither test is sound alone.
    ///
    /// Also flushes on a cross-kind conflict with an earlier higher-kind draw (see
    /// [`HigherKindRects::conflicts`]), then records this rect. Returns `false` when
    /// culled. Polyline calls it only once its kept-point walk proves it emits geometry.
    ///
    /// [`HigherKindRects::conflicts`]:
    /// crate::renderer::frontend::composer::higher_kind::HigherKindRects::conflicts
    fn admit_higher_kind(&mut self, tier: PaintTier, bounds: URect) -> bool {
        // Clipped first, so the registered rect is what is painted.
        let bounds = self.composer.clip.clamped(bounds);
        if bounds.is_paint_empty() {
            return false;
        }
        if self.composer.batch.open_grid.any_overlap(bounds) {
            self.close_batch();
        }
        if self.composer.higher_kinds.conflicts(tier, bounds) {
            self.flush();
        }
        self.composer.higher_kinds.push(tier, bounds);
        true
    }

    /// Flush if a quad-tier draw at `overlap` overlaps something in the group that
    /// would be reordered above it (quad is the lowest kind). Text is checked against
    /// the open batch's grid and batches closed in this group ([`Self::closed_hit`]); an
    /// open-batch hit also closes the batch so its text cannot coalesce forward and
    /// re-cover the quad.
    fn quad_forces_flush(&mut self, overlap: URect) {
        if self.composer.batch.open_grid.any_overlap(overlap) {
            self.close_batch();
            self.flush();
        } else if self.closed_hit(overlap) || self.composer.higher_kinds.any_overlap(overlap) {
            self.flush();
        }
    }

    /// `true` if `q` overlaps text of a batch closed in the in-flight group. The first
    /// query hitting a pending batch scissor drains all pending batches into the closed
    /// grid; groups nothing probes never pay the fill.
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

    /// Push `frame` as the clip in force, closing the batch and group first if it
    /// differs. The break runs before the stack moves, since [`Self::flush`] stamps the
    /// closing group with the stack top. Named apart from the [`PaintSink`] pair so a
    /// rename cannot turn `pop_clip` into silent recursion.
    fn enter_clip(&mut self, frame: ClipFrame) {
        self.break_for_clip(Some(frame));
        self.composer.clip.push(frame);
    }

    /// Restore the parent clip; see [`Self::enter_clip`].
    fn leave_clip(&mut self) {
        let parent = self.composer.clip.parent();
        self.break_for_clip(parent);
        self.composer.clip.pop();
    }

    /// Close what the clip in force owns, if `next` differs. Chains compare by value, so a
    /// redundant push/pop is a no-op.
    fn break_for_clip(&mut self, next: Option<ClipFrame>) {
        let next_chain = next.map_or(Span::default(), |frame| frame.chain);
        let chain_changed = !self
            .out
            .chains_equal(next_chain, self.composer.clip.chain());
        if chain_changed {
            // The stencil mask stack follows the active chain; close before the group transition,
            // while the stack top still names the batch's chain.
            self.close_batch();
        }
        if next.map(|frame| frame.scissor) != self.composer.clip.scissor() || chain_changed {
            self.flush();
        }
    }

    /// Clear-fold discard: drop the scene output and its scratch. Walk state survives: the clip
    /// stack is empty by precondition and transform pops may still be ahead.
    fn discard_composed(&mut self) {
        self.out.discard_scene();
        self.composer.reset_group_scratch(self.out.display.physical);
    }
}

/// The straight colour halfway between `a` and `b`, interpolated premultiplied. A midpoint with no
/// alpha is transparent black.
fn premultiplied_midpoint(a: RgbaF32, b: RgbaF32) -> RgbaF32 {
    let (a, b) = (a.premultiplied(), b.premultiplied());
    let alpha = f32::midpoint(a.a, b.a);
    if alpha <= 0.0 {
        return RgbaF32::TRANSPARENT;
    }
    let channel = |x: f32, y: f32| f32::midpoint(x, y) / alpha;
    RgbaF32 {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: alpha,
    }
}

/// `v` in `0..=1` as unorm16, matching `unpack2x16unorm`: `bits / 65535`.
#[expect(
    clippy::cast_sign_loss,
    reason = "every value is clamped to a non-negative range before the cast"
)]
const fn unorm16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}
