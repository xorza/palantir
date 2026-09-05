//! Everything the painter needs to record one editor's frame.

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::shape::Shape;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::text_edit::caret_paint::CaretPaint;
use crate::widgets::text_edit::text_geometry::TextGeometry;
use crate::widgets::text_edit::text_layout::TextLayout;
use crate::widgets::widget::Widget;
use glam::Vec2;

#[derive(Debug)]
pub(super) struct PaintInput<'a> {
    pub(super) chrome: Background,
    /// Identity of the node the text block is recorded on — a child of the
    /// field, so the layout engine is what places it. Derived from the
    /// field's own id, so it is as stable across frames as the field is.
    pub(super) block_id: WidgetId,
    pub(super) text: &'a str,
    pub(super) placeholder: &'a str,
    pub(super) geometry: TextGeometry,
    pub(super) selection_rects: &'a [Rect],
    pub(super) selection_color: RgbaF32,
    pub(super) text_color: RgbaF32,
    pub(super) placeholder_color: RgbaF32,
    pub(super) scroll: ScrollState,
    pub(super) caret: Option<CaretPaint>,
}

impl PaintInput<'_> {
    /// Applies the measured minimums to `widget`, then records it. The
    /// widget arrives here rather than in `PaintInput` so there is one
    /// copy of it, not two to keep in step.
    pub(super) fn record(self, ui: &mut Ui, mut widget: Widget) {
        let layout = self.geometry.layout;
        let ctx = layout.ctx;
        if !ctx.multiline {
            let mut min_size = widget.authored_min_size().unwrap_or(Size::ZERO);
            // The block's own height rather than the theme's leading, because a
            // panned axis contributes no max-content — so this floor is what the
            // field's height *is*, and a floor a thousandth under what the shaper
            // measured is a field a thousandth shorter than the chip it replaces.
            min_size.h = min_size
                .h
                .max(self.block_size(layout).h + ctx.padding.vert());
            if widget.authored_size().unwrap_or_default().w().is_hug() {
                let reserved =
                    self.geometry.display_size.w + layout.caret_reserve() + ctx.padding.horiz();
                min_size.w = min_size.w.max(reserved);
            }
            widget.configure().min_size(min_size);
        }

        let block = self.block(layout);
        widget.record(ui, Some(&self.chrome), |ui| {
            block.record(ui, None, |ui| {
                for rect in self.selection_rects {
                    ui.add_shape(Shape::rect(*rect).fill(self.selection_color));
                }

                let (display, color) = if self.text.is_empty() {
                    (ui.intern(self.placeholder), self.placeholder_color)
                } else {
                    (ui.intern(self.text), self.text_color)
                };
                if !display.is_empty() {
                    ui.add_shape(
                        Shape::text(display, ctx.font)
                            .at_origin(Vec2::ZERO)
                            .color(color)
                            .wrap(if ctx.multiline {
                                TextWrap::Wrap
                            } else {
                                TextWrap::Scroll
                            })
                            .align(layout.text_align),
                    );
                }

                if let Some(caret) = self.caret {
                    // Block-local, and unclamped: a clamp here would hold the caret
                    // inside the *widget's* box, which is the one thing here that is
                    // still a frame stale. The block carries the caret with it, and
                    // the field's own clip is what keeps it from painting outside.
                    let rect = Rect::new(
                        caret.pos.x,
                        caret.pos.y_top,
                        caret.width,
                        caret.pos.line_height,
                    );
                    let shape = Shape::rect(rect).fill(caret.color);
                    match caret.anim {
                        Some(anim) => ui.add_shape_animated(shape, anim),
                        None => ui.add_shape(shape),
                    }
                }
            });
        });
    }

    /// The box the block occupies, for the one display measure this pass
    /// has. The floors and the caret's room are
    /// [`TextLayout::block_size`]'s, so the node the engine places and the
    /// minimums the field reports cannot disagree about its extent.
    fn block_size(&self, layout: TextLayout) -> Size {
        layout.block_size(self.geometry.display_size)
    }

    /// The node the run, the wash and the caret are recorded against.
    ///
    /// **Where it sits inside the inner rect is the layout engine's**, and that is
    /// the whole point: an offset inside a rect is an alignment, and an alignment
    /// wants the rect — which record time does not have, arrange not having run. A
    /// child that `arrange` places resolves it against *this* frame's rect, so a
    /// field aligns the same on the frame it appears as on every frame after.
    ///
    /// Pinned to what the probe measured, because that is what has to be aligned
    /// and a field's text is shaped to scroll — left to hug, the block would take
    /// the *minimum* a scrolling run reports, which is nothing. Nothing holds it to
    /// the field's width: the field is a scrolling viewport, so a panned axis
    /// reports no min-content and a block wider than its field makes the field
    /// scroll rather than refuse to shrink.
    ///
    /// The caret's room is added rather than deflated out of the rect the block is
    /// aligned in. Same arithmetic, better placed: the caret at the end of a line
    /// falls *inside* the block it belongs to instead of just past it. Single-line
    /// only, matching [`TextGeometry::resolve`](crate::widgets::text_edit::text_geometry::TextGeometry::resolve) — a wrapped block reserves nothing,
    /// because its caret has a next line to fall to.
    ///
    /// The scroll rides as a transform rather than being folded into every shape's
    /// coordinates, so the three shapes stay in one frame of reference and a
    /// scrolled field is the same picture slid sideways — through the same
    /// [`ScrollState::transform`] a `Scroll` viewport carries its children with.
    fn block(&self, layout: TextLayout) -> Widget {
        let size = self.block_size(layout);
        Widget::leaf()
            .id(self.block_id)
            .size((Sizing::fixed(size.w), Sizing::fixed(size.h)))
            .align(layout.block_align())
            .transform(self.scroll.transform())
    }
}
