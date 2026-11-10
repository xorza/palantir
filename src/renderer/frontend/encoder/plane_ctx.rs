//! [`PlaneCtx`]: one plane's encode walk, built per tree by [`Encoder::encode`](crate::renderer::frontend::encoder::Encoder::encode).

use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::layout::plane_layout::PlaneLayout;
use crate::layout::text::text_runs::TextRuns;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::{FitRect, ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::paint::line_style::LineCap;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::encoder::cull_region::CullRegion;
use crate::renderer::frontend::encoder::gradient_resolver::GradientPass;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_backdrop_payload::BackdropDraw;
use crate::renderer::frontend::payload::draw_backdrop_payload::{
    BackdropOutline, DrawBackdropPayload,
};
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_effect_payload::{
    DrawEffectPayload, EffectCaptureDraw,
};
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::{DrawImagePayload, GpuViewDraw};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::{
    DrawQuadPayload, RectQuad, TriangleQuad,
};
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::scene::tree::Tree;
use crate::scene::tree::backdrop_entry::BackdropOrigin;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_animations::PaintAnimationCursor;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::placed_stroke::PlacedStroke;
use crate::shape::geometry::shape_outline::PlacedOutline;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::lowered_stroke::LoweredStroke;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint_animation::paint_mod::PaintMod;
use crate::shape::record::{EffectBackdrop, ShapeRecord, TextAnchor};
use crate::shape::record_store::recorded_gradients::GradientId;
use crate::text::shaped_ref::ShapedTextRef;
use glam::Vec2;
use std::time::Duration;

/// One shape of a node's paint stream: its owner, its index in the owner's
/// shapes, and the record.
#[derive(Clone, Copy, Debug)]
struct ShapeItem<'s> {
    owner: NodeId,
    idx: u32,
    record: &'s ShapeRecord,
}

/// The fixed inputs one plane's encode walk reads.
#[derive(Debug)]
pub(super) struct PlaneCtx<'a, 'g> {
    pub(super) tree: &'a Tree,
    pub(super) layout: &'a PlaneLayout,
    pub(super) cascade_inputs: &'a [CascadeInputHash],
    pub(super) subtree_paint_rects: &'a [Rect],
    pub(super) gradients: &'a mut GradientPass<'g>,
    pub(super) paint_animation_cursor: PaintAnimationCursor<'a>,
    pub(super) damage_filter: Option<CullRegion<'a>>,
    /// The plane's backdrops in paint order, and the ticket of each.
    pub(super) backdrops: &'a [PlaneBackdrop],
    pub(super) tickets: &'a [Option<BackdropTicket>],
    /// The first of `backdrops` not yet painted: backdrops paint in their order.
    pub(super) next_backdrop: usize,
    /// Logical-px inflation of each `subtree_paint_rect` before the damage cull, covering the AA pad the backend PreClears.
    pub(super) damage_cull_margin: f32,
    pub(super) viewport: Rect,
    pub(super) now: Duration,
}

/// A rect as one paint call draws it: its quad, a paint animation's turn, and its opacity.
#[derive(Clone, Copy, Debug)]
struct RectPaint {
    quad: RectQuad,
    spin: Option<Spin>,
    alpha: f32,
}

/// What makes a rect a material's box: its material, and what it reads, the ticket the backdrop
/// ledger wrote for it or the ground.
#[derive(Clone, Copy, Debug)]
struct BackdropLook {
    material: LoweredMaterial,
    read: BackdropDraw,
}

/// A curve or an arc record placed in its owner, with the stroke that draws it.
#[derive(Clone, Copy, Debug)]
struct NativeStroke {
    place: PlacedStroke,
    cap: LineCap,
    stroke: LoweredStroke,
    ramp: CurveRamp,
}

/// A box as one paint call draws it: its rect, its shadow, its material, and where its backdrop is
/// recorded.
#[derive(Clone, Copy, Debug)]
struct BoxPaint {
    paint: RectPaint,
    shadow: LoweredShadow,
    material: Option<LoweredMaterial>,
    site: BackdropSite,
}

/// Where a backdrop is recorded: by its owner, as its chrome or its shape at `shape_idx`.
#[derive(Clone, Copy, Debug)]
struct BackdropSite {
    owner: NodeId,
    origin: BackdropOrigin,
    shape_idx: u32,
}

/// Paint `paint` as an ordinary rect.
fn paint_rect(paint: RectPaint, out: &mut impl PaintSink) {
    let RectPaint { quad, spin, alpha } = paint;
    out.draw(DrawQuadPayload::rect(quad).turned(spin), alpha);
}

/// Paint `paint` as a material's box over what it reads, through `look`: one draw, its fill, a
/// color or a gradient, and its border over the whole box.
fn paint_backdrop(paint: RectPaint, look: BackdropLook, out: &mut impl PaintSink) {
    let RectPaint { quad, spin, alpha } = paint;
    out.draw(
        DrawBackdropPayload {
            outline: BackdropOutline::Rect {
                coverage: quad.coverage,
                rect: quad.rect,
                corners: quad.corners,
            },
            spin,
            fill: quad.fill.gpu_fill(),
            fill_axis: quad.fill.fill_axis(),
            border: quad.border.normalized(),
            material: look.material,
            opacity: 1.0,
            read: look.read,
        },
        alpha,
    );
}

impl PlaneCtx<'_, '_> {
    /// What makes the rect recorded at `site` a material's box: `material` and what it reads;
    /// `None` for a rect of no material, or a backdrop the cascade lists not.
    fn backdrop_look(
        &mut self,
        site: BackdropSite,
        material: Option<LoweredMaterial>,
    ) -> Option<BackdropLook> {
        let material = material?;
        let read = if material.captures() {
            BackdropDraw::Capture(self.backdrop_ticket(site)?)
        } else {
            BackdropDraw::Ground
        };
        Some(BackdropLook { material, read })
    }

    /// Paint a box, a node's chrome or a rect shape: its drop shadow under its fill, its fill or
    /// its material, and its inset shadow over the fill inside its border, as CSS Backgrounds 3
    /// §7.1 orders them.
    fn paint_box(&mut self, item: BoxPaint, out: &mut impl PaintSink) {
        let BoxPaint {
            paint,
            shadow,
            material,
            site,
        } = item;
        let backdrop = self.backdrop_look(site, material);
        let paint_fill = |out: &mut _| match backdrop {
            Some(look) => paint_backdrop(paint, look, out),
            None => paint_rect(paint, out),
        };
        let RectPaint { quad, spin, alpha } = paint;
        if shadow.inset() {
            paint_fill(out);
            let width = quad.border.width;
            let padding_box = Rect {
                min: Vec2::ZERO,
                size: quad.rect.size,
            }
            .deflated(width);
            let source = quad.rect.resolve_local(Some(padding_box));
            let corners = quad
                .corners
                .inner(quad.rect.size, Spacing::saturating([width; 4]));
            if let Some(shadow) = DrawQuadPayload::shadow(source, corners, shadow) {
                out.draw(shadow.turned(spin), alpha);
            }
        } else {
            // The box's rect, as `compute_paint_rect` and `QuadShape::bbox_local` mirror.
            if let Some(shadow) = DrawQuadPayload::shadow(quad.rect, quad.corners, shadow) {
                out.draw(shadow.turned(spin), alpha);
            }
            paint_fill(out);
        }
    }

    /// Draw `native` in `owner_rect`, faded and turned by `paint_mod`: its curve on the curve
    /// pipeline, owner-local, the basis crossing verbatim so cull, spin and sub-instance sizing
    /// stay one code path; or its cap's dot as a box, in the stroke color times the ramp at its
    /// start.
    fn paint_native_stroke(
        &mut self,
        native: NativeStroke,
        owner_rect: Rect,
        paint_mod: PaintMod,
        out: &mut impl PaintSink,
    ) {
        let NativeStroke {
            place,
            cap,
            stroke,
            ramp,
        } = native;
        match place {
            PlacedStroke::Curve(basis) => {
                let ramp_row = match ramp {
                    CurveRamp::None => None,
                    CurveRamp::Interned { id, hash: _ } => Some(self.gradient_row(id)),
                };
                out.draw(
                    DrawCurvePayload {
                        basis,
                        bounds: StrokeBounds::new(owner_rect, basis.bbox(), paint_mod.rotation),
                        origin: owner_rect.min,
                        fill: GpuFill::curve(stroke.color, ramp_row),
                        width: stroke.width,
                        cap,
                    },
                    paint_mod.alpha,
                );
            }
            PlacedStroke::Dot(PlacedBox { rect, corners }) => {
                let color = match ramp {
                    CurveRamp::None => stroke.color,
                    CurveRamp::Interned { id, hash: _ } => RgbaF32::from(stroke.color)
                        .tinted(self.gradients.ramp(id).color_at(0.0))
                        .to_f16(),
                };
                let quad = RectQuad {
                    coverage: Coverage::Inside,
                    rect: owner_rect.resolve_local(Some(rect)),
                    corners,
                    fill: BrushSource::Solid(color),
                    border: LoweredBorder::NONE,
                };
                let spin = Spin::about_owner(owner_rect, Vec2::ZERO, paint_mod.rotation);
                paint_rect(
                    RectPaint {
                        quad,
                        spin,
                        alpha: paint_mod.alpha,
                    },
                    out,
                );
            }
            PlacedStroke::Nothing => {}
        }
    }

    /// The ticket of the backdrop recorded at `site`, as the tree lists it; `None` for a backdrop
    /// the cascade lists not, whose rect lies outside every clip.
    fn backdrop_ticket(&mut self, site: BackdropSite) -> Option<BackdropTicket> {
        let BackdropSite {
            owner,
            origin,
            shape_idx,
        } = site;
        let entries = &self.tree.backdrops;
        let at = entries
            .binary_search_by_key(&(shape_idx, origin), |entry| {
                (entry.shape_idx, entry.origin)
            })
            .expect("a painted backdrop is listed");
        let key = BackdropKey {
            owner: self.tree.records.widget_id()[owner.idx()],
            shape: entries[at].row,
        };
        let Some(ahead) = self.backdrops[self.next_backdrop..]
            .iter()
            .position(|backdrop| backdrop.key == key)
        else {
            debug_assert!(
                !self.backdrops[..self.next_backdrop]
                    .iter()
                    .any(|backdrop| backdrop.key == key),
                "backdrops paint in the cascade's order"
            );
            return None;
        };
        let at = self.next_backdrop + ahead;
        self.next_backdrop = at + 1;
        let ticket = self.tickets[at];
        if let Some(ticket) = ticket {
            debug_assert_eq!(ticket.capture.key, key, "a backdrop's ticket is its own");
        }
        ticket
    }

    #[inline]
    fn brush_source(&mut self, brush: LoweredBrush) -> BrushSource {
        self.gradients.source(brush)
    }

    fn gradient_row(&mut self, id: GradientId) -> LutRow {
        self.gradients.resolve(id).lut_row
    }

    /// Emits one of a node's shapes; `runs` is that node's [`TextRuns`] cursor, advanced here.
    fn emit_one_shape(
        &mut self,
        item: ShapeItem<'_>,
        runs: &mut TextRuns,
        out: &mut impl PaintSink,
    ) {
        let ShapeItem {
            owner: id,
            idx: shape_idx,
            record: shape,
        } = item;
        // Shape setters check every input, so nothing non-finite reaches here; later non-finite checks guard the transform stack only.
        // No "not a no-op" assert: that needs resolved geometry, which first exists in `Draw*Payload::is_noop`.
        debug_assert!(
            !shape.has_nan(),
            "a NaN reached the encoder past the shape setters' checks: {shape:?}",
        );
        let shaped = runs.shaped(shape, self.layout);
        // Early-out so a shape animated to nothing doesn't pay for its geometry; `paint_mod.rotation` rides the stroke arms via `StrokeBounds`.
        let paint_mod = self.paint_animation_cursor.sample(shape_idx, self.now);
        if is_invisible(paint_mod.alpha) {
            return;
        }
        let alpha = paint_mod.alpha;
        let owner_rect = self.layout.rect[id.idx()];
        // The turn for payloads placed in the logical placement; a payload with an origin of its own
        // takes `Spin::about_owner` with that origin.
        let spin = Spin::about_owner(owner_rect, Vec2::ZERO, paint_mod.rotation);
        match shape {
            ShapeRecord::Quad(shape) => match shape {
                QuadShape::Box {
                    geometry,
                    fill,
                    border,
                    shadow,
                    material,
                } => {
                    let PlacedBox { rect, corners } = geometry.place(owner_rect.size);
                    let quad = RectQuad {
                        coverage: geometry.coverage,
                        rect: owner_rect.resolve_local(Some(rect)),
                        corners,
                        fill: self.brush_source(*fill),
                        border: *border,
                    };
                    self.paint_box(
                        BoxPaint {
                            paint: RectPaint { quad, spin, alpha },
                            shadow: *shadow,
                            material: *material,
                            site: BackdropSite {
                                owner: id,
                                origin: BackdropOrigin::Shape,
                                shape_idx,
                            },
                        },
                        out,
                    );
                }
                QuadShape::Triangle {
                    geometry,
                    fill,
                    border,
                    material,
                } => {
                    let Some(placed) = geometry.place(owner_rect.size) else {
                        return;
                    };
                    // Placed in the logical placement, as a rect is.
                    let points = placed.centers.map(|q| owner_rect.min + q);
                    let fill = self.brush_source(*fill);
                    let site = BackdropSite {
                        owner: id,
                        origin: BackdropOrigin::Shape,
                        shape_idx,
                    };
                    if let Some(look) = self.backdrop_look(site, *material) {
                        out.draw(
                            DrawBackdropPayload {
                                outline: BackdropOutline::Triangle {
                                    points,
                                    radius: placed.radius,
                                },
                                spin,
                                fill: fill.gpu_fill(),
                                fill_axis: fill.fill_axis(),
                                border: border.normalized(),
                                material: look.material,
                                opacity: 1.0,
                                read: look.read,
                            },
                            alpha,
                        );
                    } else {
                        let triangle = DrawQuadPayload::triangle(TriangleQuad {
                            points,
                            radius: placed.radius,
                            fill,
                            border: *border,
                        });
                        out.draw(triangle.turned(spin), alpha);
                    }
                }
            },
            ShapeRecord::Text {
                local_origin,
                text,
                color,
                align,
                ..
            } => {
                let shaped = shaped.expect("a text record always draws its run from the cursor");
                let Some(key) = shaped.key else {
                    tracing::trace!(?shape, "encoder: dropping text that shaped no buffer");
                    return;
                };
                // `local_rect: None`: the encoder places the bbox in the owner's padded inner rect via `Align::place_in`; `Some(origin)`: the widget places it at `owner.min + origin`. Shared placement, so the cascade's paint rect and damage agree with paint.
                let anchor = TextAnchor {
                    local_origin: *local_origin,
                    align: *align,
                };
                let local = anchor.bbox_local(
                    owner_rect.size,
                    self.tree.records.layout()[id.idx()].padding,
                    shaped.extent.size,
                );
                let rect = owner_rect.resolve_local(Some(local));
                out.draw(
                    DrawTextPayload {
                        rect,
                        ink: shaped.extent.ink,
                        color: *color,
                        text: ShapedTextRef::new(key, text),
                        spin,
                    },
                    alpha,
                );
            }
            ShapeRecord::Polyline {
                width,
                color_mode,
                cap,
                join,
                points,
                colors,
                bbox,
                placement,
                content_hash: _,
            } => {
                let fit = placement.map(*bbox, owner_rect.size);
                out.draw(
                    DrawPolylinePayload {
                        bounds: StrokeBounds::new(
                            owner_rect,
                            fit.apply_rect(*bbox),
                            paint_mod.rotation,
                        ),
                        origin: owner_rect.min,
                        fit,
                        width: *width,
                        points: *points,
                        colors: *colors,
                        color_mode: *color_mode,
                        cap: *cap,
                        join: *join,
                        alpha: 1.0,
                    },
                    alpha,
                );
            }
            ShapeRecord::Mesh {
                placement,
                tint,
                vertices,
                idxs,
                bbox,
                content_hash: _,
            } => {
                let fit = placement.map(*bbox, owner_rect.size);
                out.draw(
                    DrawMeshPayload {
                        bbox: fit.apply_rect(*bbox),
                        origin: owner_rect.min,
                        fit,
                        tint: *tint,
                        vertices: *vertices,
                        idxs: *idxs,
                        spin: Spin::about_owner(owner_rect, owner_rect.min, paint_mod.rotation),
                    },
                    alpha,
                );
            }
            ShapeRecord::Curve {
                cap,
                geometry,
                stroke,
                ramp,
            } => {
                let native = NativeStroke {
                    place: geometry.place(owner_rect.size, *cap, stroke.width),
                    cap: *cap,
                    stroke: *stroke,
                    ramp: *ramp,
                };
                self.paint_native_stroke(native, owner_rect, paint_mod, out);
            }
            ShapeRecord::Arc {
                cap,
                geometry,
                stroke,
                ramp,
            } => {
                let native = NativeStroke {
                    place: geometry.place(owner_rect.size, *cap, stroke.width),
                    cap: *cap,
                    stroke: *stroke,
                    ramp: *ramp,
                };
                self.paint_native_stroke(native, owner_rect, paint_mod, out);
            }
            // An effect fades, turns and reads its time with its paint animation. A backdrop effect
            // the cascade lists not lies outside every clip.
            ShapeRecord::Effect {
                backdrop,
                outline,
                effect,
                params,
                images,
                content_hash: _,
            } => {
                let backdrop = match backdrop {
                    Some(EffectBackdrop::Capture(read)) => {
                        let site = BackdropSite {
                            owner: id,
                            origin: BackdropOrigin::Shape,
                            shape_idx,
                        };
                        let Some(ticket) = self.backdrop_ticket(site) else {
                            return;
                        };
                        Some(EffectCaptureDraw {
                            read: BackdropDraw::Capture(ticket),
                            reach: read.reach,
                        })
                    }
                    Some(EffectBackdrop::Ground) => Some(EffectCaptureDraw {
                        read: BackdropDraw::Ground,
                        reach: 0.0,
                    }),
                    None => None,
                };
                let Some(outline) = outline.place(owner_rect) else {
                    return;
                };
                out.draw(
                    DrawEffectPayload {
                        outline,
                        spin,
                        effect: *effect,
                        params: *params,
                        images: *images,
                        backdrop,
                        opacity: 1.0,
                        time: paint_mod.time,
                    },
                    alpha,
                );
            }
            ShapeRecord::Icon {
                local_rect,
                handle,
                fit,
                tint,
                desaturate,
            } => {
                let base = owner_rect.resolve_local(*local_rect);
                out.draw(
                    DrawIconPayload {
                        rect: fit.place_artwork(base, handle.view_box()),
                        icon: handle.icon,
                        tint: *tint,
                        desaturate: *desaturate,
                        spin,
                    },
                    alpha,
                );
            }
            ShapeRecord::Image {
                outline,
                tint,
                source,
                fit,
                min_filter,
                mag_filter,
                downsample,
            } => {
                // The rect the image fits in, and the shape that clips it, where one does.
                let Some(placed) = outline.place(owner_rect) else {
                    return;
                };
                let base = placed.bounds();
                // Square corners clip nothing past the image's own rect.
                let outline = match placed {
                    PlacedOutline::Rect { corners, .. } if corners == Corners::ZERO => None,
                    _ => Some(placed),
                };
                // Either source carries its texture id; a `GpuView` also names its owner, whose
                // callback the backend finds in the window's `GpuViews`, and the epoch telling
                // the backend whether the target is current.
                let (handle, view) = match source {
                    ImageSource::Texture { id, .. } => (*id, None),
                    ImageSource::GpuView { id: target, epoch } => {
                        let owner = self.tree.records.widget_id()[id.idx()];
                        (
                            *target,
                            Some(GpuViewDraw {
                                owner,
                                epoch: *epoch,
                            }),
                        )
                    }
                };
                let FitRect {
                    rect,
                    uv_min,
                    uv_size,
                } = fit.resolve(base, source.intrinsic());
                let mut flags = ImageFlags::NONE;
                if matches!(*fit, ImageFit::Tile { .. }) {
                    flags = flags.union(ImageFlags::TILED);
                }
                if *min_filter == ImageFilter::Nearest {
                    flags = flags.union(ImageFlags::MIN_NEAREST);
                }
                if *mag_filter == ImageFilter::Nearest {
                    flags = flags.union(ImageFlags::MAG_NEAREST);
                }
                flags = flags.union(match *downsample {
                    ImageDownsample::Single => ImageFlags::NONE,
                    ImageDownsample::Mean => ImageFlags::TAPS_MEAN,
                    ImageDownsample::Peak => ImageFlags::TAPS_PEAK,
                });
                out.draw(
                    DrawImagePayload {
                        rect,
                        outline,
                        uv_min,
                        uv_size,
                        tint: *tint,
                        handle,
                        flags,
                        spin,
                        view,
                    },
                    alpha,
                );
            }
        }
    }

    /// Paints `id` and its subtree in paint order: the whole recursive walk, called once per root.
    pub(super) fn encode_node(&mut self, id: NodeId, out: &mut impl PaintSink) {
        if self.cascade_inputs[id.idx()].invisible() {
            return;
        }

        let subtree_paint_rect = self.subtree_paint_rects[id.idx()];
        if !subtree_paint_rect.intersects(self.viewport) {
            return;
        }

        // Damage cull, inflated by `damage_cull_margin` to cover the AA pad the backend PreClears, else a node in the pad ring is cleared but skipped.
        if let Some(region) = self.damage_filter
            && !region.any_intersects(subtree_paint_rect.inflated(self.damage_cull_margin))
        {
            return;
        }

        let rect = self.layout.rect[id.idx()];

        // Clip is in parent space (pre-transform); the transform applies inside it, to children only. Chrome paints before the clip is pushed: `Tree::open_node` folds its border into the padding that deflates the clip, and chrome self-clips via its SDF. Chrome is `None` only when every part is no-op.
        let mode = self.tree.records.attrs()[id.idx()].clip_mode();
        let clip = mode.is_clip();
        let chrome = self.tree.chrome(id);

        if let Some(chrome) = chrome {
            let quad = RectQuad {
                coverage: Coverage::Inside,
                rect,
                corners: chrome.corners,
                fill: self.brush_source(chrome.fill),
                border: chrome.border,
            };
            // Alpha `1.0`: chrome isn't a shape, so no paint animation.
            self.paint_box(
                BoxPaint {
                    paint: RectPaint {
                        quad,
                        spin: None,
                        alpha: 1.0,
                    },
                    shadow: chrome.shadow,
                    material: chrome.material,
                    site: BackdropSite {
                        owner: id,
                        origin: BackdropOrigin::Chrome,
                        shape_idx: self.tree.records.shape_span()[id.idx()].start,
                    },
                },
                out,
            );
            if chrome.ring {
                let ring = DrawQuadPayload::rect(RectQuad {
                    coverage: Coverage::Inside,
                    rect,
                    corners: chrome.corners,
                    fill: self.brush_source(LoweredBrush::Solid(RgbaF16::TRANSPARENT)),
                    border: self.tree.focus_ring,
                });
                out.draw(ring, 1.0);
            }
        }

        if clip {
            let layout = self.tree.records.layout()[id.idx()];
            let mask_rect = layout.inner_rect(rect);
            match mode {
                ClipMode::Rect => out.push_clip(PushClipPayload::rect(mask_rect)),
                ClipMode::Rounded => {
                    let painted = chrome.map(|row| row.corners).expect(
                        "ClipMode::Rounded without chrome row — open_node invariant violated",
                    );
                    out.push_clip(PushClipPayload {
                        rect: mask_rect,
                        corners: painted.inner(rect.size, layout.padding),
                    });
                }
                ClipMode::None => {}
            }
        }

        let transform = self.tree.anchored_transform(id, rect);

        // The body paints inside the node's transform; chrome stays in parent space, so a transform pans/zooms content while the chrome stays anchored.
        if let Some(t) = transform {
            out.push_transform(t);
        }
        let mut runs = TextRuns::new(self.layout.text_spans[id.idx()]);
        let tree = self.tree;
        for item in tree.tree_items(id) {
            match item {
                TreeItem::ShapeRecord(idx, record) => {
                    let item = ShapeItem {
                        owner: id,
                        idx,
                        record,
                    };
                    self.emit_one_shape(item, &mut runs, out);
                }
                TreeItem::Child(child) => {
                    self.encode_node(child.id, out);
                }
            }
        }
        debug_assert!(
            runs.is_drained(),
            "encoder text count differs from the node's shaped-text span",
        );
        if transform.is_some() {
            out.pop_transform();
        }

        if clip {
            out.pop_clip();
        }
    }
}
