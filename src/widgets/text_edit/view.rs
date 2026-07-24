//! TextEdit layout, viewport state, and shape recording.

use crate::layout::types::align::{Align, HAlign};
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::scene::node::Node;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::shape::Shape;
use crate::text::wrap::{TextWrap, canonical_wrap_width};
use crate::text::{
    CursorPos, FontFamily, FontWeight, LineFit, SelectionRects, TextMeasurement, TextShapeRequest,
    TextShaper, text_in_rect,
};
use crate::ui::Ui;
use crate::widgets::Widget;
use crate::widgets::text_edit::model::EditState;
use glam::Vec2;
use std::ops::Range;
use std::time::Duration;

const BLINK_HALF: f32 = 0.5;
const BLINK_STOP_AFTER_IDLE: f32 = 30.0;

#[derive(Clone, Default, Debug)]
pub(crate) struct ViewState {
    pub(crate) selection_rects: Option<Box<SelectionRects>>,
    pub(crate) prev_focused: bool,
    pub(crate) scroll: Vec2,
    pub(crate) block_offset: Vec2,
    pub(crate) last_caret_change: Duration,
}

#[derive(Clone, Default, Debug)]
pub(crate) struct InteractionState {
    pub(crate) drag_anchor: Option<usize>,
}

impl InteractionState {
    pub(crate) fn normalize(&mut self, text: &str) {
        self.drag_anchor = self
            .drag_anchor
            .map(|offset| EditState::repair_offset(text, offset));
    }
}

impl ViewState {
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
        self.block_offset = input.block_offset;
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
    pub(crate) fn request<'a>(&self, text: &'a str) -> TextShapeRequest<'a> {
        let request = TextShapeRequest::unbounded(
            text,
            self.font_size,
            self.line_height_px,
            self.family,
            self.weight,
        )
        .expect("TextEdit metrics were validated");
        match self.wrap_target {
            Some(width) => request
                .bounded(width, self.halign, LineFit::Wrap)
                .expect("TextEdit wrap target was validated"),
            None => request,
        }
    }
}

#[derive(Debug)]
pub(crate) struct LayoutInput {
    pub(crate) response_rect: Option<Rect>,
    pub(crate) padding: Spacing,
    pub(crate) caret_width: f32,
    pub(crate) font_size: f32,
    pub(crate) line_height_px: f32,
    pub(crate) family: FontFamily,
    pub(crate) weight: FontWeight,
    pub(crate) multiline: bool,
    pub(crate) text_align: Option<Align>,
    pub(crate) previous_block_offset: Vec2,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedLayout {
    pub(crate) ctx: ShapeCtx,
    pub(crate) text_align: Align,
    pub(crate) caret_room: f32,
    pub(crate) content_width: f32,
    pub(crate) display_width: f32,
    pub(crate) placeholder_offset: Vec2,
    pub(crate) inner_size: Size,
}

pub(crate) fn resolve_layout(input: LayoutInput) -> ResolvedLayout {
    let caret_room = input.caret_width.max(0.0);
    let wrap_target = if input.multiline {
        input
            .response_rect
            .map(|rect| canonical_wrap_width(rect.size.w - input.padding.horiz()))
    } else {
        None
    };
    let text_align = input.text_align.unwrap_or(if input.multiline {
        Align::TOP_LEFT
    } else {
        Align::LEFT
    });
    let ctx = ShapeCtx {
        font_size: input.font_size,
        line_height_px: input.line_height_px,
        padding: input.padding,
        wrap_target,
        family: input.family,
        weight: input.weight,
        multiline: input.multiline,
        halign: text_align.halign(),
        block_offset: input.previous_block_offset,
    };
    let inner_size = input.response_rect.map_or(Size::ZERO, |rect| {
        Size::new(
            (rect.size.w - input.padding.horiz()).max(0.0),
            (rect.size.h - input.padding.vert()).max(0.0),
        )
    });
    ResolvedLayout {
        ctx,
        text_align,
        caret_room,
        content_width: 0.0,
        display_width: 0.0,
        placeholder_offset: Vec2::ZERO,
        inner_size,
    }
}

#[derive(Debug)]
pub(crate) struct GeometryInput<'a> {
    pub(crate) layout: ResolvedLayout,
    pub(crate) text: &'a str,
    pub(crate) placeholder: &'a str,
    pub(crate) caret: usize,
    pub(crate) selection: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FinalGeometry {
    pub(crate) layout: ResolvedLayout,
    pub(crate) caret_pos: CursorPos,
    pub(crate) text_hash: u64,
}

#[derive(Clone, Copy, Debug)]
struct GeometryProbe {
    measurement: TextMeasurement,
    caret_pos: CursorPos,
    text_hash: u64,
}

pub(crate) fn resolve_geometry(
    shaper: &TextShaper,
    input: GeometryInput<'_>,
    selection_rects: &mut SelectionRects,
) -> FinalGeometry {
    let mut layout = input.layout;
    let probe = shaper.with_layout(layout.ctx.request(input.text), |probe| {
        if let Some(selection) = input.selection {
            probe.selection_rects(selection, selection_rects);
        } else {
            selection_rects.clear();
        }
        GeometryProbe {
            measurement: probe.measurement,
            caret_pos: probe.cursor_xy(input.caret),
            text_hash: probe.request.key.text_hash,
        }
    });
    let measurement = probe.measurement;
    let placeholder_measurement = if input.text.is_empty() && !input.placeholder.is_empty() {
        shaper.with_layout(layout.ctx.request(input.placeholder), |probe| {
            probe.measurement
        })
    } else {
        measurement
    };
    let widget_align = if layout.ctx.multiline {
        Align::v(layout.text_align.valign())
    } else {
        layout.text_align
    };
    let align_size = Size::new(
        if layout.ctx.multiline {
            layout.inner_size.w
        } else {
            (layout.inner_size.w - layout.caret_room).max(0.0)
        },
        layout.inner_size.h,
    );
    let containing = Rect {
        min: Vec2::ZERO,
        size: align_size,
    };
    let aligned = |size: Size| {
        text_in_rect(
            containing,
            Size::new(size.w, size.h.max(layout.ctx.line_height_px)),
            widget_align,
        )
        .min
    };
    layout.ctx.block_offset = aligned(measurement.size);
    layout.placeholder_offset = aligned(placeholder_measurement.size);
    layout.content_width = measurement.size.w;
    layout.display_width = placeholder_measurement.size.w;
    FinalGeometry {
        layout,
        caret_pos: probe.caret_pos,
        text_hash: probe.text_hash,
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
    pub(crate) block_offset: Vec2,
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
    pub(crate) selection_rects: &'a SelectionRects,
    pub(crate) selection_color: Color,
    pub(crate) text_color: Color,
    pub(crate) placeholder_color: Color,
    pub(crate) scroll: Vec2,
    pub(crate) caret: Option<CaretPaint>,
}

pub(crate) fn record(ui: &mut Ui, mut widget: Widget, input: PaintInput<'_>) {
    let mut node = input.node;
    let ctx = input.layout.ctx;
    if !ctx.multiline {
        let min_size = node.min_size.get_or_insert(Size::ZERO);
        min_size.h = min_size.h.max(ctx.line_height_px + ctx.padding.vert());
        if node.size.unwrap_or_default().w().is_hug() {
            let reserved =
                input.layout.display_width + ctx.padding.horiz() + 2.0 * input.layout.caret_room;
            let min_size = node.min_size.get_or_insert(Size::ZERO);
            min_size.w = min_size.w.max(reserved);
        }
    }

    widget.node = node;
    widget.record(ui, Some(&input.chrome), |ui| {
        let [pad_l, pad_t, _, _] = ctx.padding.as_array();
        let text_origin = Vec2::new(
            pad_l + ctx.block_offset.x - input.scroll.x,
            pad_t + ctx.block_offset.y - input.scroll.y,
        );
        for rect in input.selection_rects {
            ui.add_shape(
                Shape::rect(Rect {
                    min: rect.min + text_origin,
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
            let display_offset = if input.text.is_empty() {
                input.layout.placeholder_offset
            } else {
                ctx.block_offset
            };
            let display_origin = Vec2::new(
                pad_l + display_offset.x - input.scroll.x,
                pad_t + display_offset.y - input.scroll.y,
            );
            ui.add_shape(Shape::Text {
                local_origin: Some(display_origin),
                text: display,
                color,
                font_size_px: ctx.font_size,
                line_height_px: ctx.line_height_px,
                wrap: if ctx.multiline {
                    TextWrap::Wrap
                } else {
                    TextWrap::Scroll
                },
                align: input.layout.text_align,
                family: ctx.family,
                weight: ctx.weight,
            });
        }

        if let Some(caret) = input.caret {
            let max_x = (pad_l + input.layout.inner_size.w - caret.width).max(pad_l);
            let rect = Rect::new(
                (text_origin.x + caret.pos.x).clamp(pad_l, max_x),
                text_origin.y + caret.pos.y_top,
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
}
