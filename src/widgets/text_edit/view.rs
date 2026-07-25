//! TextEdit layout, viewport state, and shape recording.

use crate::layout::types::align::{self, Align, HAlign};
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::scene::node::Node;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::shape::Shape;
use crate::text::probe::{CursorPos, SelectionRects};
use crate::text::wrap::{LineFit, TextWrap};
use crate::text::{FontFamily, FontWeight, TextShapeRequest, TextShaper};
use crate::ui::Ui;
use crate::widgets::Widget;
use crate::widgets::text_edit::model::EditState;
use glam::Vec2;
use std::ops::Range;
use std::time::Duration;

const BLINK_HALF: f32 = 0.5;
const BLINK_STOP_AFTER_IDLE: f32 = 30.0;

#[derive(Clone, Default, Debug)]
pub(super) struct ViewState {
    pub(super) selection_rects: Option<Box<SelectionRects>>,
    pub(super) prev_focused: bool,
    pub(super) scroll: Vec2,
    pub(super) block_offset: Vec2,
    pub(super) last_caret_change: Duration,
}

#[derive(Clone, Default, Debug)]
pub(super) struct InteractionState {
    pub(super) drag_anchor: Option<usize>,
}

impl InteractionState {
    pub(super) fn normalize(&mut self, text: &str) {
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

    pub(super) fn update(&mut self, input: ViewUpdateInput) -> ViewUpdate {
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
pub(super) struct ShapeCtx {
    pub(super) font_size: f32,
    pub(super) line_height_px: f32,
    pub(super) padding: Spacing,
    wrap_target: Option<f32>,
    pub(super) family: FontFamily,
    pub(super) weight: FontWeight,
    pub(super) multiline: bool,
    halign: HAlign,
    pub(super) block_offset: Vec2,
}

impl ShapeCtx {
    pub(super) fn request<'a>(&self, text: &'a str) -> TextShapeRequest<'a> {
        let request = TextShapeRequest::unbounded(
            text,
            self.font_size,
            self.line_height_px,
            self.family,
            self.weight,
        );
        match self.wrap_target {
            Some(width) => request.bounded(width, self.halign, LineFit::Wrap),
            None => request,
        }
    }
}

#[derive(Debug)]
pub(super) struct LayoutInput {
    pub(super) response_rect: Option<Rect>,
    pub(super) padding: Spacing,
    pub(super) caret_width: f32,
    pub(super) font_size: f32,
    pub(super) line_height_px: f32,
    pub(super) family: FontFamily,
    pub(super) weight: FontWeight,
    pub(super) multiline: bool,
    pub(super) text_align: Option<Align>,
    pub(super) previous_block_offset: Vec2,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ResolvedLayout {
    pub(super) ctx: ShapeCtx,
    pub(super) text_align: Align,
    pub(super) caret_room: f32,
    pub(super) content_width: f32,
    pub(super) display_width: f32,
    pub(super) placeholder_offset: Vec2,
    pub(super) inner_size: Size,
}

pub(super) fn resolve_layout(input: LayoutInput) -> ResolvedLayout {
    let caret_room = input.caret_width.max(0.0);
    // Raw inner width; `TextShapeKey::bounded` owns the canonical rounding.
    let wrap_target = if input.multiline {
        input
            .response_rect
            .map(|rect| rect.size.w - input.padding.horiz())
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
pub(super) struct GeometryInput<'a> {
    pub(super) layout: ResolvedLayout,
    pub(super) text: &'a str,
    pub(super) placeholder: &'a str,
    pub(super) caret: usize,
    pub(super) selection: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FinalGeometry {
    pub(super) layout: ResolvedLayout,
    pub(super) caret_pos: CursorPos,
    pub(super) text_hash: u64,
}

pub(super) fn resolve_geometry(
    shaper: &TextShaper,
    input: GeometryInput<'_>,
    selection_rects: &mut SelectionRects,
) -> FinalGeometry {
    let mut layout = input.layout;
    let probe = shaper.layout(layout.ctx.request(input.text));
    if let Some(selection) = input.selection {
        probe.selection_rects(selection, selection_rects);
    } else {
        selection_rects.clear();
    }
    let measured = probe.size;
    let caret_pos = probe.cursor_xy(input.caret);
    let text_hash = probe.request.key.text_hash;
    // The probe holds the shaper borrow; release it before the
    // placeholder lease below.
    drop(probe);
    let placeholder_measured = if input.text.is_empty() && !input.placeholder.is_empty() {
        shaper.layout(layout.ctx.request(input.placeholder)).size
    } else {
        measured
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
        align::align_in_rect(
            containing,
            Size::new(size.w, size.h.max(layout.ctx.line_height_px)),
            widget_align,
        )
        .min
    };
    layout.ctx.block_offset = aligned(measured);
    layout.placeholder_offset = aligned(placeholder_measured);
    layout.content_width = measured.w;
    layout.display_width = placeholder_measured.w;
    FinalGeometry {
        layout,
        caret_pos,
        text_hash,
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ViewUpdateInput {
    pub(super) response_rect: Option<Rect>,
    pub(super) ctx: ShapeCtx,
    pub(super) caret_pos: CursorPos,
    pub(super) caret_width: f32,
    pub(super) content_width: f32,
    pub(super) focused: bool,
    pub(super) caret_moved: bool,
    pub(super) edited: bool,
    pub(super) gained_focus: bool,
    pub(super) now: Duration,
    pub(super) block_offset: Vec2,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ViewUpdate {
    pub(super) scroll: Vec2,
    pub(super) caret_anim: Option<PaintAnim>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CaretPaint {
    pub(super) pos: CursorPos,
    pub(super) width: f32,
    pub(super) color: Color,
    pub(super) anim: Option<PaintAnim>,
}

#[derive(Debug)]
pub(super) struct PaintInput<'a> {
    pub(super) node: Node,
    pub(super) chrome: Background,
    pub(super) text: &'a str,
    pub(super) placeholder: &'a str,
    pub(super) layout: ResolvedLayout,
    pub(super) selection_rects: &'a SelectionRects,
    pub(super) selection_color: Color,
    pub(super) text_color: Color,
    pub(super) placeholder_color: Color,
    pub(super) scroll: Vec2,
    pub(super) caret: Option<CaretPaint>,
}

pub(super) fn record(ui: &mut Ui, mut widget: Widget, input: PaintInput<'_>) {
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
