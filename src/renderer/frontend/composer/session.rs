//! One frame being composed, and the sink paint calls arrive through.

use crate::common::span::Span;
use crate::icons::icon_raster_key::IconRasterKey;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::geometry::urect::URect;
use crate::primitives::math::domain::{EPS, is_invisible};
use crate::primitives::math::num::Vec2Ext;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::primitives::paint::backdrop_filter::Lens;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::composer::clip_stack::ClipFrame;
use crate::renderer::frontend::composer::curve_run::CurveRun;
use crate::renderer::frontend::composer::geometry;
use crate::renderer::frontend::composer::lowering::{Lowering, ScaledRect};
use crate::renderer::frontend::composer::packed_quad::PackedQuad;
use crate::renderer::frontend::composer::paint_order::{Lane, Ordering};
use crate::renderer::frontend::composer::polyline_scratch::PolylineLook;
use crate::renderer::frontend::composer::stroke_bbox::StrokeBbox;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::ImageDraw;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_pane_payload::DrawPanePayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::pane_captures::PaneTicket;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::backdrop_row::{BackdropRow, PaneSource};
use crate::renderer::render_buffer::icon_draw_row::IconDrawRow;
use crate::renderer::render_buffer::image_draw_row::{ImageDrawRow, ImageInstance};
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::renderer::render_buffer::mesh_draw_row::{MeshDraw, MeshDrawRow, MeshInstance};
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::render_target_draw::RenderTargetDraw;
use crate::renderer::render_buffer::rounded_clip::RoundedClip;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use crate::renderer::render_buffer::{MAX_ROUNDED_CLIP_DEPTH, RenderBuffer};
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;
use crate::shape::record_store::RecordStore;
use crate::shape::stroke_bounds::StrokeJoints;
use crate::text;
use glam::{UVec2, Vec2};
use std::num::NonZeroI64;

/// One compose pass in flight: the [`Composer`]'s scratch bound to the buffer
/// being filled, the record payloads, and the display. Paint arrives through
/// [`PaintSink`] in authoring order: each draw is lowered here (`lowering`, `packed_quad`,
/// `curve_run`, `polyline_scratch`) and ordered through [`Ordering`].
#[derive(Debug)]
pub(crate) struct ComposeSession<'a> {
    pub(super) composer: &'a mut Composer,
    pub(super) store: &'a RecordStore,
    pub(super) out: &'a mut RenderBuffer,
}

impl ComposeSession<'_> {
    /// The walk transform and the display this session lowers draws under.
    const fn lowering(&self) -> Lowering<'_> {
        Lowering {
            transform: &self.composer.transform,
            display: &self.out.display,
        }
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

    /// Paint order over this session's buffer, under the clip in force.
    const fn ordering(&mut self) -> Ordering<'_> {
        Ordering {
            order: &mut self.composer.order,
            clip: &self.composer.clip,
            out: &mut *self.out,
        }
    }
}

impl Drop for ComposeSession<'_> {
    /// Close the trailing text batch and draw group. A destructor so a dropped session cannot leave
    /// a `RenderBuffer` that looks populated but is missing its last group.
    fn drop(&mut self) {
        let mut ordering = self.ordering();
        ordering.close_batch();
        ordering.flush();
    }
}

impl PaintSink for ComposeSession<'_> {
    fn push_clip(&mut self, p: PushClipPayload) {
        let logical_radius = (!p.corners.is_approx_zero()).then_some(p.corners);
        let ScaledRect {
            physical,
            urect: me,
        } = self.lowering().scaled_rect(p.rect);
        let parent = self.composer.clip.top();
        let scissor = match parent {
            Some(parent) => me.clamp_to(parent.scissor),
            None => me,
        };
        let parent_chain = parent.map_or_else(Span::default, |f| f.chain);
        let chain = if let Some(logical_radius) = logical_radius {
            let scale_physical = self.composer.transform.physical_scale();
            // `mask_rect` stays unclamped: the SDF needs the true edges or corners shift inward
            // when the clip leaves the viewport.
            let rc = RoundedClip {
                mask_rect: physical,
                corners: logical_radius.fit_to(physical.size, scale_physical),
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
        debug_assert!(
            !p.fill.kind.is_backdrop(),
            "a pane arrives through `pane`, with its ticket"
        );
        let packed = PackedQuad::pack(&p, &self.lowering());
        if self.fold_into_clear(&p, &packed) {
            return;
        }
        // The clipped rect is also the overlap test's rect, so an ancestor-clipped quad does not
        // force a needless flush.
        if self
            .ordering()
            .admit(Lane::Quad, packed.rect.urect)
            .is_none()
        {
            return;
        }
        self.push_quad(&p, &packed, p.fill.lut_row);
        self.record_opaque_cover(&p, &packed);
    }

    /// A pane lowers to its quad once, which neither folds into the clear nor covers anything
    /// opaquely: the backdrop shows through it.
    fn pane(&mut self, p: DrawPanePayload) {
        let quad = p.quad();
        let packed = PackedQuad::pack(&quad, &self.lowering());
        let Some(visible) = self.ordering().cull(packed.rect.urect) else {
            return;
        };
        // Listed before it is ordered, so its own paint is not among what it shares a break past.
        let row = self.list_pane(visible, p.ticket, p.backdrop);
        self.ordering().order(Lane::Quad, visible);
        self.push_quad(&quad, &packed, row);
    }

    fn mesh(&mut self, p: DrawMeshPayload) {
        let viewport_physical = self.out.display.physical;
        // `draw_mesh` already gated empty meshes, so `vertices` is non-empty. Inflate by 0.5 phys px
        // like polyline's AA fringe: overlap-test false negatives reorder paint. Mesh
        // skips snapping, so it cannot use `scaled_rect`.
        let to_physical = self.composer.transform.physical_map(p.origin);
        let bbox = p.spin.map_or(p.bbox, |spin| p.bbox.spun_cover(spin.pivot));
        let physical = to_physical.bbox(bbox);
        let fringe = Vec2::splat(AA_HALF_WIDTH);
        let mesh_urect = geometry::urect_from_physical(
            physical.min - fringe,
            physical.max() + fringe,
            viewport_physical,
        );
        // A surviving mesh closes the open text batch so its text emits before this above-text
        // geometry.
        if self
            .ordering()
            .admit(Lane::Tier(PaintTier::Mesh), mesh_urect)
            .is_none()
        {
            return;
        }
        // Verts stay owner-local; the per-instance translate folds in owner origin and transform
        // stack, so the shader produces physical coords.
        // A turn about the pivot `c` maps `pos` to `R(pos - c) + c`, so the instance takes
        // `s·R` as its complex scale and `s·(c - R c)` into its translate.
        let scale_physical = self.composer.transform.physical_scale();
        let (rotor_scale, translate) = match p.spin {
            None => (
                Vec2::new(scale_physical, 0.0),
                to_physical.point(Vec2::ZERO),
            ),
            Some(spin) => {
                let rotor = Vec2::from_angle(spin.angle);
                (
                    rotor * scale_physical,
                    to_physical.point(spin.pivot - rotor.rotate(spin.pivot)),
                )
            }
        };
        self.out.meshes.push(MeshDrawRow {
            draw: MeshDraw {
                vertices: p.vertices,
                idxs: p.idxs,
            },
            instance: MeshInstance {
                translate,
                rotor_scale,
                tint: p.tint,
                ..bytemuck::Zeroable::zeroed()
            },
        });
    }

    fn icon(&mut self, p: DrawIconPayload) {
        let ScaledRect {
            physical: physical_rect,
            urect,
        } = self.lowering().scaled_rect(p.rect);
        // A turned icon is placed as an unturned one and turned whole about the pivot on the GPU,
        // so only what it reaches changes.
        let spin = p.spin.map(|spin| self.lowering().physical_spin(spin));
        let urect = match spin {
            None => urect,
            Some(spin) => {
                let reach = physical_rect.turned_cover(spin.pivot, spin.angle);
                geometry::urect_from_physical(reach.min, reach.max(), self.out.display.physical)
            }
        };
        if self
            .ordering()
            .admit(Lane::Tier(PaintTier::Icon), urect)
            .is_none()
        {
            return;
        }
        let box_px = Vec2::new(physical_rect.size.w, physical_rect.size.h);
        let key = IconRasterKey::for_box(p.icon, box_px);
        let (origin, size) = if key.is_exact() {
            // Whole-pixel origin with the raster centered in its box; the `Nearest` atlas sampler
            // needs integer origins.
            let centered = physical_rect.min + (box_px - key.size().as_vec2()) * 0.5;
            (centered.fast_round().as_ivec2(), key.size().as_uvec2())
        } else {
            // Above the exact band the raster is a nearby or capped rung; at its own size it
            // would stick out of or leave a gap in the box the cull and damage rects use. So
            // the quad is the box and the raster is resampled.
            let min = physical_rect.min.fast_round();
            let max = physical_rect.max().fast_round();
            let size = (max - min).max(Vec2::ONE);
            (min.as_ivec2(), size.as_uvec2())
        };
        self.out.icons.push(IconDrawRow {
            key,
            origin,
            size,
            color: p.tint,
            desaturate: p.desaturate,
            spin,
        });
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "every value is clamped to a non-negative range before the cast"
    )]
    fn image(&mut self, draw: ImageDraw<'_>) {
        let ImageDraw { payload: p, view } = draw;
        // A turned image snaps to no pixel, and moves its center about the pivot as a quad does.
        let ScaledRect {
            physical: physical_rect,
            urect: image_urect,
        } = match p.spin {
            None => self.lowering().scaled_rect(p.rect),
            Some(spin) => {
                let exact = self
                    .lowering()
                    .scaled_rect_with(p.rect, EdgeRounding::Exact)
                    .physical;
                self.lowering().turned_rect(exact, spin)
            }
        };
        if self
            .ordering()
            .admit(Lane::Tier(PaintTier::Image), image_urect)
            .is_none()
        {
            return;
        }
        // A `GpuView` draws over only its visible part, since that is all its target holds; a
        // turned one draws whole, as its visible part is not a rect of its own.
        let seen = view
            .filter(|_| p.spin.is_none())
            .and_then(|_| self.seen(physical_rect));
        let composite = seen.unwrap_or(physical_rect);
        let mut instance = ImageInstance {
            rect: composite,
            uv_min: p.uv_min,
            uv_size: p.uv_size,
            tint: p.tint,
            flags: p.flags,
            rotation: p.spin.map_or(0.0, |spin| spin.angle),
            ..bytemuck::Zeroable::zeroed()
        };
        // A `GpuView` also needs its off-screen target painted. Sized to what is on
        // screen, not the rect: layout may return a rect larger than the surface, and
        // following it would allocate and draw discarded pixels.
        if let Some(view) = view {
            let cap = NonZeroI64::from(self.composer.max_texture_dim).get();
            let whole = physical_rect.size;
            // The cap is measured against the whole view so the downsample does not change as the
            // view scrolls (it would shimmer).
            let downsample =
                (self.composer.max_texture_dim.get() as f32 / whole.w.max(whole.h)).min(1.0);
            let px = |v: f32| ((v * downsample).ceil() as i64).clamp(1, cap) as u32;
            // Floored at zero, unlike a size: a target needs a pixel, and a corner may be the
            // origin.
            let at = |v: f32| ((v * downsample).floor() as i64).clamp(0, cap) as u32;
            let (used, offset) = match seen {
                Some(seen) => (seen.size, seen.min - physical_rect.min),
                None => (whole, Vec2::ZERO),
            };
            // The window lands inside the view by arithmetic, not a clamp: origin rounds
            // down and size rounds up, so `offset + used <= full`. Pinned by
            // `compose_gpu_view_caps_wide_and_tall_targets_uniformly`.
            let target = RenderTargetDraw {
                id: p.handle,
                used: UVec2::new(px(used.w), px(used.h)),
                full: UVec2::new(px(whole.w), px(whole.h)),
                offset: UVec2::new(at(offset.x), at(offset.y)),
                raster_factor: self.composer.transform.physical_scale() * downsample,
                paint: view.paint.clone(),
                epoch: view.epoch,
            };
            // The target holds `used` in the top-left of its capacity; the rest is slack.
            let crop = target.used.as_vec2() / target.capacity().as_vec2();
            instance.uv_min *= crop;
            instance.uv_size *= crop;
            instance.flags = instance.flags.union(ImageFlags::CLAMP_TO_UV);
            self.out.frame_targets.push(target);
        }
        self.out.images.push(ImageDrawRow {
            // Only the registration id: the encoder already resolved fit into `rect` + UV. The
            // view's off-screen paint is scheduled above, in `frame_targets`.
            id: p.handle,
            instance,
        });
    }

    fn curve(&mut self, p: DrawCurvePayload) {
        let transform = self.composer.transform.cur();
        let width_physical = p.width * self.composer.transform.physical_scale();
        let cap = p.cap;
        let bbox_urect = StrokeBbox {
            transform,
            bbox: p.bounds.cull_rect(),
            origin: p.origin,
            width_physical,
            cap,
            joints: StrokeJoints::Unjoined,
            display: self.out.display,
        }
        .urect();
        if self
            .ordering()
            .admit(Lane::Tier(PaintTier::Curve), bbox_urect)
            .is_none()
        {
            return;
        }
        let run = CurveRun::lower(&p, width_physical, &self.lowering());
        run.push_into(&mut self.out.curves);
    }

    fn polyline(&mut self, p: DrawPolylinePayload) {
        let display = self.out.display;
        let mode = p.color_mode;
        let cap = p.cap;
        let join = p.join;
        let transform = self.composer.transform.cur();
        let width_physical = p.width * self.composer.transform.physical_scale();

        // Inflated physical AABB, culled once and ordered once it proves to emit. Inflating by
        // the stroke fringe means the cull never trims reachable pixels.
        let bbox = StrokeBbox {
            transform,
            bbox: p.bounds.cull_rect(),
            origin: p.origin,
            width_physical,
            cap,
            joints: StrokeJoints::polyline(p.points.len, join),
            display,
        }
        .urect();
        let Some(visible) = self.ordering().cull(bbox) else {
            return;
        };

        let src_points = &self.store.polyline_points[p.points.range()];
        let src_colors = &self.store.polyline_colors[p.colors.range()];

        // Physical px with the owner origin folded in. No snapping: it would shift thin lines
        // off-axis.
        let to_physical = self.composer.transform.physical_map(p.origin);
        let rotor = p.bounds.spin().map(Spin::rotor);
        let kept = self.composer.polyline.keep(
            src_points
                .iter()
                .map(|&q| to_physical.point(rotor.map_or(q, |rotor| rotor.apply(q)))),
        );
        if !kept {
            return;
        }
        // Only now that the polyline will emit geometry: an empty or culled one must not split the
        // batch or group.
        self.ordering().order(Lane::Tier(PaintTier::Curve), visible);
        self.composer.polyline.emit(
            PolylineLook {
                colors: src_colors,
                mode,
                alpha: p.alpha,
                width: width_physical,
                cap,
                join,
            },
            &mut self.out.curves,
        );
    }

    fn text(&mut self, t: DrawTextPayload) {
        let spin = t.spin.map(|spin| self.lowering().physical_spin(spin));
        let world = self.composer.transform.apply_rect(t.rect);
        let scale = self.out.display.scale_factor();
        let physical_rect = world.scaled_by(scale, self.out.display.edge_rounding());
        // What the glyphs can reach: ink from the placed origin at true size, padded by
        // the scale-step fraction like the run's damage rect (`inflate_text_damage`).
        // Covered, never rounded in, so the last AA column is not cut.
        let unclipped = {
            let local_inked = t.rect.inflated_by(t.ink);
            let inked = self.composer.transform.apply_rect(local_inked);
            let lead = (world.min - inked.min) * scale;
            let size = Vec2::new(inked.size.w, inked.size.h) * scale;
            let slack = text::snap_slack(local_inked.size);
            let pad = Vec2::new(slack.w, slack.h) * scale;
            let min = physical_rect.min - lead;
            let mut reach = Rect::from_min_max(min - pad, min + size + pad);
            // A turned run is placed as an unturned one and turned whole about the pivot on the
            // GPU, so only what it reaches changes.
            if let Some(spin) = spin {
                reach = reach.turned_cover(spin.pivot, spin.angle);
            }
            geometry::urect_from_physical(reach.min, reach.max(), self.out.display.physical)
        };
        // `bounds` feeds the batch GPU scissor and the backend's per-line y-cull; there is no
        // per-glyph clip.
        let Some(bounds) = self.ordering().admit(Lane::Text, unclipped) else {
            return;
        };
        let row = TextDrawRow {
            origin: physical_rect.min,
            bounds,
            color: t.color,
            text: t.text,
            // Snap the ancestor-transform part of the text scale to 0.5% steps; continuous zoom
            // would mint a new glyph cache key every frame.
            scale: geometry::snap_text_scale(self.composer.transform.scale()),
            spin,
        };
        self.ordering().push_text(row, bounds != unclipped);
    }
}

impl ComposeSession<'_> {
    /// Push `p`, packed as `packed`, with `lut_row` its gradient row or its pane's row.
    fn push_quad(&mut self, p: &DrawQuadPayload, packed: &PackedQuad, lut_row: LutRow) {
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
            rect: packed.rect.physical,
            fill: p.fill.color,
            corners: packed.corners,
            stroke_color: p.stroke.color,
            stroke_width: packed.stroke_width,
            fill_kind,
            fill_lut_row: lut_row,
            fill_axis: packed.fill_axis,
            rotation: packed.rotation,
        });
    }

    /// List the pane of `ticket` and `backdrop`, seen at `visible`, at the source paint order
    /// gives it ([`Ordering::pane_source`]); its row is the quad's `fill_lut_row`.
    fn list_pane(
        &mut self,
        visible: URect,
        ticket: PaneTicket,
        backdrop: LoweredBackdrop,
    ) -> LutRow {
        debug_assert!(
            ticket.footprint.union(visible) == ticket.footprint,
            "a pane's footprint holds what it shows"
        );
        let source = self.ordering().pane_source(ticket);
        let scale = self.composer.transform.physical_scale();
        let row = LutRow(self.out.backdrops.len() as u32);
        self.out.backdrops.push(BackdropRow {
            key: ticket.key,
            footprint: ticket.footprint,
            plan: ticket.plan,
            region: ticket.region,
            lens: backdrop.lens().map(|lens| Lens {
                bezel: lens.bezel * scale,
                thickness: lens.thickness * scale,
                ..lens
            }),
            max_luminance: backdrop.max_luminance(),
            source,
        });
        row
    }

    /// Clear fold: an opaque, solid, sharp, unclipped quad covering the viewport is
    /// bit-identical to `LoadOp::Clear(fill)` and hides everything before it, so
    /// discard the scene so far and record the fill as the pass clear. The clip must be
    /// empty: a scissored cover hides only its scissor, and an empty clip guarantees no
    /// group in flight references the `rounded_clips` state the discard wipes.
    ///
    /// Returns `true` when folded; the quad must not be emitted.
    fn fold_into_clear(&mut self, p: &DrawQuadPayload, packed: &PackedQuad) -> bool {
        let physical = packed.rect.physical;
        let covers_viewport = physical.min.x <= EPS
            && physical.min.y <= EPS
            && physical.max().x >= self.out.display.physical.as_vec2().x - EPS
            && physical.max().y >= self.out.display.physical.as_vec2().y - EPS;
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

    /// Opaque-cover annotation, for the occlusion pruner, of the quad just pushed. `SOLID` only: a
    /// shadow's blur and a triangle's empty corners don't cover the whole `rect`, and a turned
    /// quad covers no axis-aligned rect of its own. A quad on the fast path has no anti-aliased
    /// edge to inset by.
    fn record_opaque_cover(&mut self, p: &DrawQuadPayload, packed: &PackedQuad) {
        if p.fill.kind != FillKind::SOLID || !p.fill.color.is_opaque() || packed.rotation != 0.0 {
            return;
        }
        let quad = self.out.quads.last().expect("the quad just pushed");
        let inscribed = packed.rect.physical.inscribed_for_corners(packed.corners);
        let stroke_inset = if is_invisible(packed.stroke_width) || p.stroke.color.is_opaque() {
            0.0
        } else {
            packed.stroke_width
        };
        let aa_inset = if quad.fill_kind.is_fast() {
            0.0
        } else {
            AA_HALF_WIDTH
        };
        let cover = inscribed.deflated_by(Spacing::saturating([stroke_inset + aa_inset; 4]));
        if !cover.is_paint_empty() {
            self.ordering().record_opaque(cover);
        }
    }

    /// Push `frame` as the clip in force, closing the batch and group first if it
    /// differs ([`Ordering::break_for_clip`]). Named apart from the [`PaintSink`] pair so a
    /// rename cannot turn `pop_clip` into silent recursion.
    fn enter_clip(&mut self, frame: ClipFrame) {
        self.ordering().break_for_clip(Some(frame));
        self.composer.clip.push(frame);
    }

    /// Restore the parent clip; see [`Self::enter_clip`].
    fn leave_clip(&mut self) {
        let parent = self.composer.clip.parent();
        self.ordering().break_for_clip(parent);
        self.composer.clip.pop();
    }

    /// Clear-fold discard: drop the scene output and its scratch, and hide the panes before the
    /// cover. Walk state survives: the clip stack is empty by precondition and transform pops may
    /// still be ahead.
    fn discard_composed(&mut self) {
        for row in &mut self.out.backdrops {
            row.source = PaneSource::Hidden;
        }
        self.out.discard_scene();
        self.composer.order.start_frame(self.out.display.physical);
    }
}
