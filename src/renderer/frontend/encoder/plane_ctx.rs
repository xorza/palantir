//! [`PlaneCtx`]: one plane's encode walk, built per tree by [`Encoder::encode`](crate::renderer::frontend::encoder::Encoder::encode).

use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::pane_row::PaneRow;
use crate::layout::plane_layout::PlaneLayout;
use crate::layout::text::text_runs::TextRuns;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::{FitRect, ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::encoder::cull_region::CullRegion;
use crate::renderer::frontend::encoder::gradient_resolver::GradientPass;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::{
    DrawImagePayload, ImageDraw, ViewPaint,
};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_pane_payload::DrawPanePayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::{
    DrawQuadPayload, RectQuad, TriangleQuad,
};
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::gpu_paint::gpu_views::GpuViews;
use crate::renderer::pane_captures::PaneTicket;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_animations::PaintAnimationCursor;
use crate::scene::tree::pane_entry::PaneOrigin;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::record::{ShapeRecord, TextAnchor};
use crate::shape::record_store::recorded_gradients::GradientId;
use crate::shape::rect::RectKind;
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
    /// Live `GpuView`s by owner `WidgetId`, one map across layers; an `ImageSource::GpuView` carries only its epoch.
    pub(super) gpu_views: &'a GpuViews,
    pub(super) damage_filter: Option<CullRegion<'a>>,
    /// The plane's panes in paint order, and the ticket of each.
    pub(super) panes: &'a [PaneRow],
    pub(super) tickets: &'a [Option<PaneTicket>],
    /// The first of `panes` not yet painted: panes paint in their order.
    pub(super) next_pane: usize,
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

/// What makes a rect a pane: its backdrop filter, and the ticket the pane ledger wrote for it.
#[derive(Clone, Copy, Debug)]
struct PaneLook {
    backdrop: LoweredBackdrop,
    ticket: PaneTicket,
}

/// Where a pane is recorded: by its owner, as its chrome or its shape at `shape_idx`.
#[derive(Clone, Copy, Debug)]
struct PaneSite {
    owner: NodeId,
    origin: PaneOrigin,
    shape_idx: u32,
}

/// Paint `paint` as an ordinary rect.
fn paint_rect(paint: RectPaint, out: &mut impl PaintSink) {
    let RectPaint { quad, spin, alpha } = paint;
    out.draw(DrawQuadPayload::rect(quad).turned(spin), alpha);
}

/// Paint `paint` as a pane over the pixels painted before it, through `look`. A pane's tint is
/// solid: a gradient fill and the border over it draw as the rect above the pane.
fn paint_pane(paint: RectPaint, look: PaneLook, out: &mut impl PaintSink) {
    let RectPaint { quad, spin, alpha } = paint;
    let tint = match quad.fill {
        BrushSource::Solid(color) => Some(color),
        BrushSource::Gradient(_) => None,
    };
    out.draw(
        DrawPanePayload {
            kind: quad.kind,
            rect: quad.rect,
            corners: quad.corners,
            spin,
            tint: tint.unwrap_or(RgbaF16::TRANSPARENT),
            stroke: if tint.is_some() {
                quad.stroke.normalized()
            } else {
                ShapeStroke::NONE
            },
            backdrop: look.backdrop,
            opacity: 1.0,
            ticket: look.ticket,
        },
        alpha,
    );
    if tint.is_none() {
        paint_rect(paint, out);
    }
}

impl PlaneCtx<'_, '_> {
    /// What makes the rect recorded at `site` a pane: `backdrop` and its ticket; `None` for a
    /// rect of no backdrop, or a pane the cascade lists not.
    fn pane_look(&mut self, site: PaneSite, backdrop: LoweredBackdrop) -> Option<PaneLook> {
        if backdrop.is_none() {
            return None;
        }
        let ticket = self.pane_ticket(site)?;
        Some(PaneLook { backdrop, ticket })
    }

    /// The ticket of the pane recorded at `site`, as the tree lists it; `None` for a pane the
    /// cascade lists not, whose rect lies outside every clip.
    fn pane_ticket(&mut self, site: PaneSite) -> Option<PaneTicket> {
        let PaneSite {
            owner,
            origin,
            shape_idx,
        } = site;
        let entries = &self.tree.panes;
        let at = entries
            .binary_search_by_key(&(shape_idx, origin), |entry| {
                (entry.shape_idx, entry.origin)
            })
            .expect("a painted pane is listed");
        let key = PaneKey {
            owner: self.tree.records.widget_id()[owner.idx()],
            shape: entries[at].row,
        };
        let Some(ahead) = self.panes[self.next_pane..]
            .iter()
            .position(|pane| pane.key == key)
        else {
            debug_assert!(
                !self.panes[..self.next_pane]
                    .iter()
                    .any(|pane| pane.key == key),
                "panes paint in the cascade's order"
            );
            return None;
        };
        let at = self.next_pane + ahead;
        self.next_pane = at + 1;
        self.tickets[at]
    }

    #[inline]
    fn brush_source(&mut self, brush: ShapeBrush) -> BrushSource {
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
        // The turn for payloads placed in the logical frame; a payload with an origin of its own
        // takes `Spin::about_owner` with that origin.
        let spin = Spin::about_owner(owner_rect, Vec2::ZERO, paint_mod.rotation);
        match shape {
            ShapeRecord::Quad(shape) => match shape {
                QuadShape::Rect {
                    kind,
                    local_rect,
                    corners,
                    fill,
                    border,
                    backdrop,
                } => {
                    let rect = owner_rect.resolve_local(*local_rect);
                    let quad = RectQuad {
                        kind: *kind,
                        rect,
                        corners: *corners,
                        fill: self.brush_source(*fill),
                        stroke: *border,
                    };
                    let paint = RectPaint { quad, spin, alpha };
                    let site = PaneSite {
                        owner: id,
                        origin: PaneOrigin::Shape,
                        shape_idx,
                    };
                    match self.pane_look(site, *backdrop) {
                        Some(look) => paint_pane(paint, look, out),
                        None => paint_rect(paint, out),
                    }
                }
                QuadShape::Shadow {
                    local_rect,
                    corners,
                    shadow,
                } => {
                    // The composer grows a drop shadow from the source rect in physical pixels, so it lands on the same pixels as the fill.
                    let source = owner_rect.resolve_local(*local_rect);
                    if let Some(shadow) = DrawQuadPayload::shadow(source, *corners, *shadow) {
                        out.draw(shadow.turned(spin), alpha);
                    }
                }
                QuadShape::Triangle {
                    a,
                    b,
                    c,
                    radius,
                    fill,
                    border,
                    bbox: _,
                } => {
                    // Placed in the logical frame, as a rect is. Solid fill only: the reused quad lanes have no room for a gradient.
                    let triangle = DrawQuadPayload::triangle(TriangleQuad {
                        points: [*a, *b, *c].map(|q| owner_rect.min + q),
                        radius: *radius,
                        fill: *fill,
                        stroke: *border,
                    });
                    out.draw(triangle.turned(spin), alpha);
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
                content_hash: _,
            } => {
                out.draw(
                    DrawPolylinePayload {
                        bounds: StrokeBounds::new(owner_rect, *bbox, paint_mod.rotation),
                        origin: owner_rect.min,
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
                local_rect,
                tint,
                vertices,
                idxs,
                bbox,
                content_hash: _,
            } => {
                let origin = owner_rect.resolve_local(*local_rect).min;
                out.draw(
                    DrawMeshPayload {
                        bbox: *bbox,
                        origin,
                        tint: *tint,
                        vertices: *vertices,
                        idxs: *idxs,
                        spin: Spin::about_owner(owner_rect, origin, paint_mod.rotation),
                    },
                    alpha,
                );
            }
            ShapeRecord::Curve {
                cap,
                basis,
                stroke,
                bbox,
                ramp,
            } => {
                // Owner-local; the basis crosses verbatim so cull, spin and sub-instance sizing stay one code path.
                let ramp_row = match *ramp {
                    CurveRamp::None => None,
                    CurveRamp::Interned { id, hash: _ } => Some(self.gradient_row(id)),
                };
                out.draw(
                    DrawCurvePayload {
                        basis: *basis,
                        bounds: StrokeBounds::new(owner_rect, *bbox, paint_mod.rotation),
                        origin: owner_rect.min,
                        fill: GpuFill::curve(stroke.color, ramp_row),
                        width: stroke.width,
                        cap: *cap,
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
                        rect: fit.resolve(base, handle.view_box()),
                        icon: handle.icon,
                        tint: *tint,
                        desaturate: *desaturate,
                        spin,
                    },
                    alpha,
                );
            }
            ShapeRecord::Image {
                local_rect,
                tint,
                source,
                fit,
                min_filter,
                mag_filter,
                downsample,
            } => {
                let base = owner_rect.resolve_local(*local_rect);
                // A registered image carries its id; a `GpuView` is looked up in `Ui::gpu_views` and its paint callback rides with the payload, with the epoch telling the backend whether the target is current.
                let (handle, view) = match source {
                    ImageSource::Texture { id, .. } => (*id, None),
                    ImageSource::GpuView { epoch } => {
                        let widget_id = self.tree.records.widget_id()[id.idx()];
                        let view = self.gpu_views.view(widget_id);
                        let paint = ViewPaint {
                            paint: &view.paint,
                            epoch: *epoch,
                        };
                        (view.texture_id, Some(paint))
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
                    ImageDraw {
                        payload: DrawImagePayload {
                            rect,
                            uv_min,
                            uv_size,
                            tint: *tint,
                            handle,
                            flags,
                            spin,
                        },
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
            // Alpha `1.0`: chrome isn't a shape, so no paint animation. CSS Backgrounds 3 §7.1 order: drop shadow under the fill, inset over it.
            let fill = RectQuad {
                kind: RectKind::Rounded,
                rect,
                corners: chrome.corners,
                fill: self.brush_source(chrome.fill),
                stroke: chrome.border,
            };
            let site = PaneSite {
                owner: id,
                origin: PaneOrigin::Chrome,
                shape_idx: self.tree.records.shape_span()[id.idx()].start,
            };
            let pane = self.pane_look(site, chrome.backdrop);
            let paint = RectPaint {
                quad: fill,
                spin: None,
                alpha: 1.0,
            };
            let paint_fill = |out: &mut _| match pane {
                Some(look) => paint_pane(paint, look, out),
                None => paint_rect(paint, out),
            };
            if chrome.shadow.inset() {
                paint_fill(out);
                let width = chrome.border.width;
                let padding_box = Rect {
                    min: Vec2::ZERO,
                    size: rect.size,
                }
                .deflated(width);
                let source = rect.resolve_local(Some(padding_box));
                let corners = chrome
                    .corners
                    .inner(rect.size, Spacing::saturating([width; 4]));
                if let Some(shadow) = DrawQuadPayload::shadow(source, corners, chrome.shadow) {
                    out.draw(shadow, 1.0);
                }
            } else {
                // The owner's rect, as `compute_paint_rect` mirrors.
                if let Some(shadow) = DrawQuadPayload::shadow(rect, chrome.corners, chrome.shadow) {
                    out.draw(shadow, 1.0);
                }
                paint_fill(out);
            }
            if chrome.ring {
                let ring = DrawQuadPayload::rect(RectQuad {
                    kind: RectKind::Rounded,
                    rect,
                    corners: chrome.corners,
                    fill: self.brush_source(ShapeBrush::Solid(RgbaF16::TRANSPARENT)),
                    stroke: self.tree.focus_ring,
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
