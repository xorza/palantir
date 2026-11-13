//! One frame being composed, and the sink paint calls arrive through.

use crate::common::span::Span;
use crate::icons::icon_raster_key::IconRasterKey;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::geometry::urect::URect;
use crate::primitives::math::domain::EPS;
use crate::primitives::math::libm_ext::LibmVec2;
use crate::primitives::math::num::Vec2Ext;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::composer::clip_stack::ClipFrame;
use crate::renderer::frontend::composer::curve_run::CurveRun;
use crate::renderer::frontend::composer::geometry;
use crate::renderer::frontend::composer::lowering::{Lowering, ScaledRect};
use crate::renderer::frontend::composer::paint_order::{GroupClose, Ordering, TextClip};
use crate::renderer::frontend::composer::polyline_scratch::{PolylineLook, StagedColors};
use crate::renderer::frontend::composer::stroke_bbox::StrokeBbox;
use crate::renderer::frontend::composer::template_shape::TemplateShape;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_effect::{
    ClosedEffect, EffectCaptureDraw,
};
use crate::renderer::frontend::payload::draw_closed_payload::closed_fill::ClosedFill;
use crate::renderer::frontend::payload::draw_closed_payload::closed_image::ClosedImage;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_shadow_payload::DrawShadowPayload;
use crate::renderer::frontend::payload::draw_stroke_payload::{
    DrawStrokePayload, PolylinePath, StrokePath,
};
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::{BackdropRow, BackdropSource, MaterialLook};
use crate::renderer::render_buffer::clip_chain::ClipChain;
use crate::renderer::render_buffer::draw_run::Lane;
use crate::renderer::render_buffer::effect_draw_row::{EffectDrawRow, EffectInstance, EffectKey};
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::icon_draw_row::IconDrawRow;
use crate::renderer::render_buffer::image_draw_row::{ImageDrawRow, ImageInstance};
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::renderer::render_buffer::mesh_draw_row::{MeshDraw, MeshDrawRow, MeshInstance};
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::quad_draw_row::{QuadDrawRow, QuadInstance, QuadKey};
use crate::renderer::render_buffer::render_target_draw::RenderTargetDraw;
use crate::renderer::render_buffer::rounded_clip::RoundedClip;
use crate::renderer::render_buffer::shadow_instance::{ShadowInstance, ShadowKind};
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use crate::renderer::render_buffer::template_cover::TemplateCover;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::geometry::curve_basis::CurveBasis;
use crate::shape::geometry::stroke_geometry::PolylineColorSpan;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use crate::shape::record_store::RecordStore;
use crate::shape::stroke_bounds::StrokeJoints;
use crate::text;
use glam::{UVec2, Vec2};
use std::num::NonZeroU32;

/// One compose pass in flight: the [`Composer`]'s scratch bound to the buffer
/// being filled, the record payloads, and the display. Paint arrives through
/// [`PaintSink`] in authoring order: each draw is lowered here (`lowering`, `template_shape`,
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

    /// The rounded clips in force that cut a draw reaching the pixels `visible`: none where each
    /// clip of the chain covers every pixel center there whole, as most draws inside a rounded
    /// card lie; the chain otherwise. The chain's whole rect answers most draws; the clips' own
    /// test, exact for a convex clip, the rest.
    fn clip_chain(&mut self, visible: URect) -> ClipChain {
        let Some(frame) = self.composer.clip.top() else {
            return ClipChain::NONE;
        };
        if frame.chain.len == 0 {
            return ClipChain::NONE;
        }
        let centers = Rect::from(visible).deflated(0.5);
        // Innermost first: a draw near an edge most often crosses the nearest clip's.
        let whole = frame.whole.contains_rect(centers)
            || self.out.rounded_clips[frame.chain.range()]
                .iter()
                .rev()
                .all(|clip| clip.holds(centers));
        if whole {
            ClipChain::NONE
        } else {
            self.out.clips_drawn = true;
            ClipChain::new(frame.chain)
        }
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
        ordering.flush(GroupClose::End);
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
            // clip, copied, as a fragment reads each chain as one contiguous run. Re-pushing the
            // innermost clip adds no depth. An ancestor that holds the new clip's reach, its rect
            // grown by the antialiasing reach, covers it whole wherever the new clip covers any of
            // a pixel, so a product with it changes no bit: it leaves the chain, which nested cards
            // keep one clip deep, as Skia's `ClipStack` drops an element another contains. Refused
            // until a profile shows deep nesting: WebRender's clip-chain nodes with parent links,
            // which the fragment would walk through memory at each pixel.
            if self.out.rounded_clips[parent_chain.range()].last() == Some(&rc) {
                parent_chain
            } else {
                let chain_start = self.out.rounded_clips.len() as u32;
                if chain_start >= ClipChain::MAX_POOL {
                    geometry::rounded_clip_pool_overflow(chain_start);
                }
                let reach = physical.inflated(AA_HALF_WIDTH);
                for at in parent_chain.range() {
                    let ancestor = self.out.rounded_clips[at];
                    if !ancestor.holds(reach) {
                        self.out.rounded_clips.push(ancestor);
                    }
                }
                self.out.rounded_clips.push(rc);
                let depth = self.out.rounded_clips.len() as u32 - chain_start;
                if depth > ClipChain::MAX_DEPTH {
                    geometry::rounded_clip_depth_overflow(depth);
                }
                Span::new(chain_start, depth)
            }
        } else {
            // A rect clip inside rounded ancestors inherits their chain, which still cuts what it
            // holds.
            parent_chain
        };
        let whole = self.out.rounded_clips[chain.range()]
            .iter()
            .map(|clip| clip.covered())
            .reduce(Rect::clamp_to)
            .unwrap_or_default();
        self.enter_clip(ClipFrame {
            scissor,
            chain,
            whole,
        });
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

    fn fill(&mut self, p: DrawClosedPayload<ClosedFill>) {
        let lowering = self.lowering();
        let shape = TemplateShape::of(p.geometry, p.spin, &lowering).turned(p.spin, &lowering);
        let border_width = p.paint.border.width * lowering.transform.physical_scale();
        if self.fold_into_clear(&p, &shape, border_width) {
            return;
        }
        // The clipped rect is also the overlap test's rect, so an ancestor-clipped quad does not
        // force a needless flush.
        let Some(visible) = self.ordering().admit(Lane::Quad, shape.rect.urect) else {
            return;
        };
        let clip = self.clip_chain(visible);
        let form = FillForm::new(ShapeForm::of(&p.geometry), FillBrush::of(p.paint.fill.kind));
        self.push_quad(
            QuadKey::Fill(form),
            QuadInstance {
                header: shape.header(clip),
                fill: p.paint.fill.color,
                border_color: p.paint.border.color,
                border_width,
                fill_kind: p.paint.fill.kind,
                fill_lut_row: p.paint.fill.lut_row,
                fill_axis: p.paint.fill_axis,
            },
        );
        self.record_opaque_cover(&p, &shape);
    }

    /// A shadow in the quad stream, among the fills it lies under or over.
    fn shadow(&mut self, p: DrawShadowPayload) {
        let lowering = self.lowering();
        // The lanes are logical px, scaled here; the quad is built from the same lanes, so the
        // shader finds its source again.
        let geometry = p
            .shadow
            .geometry_f16
            .scaled_saturating(lowering.transform.physical_scale());
        let shape =
            TemplateShape::shadow(&p, ShadowGeometry::from_lanes(geometry.lanes()), &lowering);
        let Some(visible) = self.ordering().admit(Lane::Quad, shape.rect.urect) else {
            return;
        };
        let clip = self.clip_chain(visible);
        let shadow = ShadowInstance {
            header: shape.header(clip),
            color: p.shadow.color,
            kind: if p.shadow.inset() {
                ShadowKind::INSET
            } else {
                ShadowKind::DROP
            },
            geometry,
            unused: [0; 4],
        };
        self.push_quad(QuadKey::Shadow, QuadInstance::from(shadow));
    }

    fn mesh(&mut self, p: DrawMeshPayload) {
        let viewport_physical = self.out.display.physical;
        // `draw_mesh` already gated empty meshes, so `vertices` is non-empty. Inflate by 0.5 phys px
        // like polyline's AA fringe: overlap-test false negatives reorder paint. Mesh
        // skips snapping, so it cannot use `scaled_rect`.
        let to_physical = self.composer.transform.physical_map(p.origin);
        // The vertices are relative to `origin`, and so the pivot they turn about; the cull takes
        // the bound at this frame's angle.
        let spin = p.spin.map(|spin| spin.relative_to(p.origin));
        let bbox = spin.map_or(p.bbox, |spin| p.bbox.turned_cover(spin.pivot, spin.angle));
        let physical = to_physical.bbox(bbox);
        let fringe = Vec2::splat(AA_HALF_WIDTH);
        let mesh_urect = geometry::urect_from_physical(
            physical.min - fringe,
            physical.max() + fringe,
            viewport_physical,
        );
        // A surviving mesh closes the open text batch so its text emits before this above-text
        // geometry.
        let Some(visible) = self
            .ordering()
            .admit(Lane::Tier(PaintTier::Mesh), mesh_urect)
        else {
            return;
        };
        // Verts stay as recorded; the per-instance transform folds in the fit, the owner origin
        // and the transform stack, so the shader produces physical coords. The fit maps `pos` to
        // `o + S·pos`, and a turn about the pivot `c` maps that to `R(o + S·pos - c) + c`, so the
        // instance takes `s·R·S` as its linear part and `s·(R(o - c) + c)` into its translate.
        let scale_physical = self.composer.transform.physical_scale();
        let rotor = spin.map_or(Vec2::X, |spin| Vec2::libm_from_angle(spin.angle));
        let pivot = spin.map_or(Vec2::ZERO, |spin| spin.pivot);
        let x_axis = rotor * (scale_physical * p.fit.scale.x);
        let y_axis = rotor.perp() * (scale_physical * p.fit.scale.y);
        let translate = to_physical.point(rotor.rotate(p.fit.offset - pivot) + pivot);
        let clip = self.clip_chain(visible);
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
                clip,
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
        let Some(visible) = self.ordering().admit(Lane::Tier(PaintTier::Icon), urect) else {
            return;
        };
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
        let clip = self.clip_chain(visible);
        self.out.icons.push(IconDrawRow {
            key,
            origin,
            size,
            color: p.tint,
            desaturate: p.desaturate,
            spin,
            clip,
        });
    }

    /// An image over a square-cornered box, which clips nothing past the image's own rect, draws
    /// one quad over that rect; any other shape draws its template.
    fn image(&mut self, p: DrawClosedPayload<ClosedImage>) {
        match p.geometry {
            PlacedClosed::Box(PlacedBox { corners, .. }) if corners == Corners::ZERO => {
                self.plain_image(p);
            }
            _ => self.shaped_image(p),
        }
    }

    /// An effect over the shape a quad of it would shade, packed as one: snapped, or moved and
    /// turned about its center by a paint animation's turn, whose edges snap to no pixel. A
    /// material's box draws through its effect's material stage: it neither folds into the clear
    /// nor covers anything opaquely, as what it reads shows through it.
    fn effect(&mut self, p: DrawClosedPayload<ClosedEffect>) {
        let lowering = self.lowering();
        let scale = lowering.transform.physical_scale();
        let unturned = TemplateShape::of(p.geometry, p.spin, &lowering);
        // A halo reaches past its shape, so it is culled, ordered and clipped by what it covers.
        let halo = p.paint.outside.map(|outside| outside * scale);
        let covered = halo.map(|reach| unturned.halo(reach, p.spin, &lowering));
        let shape = unturned.turned(p.spin, &lowering);
        let border_width = p.paint.border().width * scale;
        let physical = shape.rect.physical;
        let Some(visible) = self.ordering().cull(
            covered
                .as_ref()
                .map_or(shape.rect.urect, |covered| covered.urect),
        ) else {
            return;
        };
        let look = p.paint.stage.map(|stage| MaterialLook {
            max_luminance: stage.max_luminance,
            min_luminance: stage.min_luminance,
            fill: stage.fill,
            fill_axis: stage.fill_axis,
            border_color: stage.border.color,
            border_width,
        });
        // A backdrop is listed before it is ordered, so its own paint is not among what it shares a
        // break past.
        let backdrop_idx = match p.paint.backdrop {
            Some(EffectCaptureDraw { ticket, reach }) => {
                // The rect, or its turn's cover, grown by its reach, inside the capture by half a
                // pixel, so a read at the window's edge blends no texel past it.
                let cover = match p.spin {
                    None => physical,
                    Some(spin) => physical.turned_cover(physical.center(), spin.angle),
                };
                let reads = cover
                    .inflated(reach.margin(scale, None) as f32)
                    .clamp_to(Rect::from(ticket.capture.footprint).deflated(0.5));
                self.list_backdrop(visible, ticket, |source| BackdropRow {
                    reads,
                    look,
                    capture: ticket.capture,
                    source,
                })
            }
            None => 0,
        };
        let form = ShapeForm::of(&p.geometry);
        let flags = if p.paint.stage.is_some() {
            EffectInstance::MATERIAL
        } else {
            0
        };
        self.ordering()
            .order(Lane::Tier(PaintTier::Effect), visible);
        let clip = self.clip_chain(visible);
        self.out.effects.push(EffectDrawRow {
            key: EffectKey {
                effect: p.paint.effect,
                form,
                cover: if halo.is_some() {
                    TemplateCover::Rim
                } else {
                    TemplateCover::Whole
                },
            },
            images: p.paint.images,
            instance: EffectInstance {
                header: shape.header(clip),
                border_width,
                // At least the antialiasing reach, so a halo draws the whole outer half of the
                // shape's edge.
                reach: halo.map_or(0.0, |reach| reach.max(AA_HALF_WIDTH)),
                flags,
                time: p.paint.time,
                opacity: p.paint.opacity,
                scale,
                params: p.paint.params.start,
                backdrop_idx,
                ..bytemuck::Zeroable::zeroed()
            },
        });
    }

    fn stroke(&mut self, p: DrawStrokePayload) {
        match p.path {
            StrokePath::Curve(basis) => self.curve(basis, &p),
            StrokePath::Polyline(path) => self.polyline(path, &p),
        }
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
        let clip = self.clip_chain(bounds);
        let row = TextDrawRow {
            origin: physical_rect.min,
            bounds,
            color: t.color,
            text: t.text,
            // Snap the ancestor-transform part of the text scale to 0.5% steps; continuous zoom
            // would mint a new glyph cache key every frame.
            scale: geometry::snap_text_scale(self.composer.transform.scale()),
            spin,
            clip,
        };
        // An unturned run's glyphs clip to its bounds where the text backend emits them
        // (`RasterPass::clip_since`), so only a turned one needs a scissor of its own bounds.
        let clip = if bounds == unclipped || spin.is_none() {
            TextClip::Whole
        } else {
            TextClip::Cut
        };
        self.ordering().push_text(row, clip);
    }
}

impl ComposeSession<'_> {
    /// How near its shape's edge, physical px, a shaped image's edge lies on it, so the shape
    /// alone fades it: a decal there would fade the edge twice, down to a quarter at its middle.
    /// The image's rect comes from the logical fit and the shape's from its own points, two f32
    /// paths whose shared edges part by a few ulps, under 1/128 px for a coordinate below 2^16;
    /// an image edge this far inside left open shows at most 4 of 255 levels of its clamped edge.
    const DECAL_SLACK_PX: f32 = 1.0 / 64.0;

    /// An image clipped to `placed`: its quad is the shape's rect, snapped, or moved and turned
    /// about its center as a quad's is, with the image's uv carried over the whole rect and its
    /// own rect relative to the quad.
    fn shaped_image(&mut self, p: DrawClosedPayload<ClosedImage>) {
        debug_assert!(
            p.paint.view.is_none(),
            "a `GpuView` composites a plain image"
        );
        let placed = p.geometry;
        let lowering = self.lowering();
        let unturned = TemplateShape::of(placed, p.spin, &lowering);
        // The image's rect lies on the pixels its shape's does.
        let edges = TemplateShape::edges(placed, p.spin, &lowering);
        let form = ShapeForm::of(&placed);
        let image = lowering.scaled_rect_with(p.paint.rect, edges).physical;
        if image.is_paint_empty() {
            return;
        }
        // The image's place in the shape, before a turn moves the two together.
        let size = Vec2::from(unturned.rect.physical.size);
        let lo = image.min - unturned.rect.physical.min;
        let shape = unturned.turned(p.spin, &lowering);
        let Some(visible) = self
            .ordering()
            .admit(Lane::Tier(PaintTier::Image), shape.rect.urect)
        else {
            return;
        };
        let hi = lo + Vec2::from(image.size);
        let uv_per_px = p.paint.uv_size / Vec2::from(image.size);
        let slack = Self::DECAL_SLACK_PX;
        let from = |at: f32| if at > slack { at } else { -f32::MAX };
        let to = |at: f32, end: f32| if at < end - slack { at } else { f32::MAX };
        let clip = self.clip_chain(visible);
        self.out.images.push(ImageDrawRow {
            id: p.paint.handle,
            key: form,
            instance: ImageInstance {
                header: shape.header(clip),
                uv_min: p.paint.uv_min - lo * uv_per_px,
                uv_size: size * uv_per_px,
                tint: p.paint.tint,
                flags: p.paint.flags,
                decal_min: Vec2::new(from(lo.x), from(lo.y)),
                decal_max: Vec2::new(to(hi.x, size.x), to(hi.y, size.y)),
                ..bytemuck::Zeroable::zeroed()
            },
        });
    }

    /// An image its own rect bounds: one quad over it.
    #[expect(
        clippy::cast_sign_loss,
        reason = "every value is clamped to a non-negative range before the cast"
    )]
    fn plain_image(&mut self, p: DrawClosedPayload<ClosedImage>) {
        let view = p.paint.view;
        // A turned image snaps to no pixel, and moves its center about the pivot as a quad does.
        let ScaledRect {
            physical: physical_rect,
            urect: image_urect,
        } = match p.spin {
            None => self.lowering().scaled_rect(p.paint.rect),
            Some(spin) => {
                let exact = self
                    .lowering()
                    .scaled_rect_with(p.paint.rect, EdgeRounding::Exact)
                    .physical;
                self.lowering().turned_rect(exact, spin)
            }
        };
        let Some(visible) = self
            .ordering()
            .admit(Lane::Tier(PaintTier::Image), image_urect)
        else {
            return;
        };
        // A `GpuView` draws over only its visible part, since that is all its target holds; a
        // turned one draws whole, as its visible part is not a rect of its own.
        let seen = view
            .filter(|_| p.spin.is_none())
            .and_then(|_| self.seen(physical_rect));
        let composite = seen.unwrap_or(physical_rect);
        let clip = self.clip_chain(visible);
        // A sharp box over what it composites, which the plain form draws as one quad.
        let mut instance = ImageInstance {
            header: ShapeHeader {
                rotation: p.spin.map_or(0.0, |spin| spin.angle),
                clip,
                ..ShapeHeader::of_box(composite)
            },
            uv_min: p.paint.uv_min,
            uv_size: p.paint.uv_size,
            tint: p.paint.tint,
            flags: p.paint.flags,
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
                id: p.paint.handle,
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
            id: p.paint.handle,
            key: ShapeForm::Rect,
            instance,
        });
    }

    /// Push `instance`, a quad of the stream that draws through `key`.
    fn push_quad(&mut self, key: QuadKey, instance: QuadInstance) {
        self.out.quads.push(QuadDrawRow { key, instance });
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

    /// A cubic or an arc of `p`: turned, culled by its centerline's bounds grown by the stroke,
    /// and lowered to its sub-instances.
    fn curve(&mut self, basis: CurveBasis, p: &DrawStrokePayload) {
        let basis = p.spin.map_or(basis, |spin| {
            CurveRun::turned(basis, spin.relative_to(p.origin))
        });
        let bbox_urect = StrokeBbox {
            transform: self.composer.transform.cur(),
            bbox: basis.bbox(),
            origin: p.origin,
            width_physical: p.width * self.composer.transform.physical_scale(),
            cap: p.cap,
            joints: StrokeJoints::Unjoined,
            display: self.out.display,
        }
        .urect();
        let Some(visible) = self
            .ordering()
            .admit(Lane::Tier(PaintTier::Curve), bbox_urect)
        else {
            return;
        };
        let run = CurveRun::lower(basis, p, &self.lowering());
        let clip = self.clip_chain(visible);
        run.push_into(&mut self.out.curves, clip);
    }

    /// The polyline of `p` along `path`: its points fitted, turned and taken to physical px, culled
    /// once and ordered once it proves to emit, then lowered to segments and joints.
    fn polyline(&mut self, path: PolylinePath, p: &DrawStrokePayload) {
        let display = self.out.display;
        let width_physical = p.width * self.composer.transform.physical_scale();

        // Inflating by the stroke fringe means the cull never trims reachable pixels. The points
        // are relative to `origin`, and so the pivot they turn about; the cull takes their bound
        // at this frame's angle.
        let spin = p.spin.map(|spin| spin.relative_to(p.origin));
        let bbox = StrokeBbox {
            transform: self.composer.transform.cur(),
            bbox: spin.map_or(path.bbox, |spin| {
                path.bbox.turned_cover(spin.pivot, spin.angle)
            }),
            origin: p.origin,
            width_physical,
            cap: p.cap,
            joints: StrokeJoints::polyline(path.points.len, path.join),
            display,
        }
        .urect();
        let Some(visible) = self.ordering().cull(bbox) else {
            return;
        };

        let src_points = &self.store.polyline_points[path.points.range()];
        let colors = &self.store.polyline_colors;
        let src_colors = match path.colors {
            PolylineColorSpan::Single => StagedColors::Single,
            PolylineColorSpan::PerPoint(span) => StagedColors::PerPoint(&colors[span.range()]),
            PolylineColorSpan::PerSegment(span) => StagedColors::PerSegment(&colors[span.range()]),
        };

        // Physical px with the owner origin folded in. No snapping: it would shift thin lines
        // off-axis.
        let to_physical = self.composer.transform.physical_map(p.origin);
        let rotor = spin.map(Spin::rotor);
        let kept = self.composer.polyline.keep(src_points.iter().map(|&q| {
            let q = path.fit.apply(q);
            to_physical.point(rotor.map_or(q, |rotor| rotor.apply(q)))
        }));
        if !kept {
            return;
        }
        // Only now that the polyline will emit geometry: an empty or culled one must not split the
        // batch or group.
        self.ordering().order(Lane::Tier(PaintTier::Curve), visible);
        let clip = self.clip_chain(visible);
        self.composer.polyline.emit(
            PolylineLook {
                colors: src_colors,
                color: p.fill.color,
                width: width_physical,
                cap: p.cap,
                join: path.join,
                clip,
            },
            &mut self.out.curves,
        );
    }

    /// Clear fold: an opaque, solid, sharp, unclipped quad covering the viewport is
    /// bit-identical to `LoadOp::Clear(fill)` and hides everything before it, so
    /// discard the scene so far and record the fill as the pass clear. The clip must be
    /// empty: a scissored cover hides only its scissor, and an empty clip guarantees no
    /// group in flight references the `rounded_clips` state the discard wipes.
    ///
    /// Returns `true` when folded; the quad must not be emitted.
    fn fold_into_clear(
        &mut self,
        p: &DrawClosedPayload<ClosedFill>,
        shape: &TemplateShape,
        border_width: f32,
    ) -> bool {
        let physical = shape.rect.physical;
        let covers_viewport = physical.min.x <= EPS
            && physical.min.y <= EPS
            && physical.max().x >= self.out.display.physical.as_vec2().x - EPS
            && physical.max().y >= self.out.display.physical.as_vec2().y - EPS;
        if !covers_viewport
            || self.composer.clip.top().is_some()
            || !p.is_solid_box()
            || !p.paint.fill.color.is_opaque()
            || !shape.is_sharp(border_width)
        {
            return false;
        }
        self.discard_composed();
        self.out.clear_override = Some(p.paint.fill.color);
        true
    }

    /// Opaque-cover annotation, for the occlusion pruner, of the quad just pushed. A solid box
    /// only ([`DrawClosedPayload::is_solid_box`]), and a turned quad covers no axis-aligned rect
    /// of its own. Its border insets nothing: an opaque fill reaches under it, so any border's
    /// band is opaque (`primitives/wgsl/composite.wgsl`). The rounded clips in force keep the
    /// cover to what they leave whole
    /// ([`RoundedClip::covered`]): past it the quad shows a share of what it covers, which a
    /// prune would drop.
    fn record_opaque_cover(&mut self, p: &DrawClosedPayload<ClosedFill>, shape: &TemplateShape) {
        if !p.is_solid_box() || !p.paint.fill.color.is_opaque() || shape.rotation != 0.0 {
            return;
        }
        let inscribed = shape.rect.physical.inscribed_for_corners(shape.corners);
        // A pixel-aligned sharp box has no antialiased edge to inset by.
        let aa_inset = if shape.is_pixel_aligned() {
            0.0
        } else {
            AA_HALF_WIDTH
        };
        let cover = self.out.rounded_clips[self.composer.clip.chain().range()]
            .iter()
            .fold(
                inscribed.deflated_by(Spacing::saturating([aa_inset; 4])),
                |cover, clip| cover.clamp_to(clip.covered()),
            );
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
            row.source = BackdropSource::Hidden;
        }
        self.out.discard_scene();
        self.composer.order.start_frame(self.out.display.physical);
    }
}
