//! One frame being composed, and the sink paint calls arrive through.

use crate::common::span::Span;
use crate::icons::icon_raster_key::IconRasterKey;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::geometry::urect::URect;
use crate::primitives::math::domain::{EPS, is_invisible};
use crate::primitives::math::libm_ext::LibmVec2;
use crate::primitives::math::num::Vec2Ext;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::composer::clip_stack::ClipFrame;
use crate::renderer::frontend::composer::curve_run::CurveRun;
use crate::renderer::frontend::composer::geometry;
use crate::renderer::frontend::composer::lowering::{Lowering, ScaledRect};
use crate::renderer::frontend::composer::packed_quad::PackedQuad;
use crate::renderer::frontend::composer::paint_order::{Lane, Ordering, TextClip};
use crate::renderer::frontend::composer::polyline_scratch::PolylineLook;
use crate::renderer::frontend::composer::stroke_bbox::StrokeBbox;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_backdrop_payload::{
    BackdropDraw, DrawBackdropPayload,
};
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_effect_payload::DrawEffectPayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::DrawImagePayload;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::backdrop_row::{
    BackdropRead, BackdropRow, BackdropSource, MaterialLook,
};
use crate::renderer::render_buffer::effect_draw_row::{EffectDrawRow, EffectInstance};
use crate::renderer::render_buffer::icon_draw_row::IconDrawRow;
use crate::renderer::render_buffer::image_draw_row::{ImageDrawRow, ImageInstance};
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::renderer::render_buffer::mesh_draw_row::{MeshDraw, MeshDrawRow, MeshInstance};
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::render_target_draw::RenderTargetDraw;
use crate::renderer::render_buffer::rounded_clip::RoundedClip;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use crate::renderer::render_buffer::{MAX_ROUNDED_CLIP_DEPTH, RenderBuffer};
use crate::shape::record_store::RecordStore;
use crate::shape::stroke_bounds::StrokeJoints;
use crate::text;
use glam::{UVec2, Vec2};
use std::num::NonZeroU32;

/// One compose pass in flight: the [`Composer`]'s scratch bound to the buffer
/// being filled, the record payloads, and the display. Paint arrives through
/// [`PaintSink`] in authoring order: each draw is lowered here (`lowering`, `packed_quad`,
/// `curve_run`, `polyline_scratch`) and ordered through [`Ordering`].
#[derive(Debug)]
#[must_use = "a session leaves its last batch and group open until `finish`"]
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

impl ComposeSession<'_> {
    /// End the session: close the trailing text batch and draw group, which the buffer's last
    /// rows wait on. Explicit, as `BufWriter::flush` is beside its drop, since the frame's order
    /// depends on when it runs. Refused: the work in `Drop`, which a `let _ =` ran at once and a
    /// held session ran wherever its scope ended.
    pub(crate) fn finish(mut self) {
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
            // mask, copied whole, as the backend reads each chain as one contiguous span. Re-pushing
            // the innermost mask adds no depth. The pool grows by the sum of the depths, a few
            // masks a clip at a UI's two to four rounded levels. Refused until a profile shows deep
            // nesting: WebRender's clip-chain nodes with parent links, which the backend would walk
            // back into a span at each draw.
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

    /// A material draws through its effect's material stage, over the rect a quad of it would
    /// shade, packed as one: it neither folds into the clear nor covers anything opaquely, as what
    /// it reads shows through it.
    fn backdrop(&mut self, p: DrawBackdropPayload) {
        let geometry = p.geometry();
        let packed = PackedQuad::pack(&geometry, &self.lowering());
        let Some(visible) = self.ordering().cull(packed.rect.urect) else {
            return;
        };
        let scale = self.composer.transform.physical_scale();
        // The opacity f16 as a quad holds it.
        let opacity = geometry.fill_axis.lanes()[3];
        let material = p.material;
        let look = MaterialLook {
            max_luminance: material.max_luminance(),
            min_luminance: material.min_luminance(),
            fill: p.fill,
            fill_axis: p.fill_axis,
            stroke_color: p.stroke.color,
            stroke_width: packed.stroke_width,
        };
        // Listed before it is ordered, so its own paint is not among what it shares a break past.
        let backdrop_idx = match p.read {
            BackdropDraw::Capture(ticket) => {
                self.list_backdrop(visible, ticket, |source| BackdropRow {
                    reads: ticket.capture.footprint.into(),
                    look: Some(look),
                    read: BackdropRead::Capture {
                        capture: ticket.capture,
                        source,
                    },
                })
            }
            BackdropDraw::Ground => self.list_ground(BackdropRow {
                reads: packed.rect.physical,
                look: Some(look),
                read: BackdropRead::Ground,
            }),
        };
        self.ordering()
            .order(Lane::Tier(PaintTier::Effect), visible);
        self.out.effects.push(EffectDrawRow {
            effect: material.effect(),
            images: Span::default(),
            instance: EffectInstance {
                rect: packed.rect.physical,
                corners: packed.corners.to_array(),
                time: 0.0,
                opacity,
                scale,
                rotation: packed.rotation,
                params: material.params,
                backdrop_idx,
                flags: match p.coverage {
                    Coverage::Inside => EffectInstance::MATERIAL,
                    Coverage::Outside => EffectInstance::MATERIAL | EffectInstance::WINDOW,
                },
                ..bytemuck::Zeroable::zeroed()
            },
        });
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
        // Verts stay as recorded; the per-instance transform folds in the fit, the owner origin
        // and the transform stack, so the shader produces physical coords. The fit maps `pos` to
        // `o + S·pos`, and a turn about the pivot `c` maps that to `R(o + S·pos - c) + c`, so the
        // instance takes `s·R·S` as its linear part and `s·(R(o - c) + c)` into its translate.
        let scale_physical = self.composer.transform.physical_scale();
        let rotor = p
            .spin
            .map_or(Vec2::X, |spin| Vec2::libm_from_angle(spin.angle));
        let pivot = p.spin.map_or(Vec2::ZERO, |spin| spin.pivot);
        let x_axis = rotor * (scale_physical * p.fit.scale.x);
        let y_axis = rotor.perp() * (scale_physical * p.fit.scale.y);
        let translate = to_physical.point(rotor.rotate(p.fit.offset - pivot) + pivot);
        self.out.meshes.push(MeshDrawRow {
            draw: MeshDraw {
                vertices: p.vertices,
                idxs: p.idxs,
            },
            instance: MeshInstance {
                translate,
                x_axis,
                y_axis,
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
    fn image(&mut self, p: DrawImagePayload) {
        let view = p.view;
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
            // A recorder with no device has no ceiling.
            let max = self
                .composer
                .texture_limit
                .max_dimension()
                .map_or(u32::MAX, NonZeroU32::get);
            let cap = i64::from(max);
            let whole = physical_rect.size;
            // The cap is measured against the whole view so the downsample does not change as the
            // view scrolls (it would shimmer).
            let downsample = (max as f32 / whole.w.max(whole.h)).min(1.0);
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
                owner: view.owner,
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

    /// An effect over the rect a quad of it would shade, packed as one: snapped, or moved and
    /// turned about its center by a paint animation's turn, whose edges snap to no pixel.
    fn effect(&mut self, p: DrawEffectPayload) {
        let scaled = match p.spin {
            None => self.lowering().scaled_rect(p.rect),
            Some(spin) => {
                let exact = self
                    .lowering()
                    .scaled_rect_with(p.rect, EdgeRounding::Exact);
                self.lowering().turned_rect(exact.physical, spin)
            }
        };
        let physical = scaled.physical;
        let Some(visible) = self.ordering().cull(scaled.urect) else {
            return;
        };
        let scale = self.composer.transform.physical_scale();
        // A backdrop is listed before it is ordered, so its own paint is not among what it shares a
        // break past.
        let backdrop_idx = match p.backdrop {
            Some(backdrop) => {
                // The rect, or its turn's cover, grown by its reach.
                let cover = match p.spin {
                    None => physical,
                    Some(spin) => physical.turned_cover(physical.center(), spin.angle),
                };
                match backdrop.read {
                    // Inside the capture by half a pixel, so a read at the window's edge blends no
                    // texel past it.
                    BackdropDraw::Capture(ticket) => {
                        let reads = cover
                            .inflated(backdrop.reach * scale)
                            .clamp_to(Rect::from(ticket.capture.footprint).deflated(0.5));
                        self.list_backdrop(visible, ticket, |source| BackdropRow {
                            reads,
                            look: None,
                            read: BackdropRead::Capture {
                                capture: ticket.capture,
                                source,
                            },
                        })
                    }
                    BackdropDraw::Ground => self.list_ground(BackdropRow {
                        reads: cover,
                        look: None,
                        read: BackdropRead::Ground,
                    }),
                }
            }
            None => 0,
        };
        self.ordering()
            .order(Lane::Tier(PaintTier::Effect), visible);
        self.out.effects.push(EffectDrawRow {
            effect: p.effect,
            images: p.images,
            instance: EffectInstance {
                rect: physical,
                corners: p.corners.fit_to(physical.size, scale).to_array(),
                time: p.time,
                opacity: p.opacity,
                scale,
                rotation: p.spin.map_or(0.0, |spin| spin.angle),
                params: p.params.start,
                backdrop_idx,
                ..bytemuck::Zeroable::zeroed()
            },
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
        let kept = self.composer.polyline.keep(src_points.iter().map(|&q| {
            let q = p.fit.apply(q);
            to_physical.point(rotor.map_or(q, |rotor| rotor.apply(q)))
        }));
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
        let physical_rect = self
            .composer
            .transform
            .physical_rect(t.rect, self.out.display.edge_rounding());
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
        let clip = if bounds == unclipped {
            TextClip::Whole
        } else {
            TextClip::Cut
        };
        self.ordering().push_text(row, clip);
    }
}

impl ComposeSession<'_> {
    /// Push `p`, packed as `packed`, with `lut_row` its gradient row or its backdrop's row.
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

    /// List `row`, a backdrop over the window's ground, which takes no capture and breaks no
    /// pass; its index is its effect instance's `backdrop`.
    fn list_ground(&mut self, row: BackdropRow) -> u32 {
        debug_assert!(
            row.read == BackdropRead::Ground,
            "a ground row reads the ground"
        );
        let idx = self.out.backdrops.len() as u32;
        self.out.backdrops.push(row);
        idx
    }

    /// List the backdrop of `ticket`, seen at `visible`, as `row` makes it from the source paint
    /// order gives it ([`Ordering::backdrop_source`]); its index is its effect instance's
    /// `backdrop`.
    fn list_backdrop(
        &mut self,
        visible: URect,
        ticket: BackdropTicket,
        row: impl FnOnce(BackdropSource) -> BackdropRow,
    ) -> u32 {
        debug_assert!(
            ticket.capture.footprint.union(visible) == ticket.capture.footprint,
            "a backdrop's footprint holds what it shows"
        );
        let source = self.ordering().backdrop_source(ticket);
        let idx = self.out.backdrops.len() as u32;
        self.out.backdrops.push(row(source));
        idx
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

    /// Clear-fold discard: drop the scene output and its scratch, and hide the backdrops before the
    /// cover. Walk state survives: the clip stack is empty by precondition and transform pops may
    /// still be ahead.
    fn discard_composed(&mut self) {
        for row in &mut self.out.backdrops {
            if let BackdropRead::Capture { source, .. } = &mut row.read {
                *source = BackdropSource::Hidden;
            }
        }
        self.out.discard_scene();
        self.composer.order.start_frame(self.out.display.physical);
    }
}
