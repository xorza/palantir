//! TextEdit layout, viewport state, and shape recording.

use crate::layout::types::align::{self, Align, HAlign};
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::shape::Shape;
use crate::text::layout_probe::Caret;
use crate::text::run::TextRun;
use crate::text::wrap::TextWrap;
use crate::text::{FontFamily, FontWeight};
use crate::ui::Ui;
use crate::widgets::text_edit::edit_state::EditState;
use crate::widgets::widget::Widget;
use glam::Vec2;
use std::ops::Range;
use std::time::Duration;

/// Retained buffer for the caret selection wash. Holds a selection up to
/// 16 visual lines inline; a longer one keeps its spill allocation, which
/// is what makes a held drag across a big multi-line selection
/// allocation-free after the first frame (`long_multiline_selection_alloc_free`).
///
/// Lives here rather than beside the probe that fills it: the probe
/// streams rects to a sink and retains nothing, so this is purely
/// `ViewState`'s storage choice.
pub(super) const SELECTION_RECTS_INLINE_CAPACITY: usize = 16;
pub(super) type SelectionRects = tinyvec::TinyVec<[Rect; SELECTION_RECTS_INLINE_CAPACITY]>;

const BLINK_HALF: Duration = Duration::from_millis(500);
const BLINK_STOP_AFTER_IDLE: Duration = Duration::from_secs(30);

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
        // The idle cutoff is the anim's to apply, not ours: a blinking
        // caret wakes the host on its own, and those wakes paint without
        // recording, so this line would stop running long before the
        // cutoff arrived.
        let caret_anim = input.focused.then_some(PaintAnim::BlinkOpacity {
            half_period: BLINK_HALF,
            started_at: self.last_caret_change,
            stop_after: BLINK_STOP_AFTER_IDLE,
        });
        ViewUpdate {
            scroll: self.scroll,
            caret_anim,
        }
    }
}

/// Everything the shaper needs to lay this editor's text out, plus the
/// padding that turns a shaped position into a widget-local one.
///
/// Deliberately carries no block offset: where the shaped block *sits*
/// isn't a shaping input, and holding both here is what let one field
/// mean last frame's offset before the probe and this frame's after it.
/// The two now live apart — [`TextLayout::prev_block_offset`] and
/// [`TextGeometry::block_offset`].
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
}

impl ShapeCtx {
    /// This editor's shaping parameters as the public run description.
    ///
    /// `TextEdit` probes through [`Ui::probe_text`](crate::Ui::probe_text)
    /// like any caller-authored widget would — which is what keeps that
    /// API honest about being enough to build a text widget with.
    ///
    /// A non-multiline editor carries no wrap target, so its `Wrap` /
    /// `SingleLine` choice and its `max_width_px` agree either way: both
    /// resolve to an unbounded shape.
    pub(super) fn run<'a>(&self, text: &'a str) -> TextRun<'a> {
        TextRun {
            text,
            font_size_px: self.font_size,
            line_height_px: self.line_height_px,
            wrap: if self.multiline {
                TextWrap::Wrap
            } else {
                TextWrap::SingleLine
            },
            align: Align::h(self.halign),
            family: self.family,
            weight: self.weight,
            max_width_px: self.wrap_target,
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

/// What is known **before** the shape probe runs: the box the text sits
/// in and the parameters it will be shaped with.
///
/// The input pass reads this — it hit-tests a click against the layout
/// the user was looking at, which is last frame's. Everything the probe
/// itself produces lands in [`TextGeometry`] instead, so neither type
/// ever holds a field that isn't answered yet.
#[derive(Clone, Copy, Debug)]
pub(super) struct TextLayout {
    pub(super) ctx: ShapeCtx,
    pub(super) text_align: Align,
    pub(super) caret_room: f32,
    pub(super) inner_size: Size,
    /// Where the shaped block sat when it was last painted. The click
    /// that arrives this frame was aimed at *that* layout, so the
    /// hit-test in `input` offsets by this rather than by the offset
    /// this frame's probe is about to produce.
    pub(super) prev_block_offset: Vec2,
}

pub(super) fn resolve_layout(input: LayoutInput) -> TextLayout {
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
    };
    let inner_size = input.response_rect.map_or(Size::ZERO, |rect| {
        Size::new(
            (rect.size.w - input.padding.horiz()).max(0.0),
            (rect.size.h - input.padding.vert()).max(0.0),
        )
    });
    TextLayout {
        ctx,
        text_align,
        caret_room,
        inner_size,
        prev_block_offset: input.previous_block_offset,
    }
}

/// What one probe of the content run yields. A named struct because the
/// closure has to hand all three back at once — the probe's borrow ends
/// with it, so nothing can be re-read afterwards.
#[derive(Clone, Copy, Debug)]
struct Probed {
    measured: Size,
    caret_pos: Caret,
    text_hash: u64,
}

#[derive(Debug)]
pub(super) struct GeometryInput<'a> {
    pub(super) layout: TextLayout,
    pub(super) text: &'a str,
    pub(super) placeholder: &'a str,
    pub(super) caret: usize,
    pub(super) selection: Option<Range<usize>>,
}

/// The layout plus everything only the shape probe could answer. Paint
/// reads this; nothing here exists before [`resolve_geometry`] runs,
/// which is why it is a separate type rather than zeroed fields on
/// [`TextLayout`].
#[derive(Clone, Copy, Debug)]
pub(super) struct TextGeometry {
    pub(super) layout: TextLayout,
    /// Where the shaped block sits inside the inner rect **this** frame.
    /// Stored back into `ViewState` at the end of the pass, which is
    /// what makes it next frame's [`TextLayout::prev_block_offset`].
    pub(super) block_offset: Vec2,
    pub(super) content_width: f32,
    pub(super) display_width: f32,
    pub(super) placeholder_offset: Vec2,
    pub(super) caret_pos: Caret,
    pub(super) text_hash: u64,
}

pub(super) fn resolve_geometry(
    ui: &mut Ui,
    input: GeometryInput<'_>,
    selection_rects: &mut SelectionRects,
) -> TextGeometry {
    let layout = input.layout;
    // The block is load-bearing: the content probe holds the shaper's
    // exclusive borrow, so the placeholder measurement below cannot be
    // taken until this one has dropped. Overlapping them is E0499, not a
    // runtime surprise.
    let Probed {
        measured,
        caret_pos,
        text_hash,
    } = {
        let probe = ui.probe_text(layout.ctx.run(input.text));
        selection_rects.clear();
        if let Some(selection) = input.selection {
            probe.selection_rects(selection, |rect| selection_rects.push(rect));
        }
        Probed {
            measured: probe.size(),
            caret_pos: probe.caret_at(input.caret),
            text_hash: probe.text_hash(),
        }
    };
    let placeholder_measured = if input.text.is_empty() && !input.placeholder.is_empty() {
        ui.probe_text(layout.ctx.run(input.placeholder)).size()
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
    TextGeometry {
        layout,
        block_offset: aligned(measured),
        content_width: measured.w,
        display_width: placeholder_measured.w,
        placeholder_offset: aligned(placeholder_measured),
        caret_pos,
        text_hash,
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ViewUpdateInput {
    pub(super) response_rect: Option<Rect>,
    pub(super) ctx: ShapeCtx,
    pub(super) caret_pos: Caret,
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
    pub(super) pos: Caret,
    pub(super) width: f32,
    pub(super) color: Color,
    pub(super) anim: Option<PaintAnim>,
}

#[derive(Debug)]
pub(super) struct PaintInput<'a> {
    pub(super) chrome: Background,
    pub(super) text: &'a str,
    pub(super) placeholder: &'a str,
    pub(super) geometry: TextGeometry,
    pub(super) selection_rects: &'a SelectionRects,
    pub(super) selection_color: Color,
    pub(super) text_color: Color,
    pub(super) placeholder_color: Color,
    pub(super) scroll: Vec2,
    pub(super) caret: Option<CaretPaint>,
}

/// Applies the measured minimums to `widget`'s staged node, then
/// records it. The node arrives on the widget rather than in
/// [`PaintInput`] so there is one copy of it, not two to keep in step.
pub(super) fn record(ui: &mut Ui, mut widget: Widget, input: PaintInput<'_>) {
    let layout = input.geometry.layout;
    let ctx = layout.ctx;
    if !ctx.multiline {
        let node = &mut widget.node;
        let min_size = node.min_size.get_or_insert(Size::ZERO);
        min_size.h = min_size.h.max(ctx.line_height_px + ctx.padding.vert());
        if node.size.unwrap_or_default().w().is_hug() {
            let reserved =
                input.geometry.display_width + ctx.padding.horiz() + 2.0 * layout.caret_room;
            let min_size = node.min_size.get_or_insert(Size::ZERO);
            min_size.w = min_size.w.max(reserved);
        }
    }

    widget.record(ui, Some(&input.chrome), |ui| {
        let [pad_l, pad_t, _, _] = ctx.padding.as_array();
        let block_offset = input.geometry.block_offset;
        let text_origin = Vec2::new(
            pad_l + block_offset.x - input.scroll.x,
            pad_t + block_offset.y - input.scroll.y,
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
                input.geometry.placeholder_offset
            } else {
                block_offset
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
                align: layout.text_align,
                family: ctx.family,
                weight: ctx.weight,
            });
        }

        if let Some(caret) = input.caret {
            let max_x = (pad_l + layout.inner_size.w - caret.width).max(pad_l);
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
