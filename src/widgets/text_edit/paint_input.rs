//! Everything the painter needs to record one editor's frame.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::text_edit::caret_paint::CaretPaint;
use crate::widgets::text_edit::text_geometry::TextGeometry;
use crate::widgets::text_edit::text_layout::TextLayout;
use glam::Vec2;

#[derive(Debug)]
pub(super) struct PaintInput<'a> {
    pub(super) chrome: Background,
    /// Identity of the text block's node, a child of the field so the layout engine places it; derived from the field's id, so stable across frames.
    pub(super) block_id: WidgetId,
    pub(super) text: &'a str,
    /// `Some(width)` while an input method composes: the wash rects cover the composition and paint as underlines `width` thick in the text colour.
    pub(super) preedit_underline: Option<f32>,
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
    /// Applies the measured minimums to `widget`, then records it; the widget arrives here to keep one copy.
    pub(super) fn record(self, ui: &mut Ui, mut widget: Widget) {
        let layout = self.geometry.layout;
        let ctx = layout.ctx;
        if !ctx.multiline {
            let mut min_size = widget.authored_min_size().unwrap_or(Size::ZERO);
            // The block's own height, not the theme's leading: a panned axis contributes no max-content, so this floor is the field's height and must match the shaper.
            let padding = ctx.padding.sums();
            min_size.h = min_size.h.max(self.block_size(layout).h + padding.h);
            if widget.authored_size().unwrap_or_default().w().is_hug() {
                let reserved = self.geometry.display_size.w + layout.caret_reserve() + padding.w;
                min_size.w = min_size.w.max(reserved);
            }
            widget.configure().min_size(min_size);
        }

        let block = self.block(layout);
        widget.record(ui, Some(&self.chrome), |ui| {
            block.record(ui, None, |ui| {
                for rect in self.selection_rects {
                    let shape = match self.preedit_underline {
                        None => Shape::rect(*rect).fill(self.selection_color),
                        Some(width) => Shape::rect(Rect::new(
                            rect.min.x,
                            rect.min.y + rect.size.h - width,
                            rect.size.w,
                            width,
                        ))
                        .fill(self.text_color),
                    };
                    ui.add_shape(shape);
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
                    // Block-local and unclamped: a clamp would hold the caret inside the widget's box, which is still a frame stale; the block carries the caret and the field's clip keeps it inside.
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

    /// The box the block occupies for this pass's one display measure; floors and caret room are [`TextLayout::block_size`]'s, so the node and the field's minimums agree.
    fn block_size(&self, layout: TextLayout) -> Size {
        layout.block_size(self.geometry.display_size)
    }

    /// The node the run, the wash and the caret are recorded against.
    ///
    /// **Where it sits inside the inner rect is the layout engine's**: an alignment needs the rect, which record time lacks, so a child that `arrange` places resolves it against this frame's rect and the field aligns the same on its first frame.
    ///
    /// Pinned to what the probe measured, since left to hug the block would take the minimum a scrolling run reports (nothing). Not held to the field's width: a panned axis reports no min-content, so a wider block scrolls the field.
    ///
    /// The caret's room is added, not deflated out of the aligned rect, so an end-of-line caret falls inside its block. Single-line only, as [`TextGeometry::resolve`](crate::widgets::text_edit::text_geometry::TextGeometry::resolve).
    ///
    /// The scroll rides as a transform ([`ScrollState::transform`], as a `Scroll` viewport does), keeping the three shapes in one frame of reference.
    fn block(&self, layout: TextLayout) -> Widget {
        let size = self.block_size(layout);
        Widget::leaf()
            .id(self.block_id)
            .size((Sizing::fixed(size.w), Sizing::fixed(size.h)))
            .align(layout.block_align())
            .transform(self.scroll.transform(Vec2::ZERO))
    }
}
