//! TextEdit layout, viewport state, and shape recording.

use crate::layout::types::align::{Align, HAlign};
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::shape::Shape;
use crate::text::wrap::TextWrap;
use crate::text::{
    CursorPos, FontFamily, FontWeight, SELECTION_RECTS_INLINE_CAPACITY, SelectionRects,
    ShapeParams, TextShaper, text_in_rect,
};
use crate::ui::Ui;
use crate::widgets::Widget;
use crate::widgets::text_edit::TextEditState;
use crate::widgets::text_edit::model::EditState;
use glam::Vec2;
use std::ops::Range;
use std::time::Duration;

const BLINK_HALF: f32 = 0.5;
const BLINK_STOP_AFTER_IDLE: f32 = 30.0;

#[derive(Clone, Default, Debug)]
pub(crate) struct ViewState {
    pub(crate) selection_rects: Option<Box<SelectionRects>>,
    pub(crate) drag_anchor: Option<usize>,
    pub(crate) prev_focused: bool,
    pub(crate) scroll: Vec2,
    pub(crate) last_caret_change: Duration,
}

impl ViewState {
    pub(crate) fn normalize(&mut self, text: &str) {
        self.drag_anchor = self
            .drag_anchor
            .map(|offset| EditState::repair_offset(text, offset));
    }

    fn update_scroll(&mut self, input: ViewUpdateInput) {
        let Some(rect) = input.response_rect else {
            self.scroll = Vec2::ZERO;
            return;
        };
        let inner_w = (rect.size.w - input.ctx.padding.horiz()).max(0.0);
        let inner_h = (rect.size.h - input.ctx.padding.vert()).max(0.0);
        if input.ctx.multiline {
            self.scroll.x = 0.0;
            let trailing = (inner_h - input.caret_width).max(0.0);
            let caret_bottom = input.caret_pos.y_top + input.caret_pos.line_height;
            if input.caret_pos.y_top < self.scroll.y {
                self.scroll.y = input.caret_pos.y_top;
            } else if caret_bottom > self.scroll.y + trailing {
                self.scroll.y = caret_bottom - trailing;
            }
            self.scroll.y = self.scroll.y.max(0.0);
        } else {
            self.scroll.y = 0.0;
            let trailing = (inner_w - input.caret_width).max(0.0);
            let caret_right = input.caret_pos.x + input.caret_width;
            if input.caret_pos.x < self.scroll.x {
                self.scroll.x = input.caret_pos.x;
            } else if caret_right > self.scroll.x + trailing {
                self.scroll.x = caret_right - trailing;
            }
            let max_scroll = (input.content_width + 2.0 * input.caret_width - inner_w).max(0.0);
            self.scroll.x = self.scroll.x.clamp(0.0, max_scroll);
        }
    }

    pub(crate) fn update(&mut self, input: ViewUpdateInput) -> ViewUpdate {
        self.update_scroll(input);
        if input.focused && (input.caret_moved || input.edited || input.gained_focus) {
            self.last_caret_change = input.now;
        }
        self.prev_focused = input.focused;
        let caret_anim = if input.focused {
            let elapsed = input
                .now
                .saturating_sub(self.last_caret_change)
                .as_secs_f32();
            (elapsed < BLINK_STOP_AFTER_IDLE).then_some(PaintAnim::BlinkOpacity {
                half_period: Duration::from_secs_f32(BLINK_HALF),
                started_at: self.last_caret_change,
            })
        } else {
            None
        };
        ViewUpdate {
            scroll: self.scroll,
            caret_anim,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapeCtx {
    pub(crate) font_size: f32,
    pub(crate) line_height_px: f32,
    pub(crate) padding: Spacing,
    wrap_target: Option<f32>,
    pub(crate) family: FontFamily,
    pub(crate) weight: FontWeight,
    pub(crate) multiline: bool,
    halign: HAlign,
    pub(crate) block_offset: Vec2,
}

impl ShapeCtx {
    pub(crate) fn params(&self) -> ShapeParams {
        ShapeParams {
            font_size_px: self.font_size,
            line_height_px: self.line_height_px,
            max_width_px: self.wrap_target,
            family: self.family,
            weight: self.weight,
            halign: self.halign,
        }
    }
}

#[derive(Debug)]
pub(crate) struct LayoutInput<'a> {
    pub(crate) text: &'a str,
    pub(crate) placeholder: &'a str,
    pub(crate) focused: bool,
    pub(crate) response_rect: Option<Rect>,
    pub(crate) padding: Spacing,
    pub(crate) caret_width: f32,
    pub(crate) font_size: f32,
    pub(crate) line_height_px: f32,
    pub(crate) family: FontFamily,
    pub(crate) weight: FontWeight,
    pub(crate) multiline: bool,
    pub(crate) text_align: Option<Align>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedLayout {
    pub(crate) ctx: ShapeCtx,
    pub(crate) text_align: Align,
    pub(crate) caret_room: f32,
    pub(crate) content_width: f32,
}

pub(crate) fn resolve_layout(shaper: &TextShaper, input: LayoutInput<'_>) -> ResolvedLayout {
    let caret_room = input.caret_width.max(0.0);
    let wrap_target = if input.multiline {
        input
            .response_rect
            .map(|rect| (rect.size.w - input.padding.horiz() - caret_room).max(1.0))
    } else {
        None
    };
    let text_align = input.text_align.unwrap_or(if input.multiline {
        Align::TOP_LEFT
    } else {
        Align::LEFT
    });
    let mut ctx = ShapeCtx {
        font_size: input.font_size,
        line_height_px: input.line_height_px,
        padding: input.padding,
        wrap_target,
        family: input.family,
        weight: input.weight,
        multiline: input.multiline,
        halign: text_align.halign(),
        block_offset: Vec2::ZERO,
    };
    let widget_align = if input.multiline {
        Align::v(text_align.valign())
    } else {
        text_align
    };
    let mut content_width = 0.0;
    if let Some(rect) = input.response_rect {
        let measure_str = if !input.text.is_empty() || input.focused {
            input.text
        } else {
            input.placeholder
        };
        let measured = shaper
            .measure(measure_str, ctx.params())
            .expect("TextEdit metrics were validated")
            .size;
        content_width = measured.w;
        let measured = Size::new(measured.w, measured.h.max(input.line_height_px));
        let inner_w = (rect.size.w - input.padding.horiz() - caret_room).max(0.0);
        let inner_h = (rect.size.h - input.padding.vert()).max(0.0);
        ctx.block_offset = text_in_rect(
            Rect::new(0.0, 0.0, inner_w, inner_h),
            measured,
            widget_align,
        )
        .min;
    }
    ResolvedLayout {
        ctx,
        text_align,
        caret_room,
        content_width,
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewUpdateInput {
    pub(crate) response_rect: Option<Rect>,
    pub(crate) ctx: ShapeCtx,
    pub(crate) caret_pos: CursorPos,
    pub(crate) caret_width: f32,
    pub(crate) content_width: f32,
    pub(crate) focused: bool,
    pub(crate) caret_moved: bool,
    pub(crate) edited: bool,
    pub(crate) gained_focus: bool,
    pub(crate) now: Duration,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewUpdate {
    pub(crate) scroll: Vec2,
    pub(crate) caret_anim: Option<PaintAnim>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaretPaint {
    pub(crate) pos: CursorPos,
    pub(crate) width: f32,
    pub(crate) color: Color,
    pub(crate) anim: Option<PaintAnim>,
}

#[derive(Debug)]
pub(crate) struct PaintInput<'a> {
    pub(crate) node: Node,
    pub(crate) chrome: Background,
    pub(crate) text: &'a str,
    pub(crate) placeholder: &'a str,
    pub(crate) layout: ResolvedLayout,
    pub(crate) selection: Option<Range<usize>>,
    pub(crate) selection_color: Color,
    pub(crate) text_color: Color,
    pub(crate) placeholder_color: Color,
    pub(crate) scroll: Vec2,
    pub(crate) caret: Option<CaretPaint>,
}

pub(crate) fn record(ui: &mut Ui, mut widget: Widget, id: WidgetId, input: PaintInput<'_>) {
    let mut node = input.node;
    let ctx = input.layout.ctx;
    if !ctx.multiline {
        let min_size = node.min_size.get_or_insert(Size::ZERO);
        min_size.h = min_size.h.max(ctx.line_height_px + ctx.padding.vert());
        if node.size.unwrap_or_default().w().is_hug() {
            let measure_str = if input.text.is_empty() {
                input.placeholder
            } else {
                input.text
            };
            let text_width = ui
                .resources
                .text
                .measure(measure_str, ctx.params())
                .expect("TextEdit metrics were validated")
                .size
                .w;
            let reserved = text_width + ctx.padding.horiz() + 2.0 * input.layout.caret_room;
            let min_size = node.min_size.get_or_insert(Size::ZERO);
            min_size.w = min_size.w.max(reserved);
        }
    }

    let mut retained = ui
        .state_mut::<TextEditState>(id)
        .view
        .selection_rects
        .take();
    let mut inline = SelectionRects::new();
    let selection_rects = retained.as_deref_mut().unwrap_or(&mut inline);
    selection_rects.clear();
    if let Some(range) = input.selection {
        ui.resources
            .text
            .selection_rects(input.text, range, ctx.params(), selection_rects);
    }

    widget.node = node;
    widget.record(ui, Some(&input.chrome), |ui| {
        let [pad_l, pad_t, _, _] = ctx.padding.as_array();
        let origin = Vec2::new(
            pad_l + ctx.block_offset.x - input.scroll.x,
            pad_t + ctx.block_offset.y - input.scroll.y,
        );
        for rect in selection_rects.iter() {
            ui.add_shape(
                Shape::rect(Rect {
                    min: rect.min + origin,
                    size: rect.size,
                })
                .fill(input.selection_color),
            );
        }

        let (display, color) = if input.text.is_empty() {
            (ui.intern(input.placeholder), input.placeholder_color)
        } else {
            (ui.intern(input.text), input.text_color)
        };
        if !display.is_empty() {
            ui.add_shape(Shape::Text {
                local_origin: Some(origin),
                text: display,
                color,
                font_size_px: ctx.font_size,
                line_height_px: ctx.line_height_px,
                wrap: if ctx.multiline {
                    TextWrap::WrapWithOverflow
                } else {
                    TextWrap::Scroll
                },
                align: input.layout.text_align,
                family: ctx.family,
                weight: ctx.weight,
            });
        }

        if let Some(caret) = input.caret {
            let rect = Rect::new(
                origin.x + caret.pos.x,
                origin.y + caret.pos.y_top,
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

    if retained.is_none() && inline.len() > SELECTION_RECTS_INLINE_CAPACITY {
        retained = Some(Box::new(inline));
    }
    ui.state_mut::<TextEditState>(id).view.selection_rects = retained;
}
