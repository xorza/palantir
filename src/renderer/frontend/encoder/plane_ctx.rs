//! [`PlaneCtx`]: one plane's encode walk, built per tree by [`Encoder::encode`](crate::renderer::frontend::encoder::Encoder::encode).

use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::layout::plane_layout::PlaneLayout;
use crate::layout::text::text_runs::TextRuns;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::{FitRect, ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::encoder::cull_region::CullRegion;
use crate::renderer::frontend::encoder::gradient_resolver::GradientPass;
use crate::renderer::frontend::paint_sink::{DrawAnimation, PaintSink};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_effect::{
    ClosedEffect, EffectCaptureDraw,
};
use crate::renderer::frontend::payload::draw_closed_payload::closed_fill::ClosedFill;
use crate::renderer::frontend::payload::draw_closed_payload::closed_image::{
    ClosedImage, GpuViewDraw,
};
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_shadow_payload::DrawShadowPayload;
use crate::renderer::frontend::payload::draw_stroke_payload::{
    DrawStrokePayload, PolylinePath, StrokePath,
};
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::scene::tree::Tree;
use crate::scene::tree::backdrop_entry::BackdropOrigin;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_animations::PaintAnimationCursor;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::geometry::placed_stroke::PlacedStroke;
use crate::shape::geometry::stroke_geometry::{PolylineGeometry, StrokeGeometry};
use crate::shape::lower::LoweredPaint;
use crate::shape::paint::capture_params::EffectBackdrop;
use crate::shape::paint::closed_paint::ClosedPaint;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::lowered_stroke::LoweredStroke;
use crate::shape::record::{ShapeRecord, TextAnchor};
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

/// What makes a shape a material's: its material, and what it reads, the ticket the backdrop
/// ledger wrote for it or the ground.
#[derive(Clone, Copy, Debug)]
struct BackdropLook {
    material: LoweredMaterial,
    read: EffectCaptureDraw,
}

/// A stroke record as one paint call draws it: its path, its lowered stroke, and what its paint
/// animation does to it this frame.
#[derive(Clone, Copy, Debug)]
struct StrokeItem {
    geometry: StrokeGeometry,
    stroke: LoweredStroke,
    animation: DrawAnimation,
}

/// A closed shape's fill as one paint call draws it: the shape in the logical frame, its lowered
/// paint, where its backdrop is recorded, and what its paint animation does to it this frame.
#[derive(Clone, Copy, Debug)]
struct FillItem {
    geometry: PlacedClosed,
    paint: LoweredPaint,
    site: BackdropSite,
    animation: DrawAnimation,
}

/// Where a backdrop is recorded: by its owner, as its chrome or its shape at `shape_idx`.
#[derive(Clone, Copy, Debug)]
struct BackdropSite {
    owner: NodeId,
    origin: BackdropOrigin,
    shape_idx: u32,
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
        let read = match material.capture() {
            Some(capture) => EffectCaptureDraw::Capture {
                ticket: self.backdrop_ticket(site)?,
                reach: capture.reach,
            },
            None => EffectCaptureDraw::Ground,
        };
        Some(BackdropLook { material, read })
    }

    /// Paint a closed shape's fill, a node's chrome or a shape's: a box's drop shadow under its
    /// fill, its fill or its material, and its inset shadow over the fill inside its border, as
    /// CSS Backgrounds 3 §7.1 orders them. A material draws its fill and its border over the
    /// whole shape, in one draw.
    fn paint_fill(&mut self, item: FillItem, out: &mut impl PaintSink) {
        let FillItem {
            geometry,
            paint:
                LoweredPaint {
                    fill,
                    border,
                    shadow,
                    material,
                },
            site,
            animation,
        } = item;
        let fill = self.brush_source(fill);
        let backdrop = self.backdrop_look(site, material);
        // A drop shadow is cast by the box, under its fill; an inset one by its padding box, over
        // it.
        let inset = shadow.inset();
        let shadow = match geometry {
            PlacedClosed::Box { placed, .. } if inset => {
                let width = border.width;
                let padding_box = Rect {
                    min: Vec2::ZERO,
                    size: placed.rect.size,
                }
                .deflated(width);
                let source = PlacedBox {
                    rect: placed.rect.resolve_local(Some(padding_box)),
                    corners: placed
                        .corners
                        .inner(placed.rect.size, Spacing::saturating([width; 4])),
                };
                Some(DrawShadowPayload::new(source, shadow))
            }
            // The box's rect, as `compute_paint_rect` and `LoweredPaint::bbox_local` mirror.
            PlacedClosed::Box { placed, .. } => Some(DrawShadowPayload::new(placed, shadow)),
            PlacedClosed::Triangle(_) => {
                debug_assert!(shadow.is_noop(), "a triangle casts no shadow");
                None
            }
        };
        if let Some(shadow) = shadow.filter(|_| !inset) {
            out.draw(shadow, animation);
        }
        match backdrop {
            Some(look) => out.draw(
                DrawClosedPayload::new(
                    geometry,
                    ClosedEffect::material(look.material, look.read, fill, border),
                ),
                animation,
            ),
            None => out.draw(
                DrawClosedPayload::new(geometry, ClosedFill::new(fill, border)),
                animation,
            ),
        }
        if let Some(shadow) = shadow.filter(|_| inset) {
            out.draw(shadow, animation);
        }
    }

    /// Draw `item` in `owner_rect`, faded and turned by its animation. A polyline crosses with
    /// its staged points and colors, fitted to the owner. A cubic or an arc places in the owner: its
    /// curve on the curve pipeline, owner-local, the basis crossing verbatim so cull, spin and
    /// sub-instance sizing stay one code path; or its cap's dot as a box, in the stroke color times
    /// the ramp at its start.
    fn paint_stroke(&mut self, item: StrokeItem, owner_rect: Rect, out: &mut impl PaintSink) {
        let StrokeItem {
            geometry,
            stroke,
            animation,
        } = item;
        let ramp_row = match stroke.ramp {
            CurveRamp::None => None,
            CurveRamp::Interned { id, hash: _ } => Some(self.gradient_row(id)),
        };
        let payload = |path| DrawStrokePayload {
            path,
            origin: owner_rect.min,
            spin: None,
            fill: GpuFill::curve(stroke.color, ramp_row),
            width: stroke.width,
            cap: stroke.cap,
        };
        let place = match geometry {
            StrokeGeometry::Polyline(PolylineGeometry {
                points,
                colors,
                bbox,
                placement,
                content_hash: _,
            }) => {
                debug_assert!(ramp_row.is_none(), "a polyline draws no ramp");
                let fit = placement.map(bbox, owner_rect.size);
                out.draw(
                    payload(StrokePath::Polyline(PolylinePath {
                        bbox: fit.apply_rect(bbox),
                        fit,
                        points,
                        colors,
                        join: stroke.join,
                    })),
                    animation,
                );
                return;
            }
            StrokeGeometry::Bezier(geometry) => {
                geometry.place(owner_rect.size, stroke.cap, stroke.width)
            }
            StrokeGeometry::Arc(geometry) => {
                geometry.place(owner_rect.size, stroke.cap, stroke.width)
            }
        };
        match place {
            PlacedStroke::Curve(basis) => out.draw(payload(StrokePath::Curve(basis)), animation),
            PlacedStroke::Dot(PlacedBox { rect, corners }) => {
                let color = match stroke.ramp {
                    CurveRamp::None => stroke.color,
                    CurveRamp::Interned { id, hash: _ } => RgbaF32::from(stroke.color)
                        .tinted(self.gradients.ramp(id).color_at(0.0))
                        .to_f16(),
                };
                let dot = PlacedClosed::inside(PlacedBox {
                    rect: owner_rect.resolve_local(Some(rect)),
                    corners,
                });
                out.draw(
                    DrawClosedPayload::new(
                        dot,
                        ClosedFill::new(BrushSource::Solid(color), LoweredBorder::NONE),
                    ),
                    animation,
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
        // Early-out so a shape animated to nothing doesn't pay for its geometry.
        let paint_mod = self.paint_animation_cursor.sample(shape_idx, self.now);
        if is_invisible(paint_mod.alpha) {
            return;
        }
        let owner_rect = self.layout.rect[id.idx()];
        // The gate fades and turns every draw of the shape by it, about the owner's center.
        let animation = DrawAnimation {
            alpha: paint_mod.alpha,
            spin: Spin::about_owner(owner_rect, paint_mod.rotation),
        };
        match shape {
            ShapeRecord::Closed { geometry, paint } => {
                let Some(placed) = geometry.place(owner_rect) else {
                    return;
                };
                let site = BackdropSite {
                    owner: id,
                    origin: BackdropOrigin::Shape,
                    shape_idx,
                };
                match paint {
                    ClosedPaint::Fill(paint) => self.paint_fill(
                        FillItem {
                            geometry: placed,
                            paint: *paint,
                            site,
                            animation,
                        },
                        out,
                    ),
                    ClosedPaint::Image {
                        tint,
                        source,
                        fit,
                        min_filter,
                        mag_filter,
                        downsample,
                    } => {
                        // Either source carries its texture id; a `GpuView` also names its owner,
                        // whose callback the backend finds in the window's `GpuViews`, and the
                        // epoch telling the backend whether the target is current.
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
                        // The image fits the shape's covering rect.
                        let FitRect {
                            rect,
                            uv_min,
                            uv_size,
                        } = fit.resolve(placed.bounds(), source.intrinsic());
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
                            DrawClosedPayload::new(
                                placed,
                                ClosedImage {
                                    rect,
                                    uv_min,
                                    uv_size,
                                    tint: *tint,
                                    handle,
                                    flags,
                                    view,
                                },
                            ),
                            animation,
                        );
                    }
                    ClosedPaint::Icon {
                        handle,
                        fit,
                        tint,
                        desaturate,
                    } => out.draw(
                        DrawIconPayload {
                            rect: fit.place_artwork(placed.bounds(), handle.view_box()),
                            icon: handle.icon,
                            tint: *tint,
                            desaturate: *desaturate,
                            spin: None,
                        },
                        animation,
                    ),
                    // An effect fades, turns and reads its time with its paint animation. A
                    // backdrop effect the cascade lists not lies outside every clip.
                    ClosedPaint::Effect {
                        backdrop,
                        effect,
                        params,
                        images,
                        content_hash: _,
                    } => {
                        let backdrop = match backdrop {
                            Some(EffectBackdrop::Capture(capture)) => {
                                let Some(ticket) = self.backdrop_ticket(site) else {
                                    return;
                                };
                                Some(EffectCaptureDraw::Capture {
                                    ticket,
                                    reach: capture.reach,
                                })
                            }
                            Some(EffectBackdrop::Ground) => Some(EffectCaptureDraw::Ground),
                            None => None,
                        };
                        out.draw(
                            DrawClosedPayload::new(
                                placed,
                                ClosedEffect {
                                    effect: *effect,
                                    params: *params,
                                    images: *images,
                                    backdrop,
                                    stage: None,
                                    opacity: 1.0,
                                    time: paint_mod.time,
                                },
                            ),
                            animation,
                        );
                    }
                }
            }
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
                        spin: None,
                    },
                    animation,
                );
            }
            ShapeRecord::Stroke { geometry, stroke } => self.paint_stroke(
                StrokeItem {
                    geometry: *geometry,
                    stroke: *stroke,
                    animation,
                },
                owner_rect,
                out,
            ),
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
                        spin: None,
                    },
                    animation,
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
            let geometry = PlacedClosed::inside(PlacedBox {
                rect,
                corners: chrome.corners,
            });
            // Alpha `1.0`: chrome isn't a shape, so no paint animation.
            self.paint_fill(
                FillItem {
                    geometry,
                    paint: LoweredPaint {
                        fill: chrome.fill,
                        border: chrome.border,
                        shadow: chrome.shadow,
                        material: chrome.material,
                    },
                    site: BackdropSite {
                        owner: id,
                        origin: BackdropOrigin::Chrome,
                        shape_idx: self.tree.records.shape_span()[id.idx()].start,
                    },
                    animation: DrawAnimation::STILL,
                },
                out,
            );
            if chrome.ring {
                let ring = ClosedFill::new(
                    self.brush_source(LoweredBrush::Solid(RgbaF16::TRANSPARENT)),
                    self.tree.focus_ring,
                );
                out.draw(DrawClosedPayload::new(geometry, ring), 1.0);
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
