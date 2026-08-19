//! TextEdit layout, viewport state, and shape recording.

use crate::layout::types::align::{self, Align, HAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::shape::Shape;
use crate::text::glyph_font::GlyphFont;
use crate::text::probe::Caret;
use crate::text::run::TextRun;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::text_edit::edit_state::EditState;
use crate::widgets::widget::Widget;
use glam::Vec2;
use std::ops::Range;
use std::time::Duration;

const BLINK_HALF: Duration = Duration::from_millis(500);
const BLINK_STOP_AFTER_IDLE: Duration = Duration::from_secs(30);

#[derive(Clone, Default, Debug)]
pub(super) struct ViewState {
    pub(super) prev_focused: bool,
    pub(super) scroll: Vec2,
    pub(super) block_offset: Vec2,
    pub(super) last_caret_change: Duration,
    /// Caret byte the view last scrolled to. Compared against the
    /// current one rather than using the pass's own `caret_moved`,
    /// because that only sees moves the widget itself made: a host that
    /// assigns `EditState::caret` between frames moves the caret without
    /// any edit or key, and the view still owes it a scroll.
    last_followed_caret: usize,
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
    /// Fold this frame's wheel delta into the offset, then keep the caret
    /// visible.
    ///
    /// The caret only pulls the view when it has *moved* since the view
    /// last followed it, or the buffer changed under it, or focus just
    /// arrived. Following it every frame would undo a wheel scroll the
    /// instant it happened, since the caret the user just scrolled away
    /// from is by definition off-screen. This is the ordinary editor
    /// bargain: the wheel roams freely, typing snaps back.
    fn update_scroll(&mut self, input: ViewUpdateInput) {
        let Some(rect) = input.response_rect else {
            self.scroll = Vec2::ZERO;
            return;
        };
        let inner_w = (rect.size.w - input.ctx.padding.horiz()).max(0.0);
        let inner_h = (rect.size.h - input.ctx.padding.vert()).max(0.0);
        let follow_caret =
            input.caret_byte != self.last_followed_caret || input.edited || input.gained_focus;
        self.last_followed_caret = input.caret_byte;
        if input.ctx.multiline {
            self.scroll.x = 0.0;
            // A multi-line editor wraps to its own width, so only the
            // vertical wheel has anywhere to go.
            self.scroll.y += input.wheel.y;
            let trailing = (inner_h - input.caret_width).max(0.0);
            let caret_bottom = input.caret_pos.y_top + input.caret_pos.line_height;
            if follow_caret {
                if input.caret_pos.y_top < self.scroll.y {
                    self.scroll.y = input.caret_pos.y_top;
                } else if caret_bottom > self.scroll.y + trailing {
                    self.scroll.y = caret_bottom - trailing;
                }
            }
            let max_scroll = (input.content_size.h - inner_h).max(0.0);
            self.scroll.y = self.scroll.y.clamp(0.0, max_scroll);
        } else {
            self.scroll.y = 0.0;
            // One line, so both wheel axes pan it horizontally — a
            // plain vertical wheel over a single-line field is the
            // common gesture, and there is nothing vertical to spend it
            // on.
            self.scroll.x += input.wheel.x + input.wheel.y;
            let trailing = (inner_w - input.caret_width).max(0.0);
            let caret_right = input.caret_pos.x + input.caret_width;
            if follow_caret {
                if input.caret_pos.x < self.scroll.x {
                    self.scroll.x = input.caret_pos.x;
                } else if caret_right > self.scroll.x + trailing {
                    self.scroll.x = caret_right - trailing;
                }
            }
            let max_scroll = (input.content_size.w + 2.0 * input.caret_width - inner_w).max(0.0);
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
    pub(super) font: GlyphFont,
    pub(super) padding: Spacing,
    wrap_target: Option<f32>,
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
            font: self.font,
            wrap: if self.multiline {
                TextWrap::Wrap
            } else {
                TextWrap::SingleLine
            },
            align: Align::h(self.halign),
            max_width_px: self.wrap_target,
        }
    }
}

#[derive(Debug)]
pub(super) struct LayoutInput {
    pub(super) response_rect: Option<Rect>,
    pub(super) padding: Spacing,
    pub(super) caret_width: f32,
    pub(super) font: GlyphFont,
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
    // Raw inner width; `WrapBound::new` owns the canonical rounding.
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
        font: input.font,
        padding: input.padding,
        wrap_target,
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
    /// Where the shaped block sits inside the inner rect, as the *record*
    /// pass can work it out — from last pass's rect, since arrange has not
    /// run.
    ///
    /// **Read by the hit-test and by nothing else.** Painting stopped needing
    /// it when the block became a child the engine places: what a click has to
    /// undo is where the text was when the user aimed at it, which is last
    /// frame's, so a value one frame behind is the right one here and the wrong
    /// one there. Stored back into `ViewState` at the end of the pass, which is
    /// what makes it next frame's [`TextLayout::prev_block_offset`].
    pub(super) block_offset: Vec2,
    /// What the run measured, and what the placeholder measured. Both axes:
    /// the width alone drives horizontal scroll and the hug reservation, and
    /// the block node the shapes hang on wants the height too — see [`record`].
    pub(super) content_size: Size,
    pub(super) display_size: Size,
    pub(super) caret_pos: Caret,
    pub(super) text_hash: u64,
}

/// Measure the run and fill `selection_rects` with the wash for
/// `input.selection` — an out-parameter so the caller's retained buffer is
/// refilled in place instead of a fresh one being handed back each frame.
pub(super) fn resolve_geometry(
    ui: &mut Ui,
    input: GeometryInput<'_>,
    selection_rects: &mut Vec<Rect>,
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
            Size::new(size.w, size.h.max(layout.ctx.font.line_height_px)),
            widget_align,
        )
        .min
    };
    TextGeometry {
        layout,
        block_offset: aligned(measured),
        content_size: measured,
        display_size: placeholder_measured,
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
    /// Both axes: the width drives single-line scroll and the hug
    /// reservation, the height bounds the multi-line wheel.
    pub(super) content_size: Size,
    /// This frame's wheel delta in logical px, already resolved from
    /// pixel + line sources. Sign matches the offset, so it adds.
    pub(super) wheel: Vec2,
    /// Caret byte after this frame's input, against which the view
    /// decides whether it owes a scroll-to-caret.
    pub(super) caret_byte: usize,
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
    /// Identity of the node the text block is recorded on — a child of the
    /// field, so the layout engine is what places it. Derived from the
    /// field's own id, so it is as stable across frames as the field is.
    pub(super) block_id: WidgetId,
    pub(super) text: &'a str,
    pub(super) placeholder: &'a str,
    pub(super) geometry: TextGeometry,
    pub(super) selection_rects: &'a [Rect],
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
        // The block's own height rather than the theme's leading, because a
        // panned axis contributes no max-content — so this floor is what the
        // field's height *is*, and a floor a thousandth under what the shaper
        // measured is a field a thousandth shorter than the chip it replaces.
        min_size.h = min_size
            .h
            .max(block_height(&input, ctx) + ctx.padding.vert());
        if node.size.unwrap_or_default().w().is_hug() {
            let reserved =
                input.geometry.display_size.w + ctx.padding.horiz() + 2.0 * layout.caret_room;
            let min_size = node.min_size.get_or_insert(Size::ZERO);
            min_size.w = min_size.w.max(reserved);
        }
    }

    let block = Widget::new(input.block_id, block_node(&input, ctx, layout));
    widget.record(ui, Some(&input.chrome), |ui| {
        block.record(ui, None, |ui| {
            for rect in input.selection_rects {
                ui.add_shape(Shape::rect(*rect).fill(input.selection_color));
            }

            let (display, color) = if input.text.is_empty() {
                (ui.intern(input.placeholder), input.placeholder_color)
            } else {
                (ui.intern(input.text), input.text_color)
            };
            if !display.is_empty() {
                ui.add_shape(
                    Shape::text(display, ctx.font)
                        .at(Vec2::ZERO)
                        .color(color)
                        .wrap(if ctx.multiline {
                            TextWrap::Wrap
                        } else {
                            TextWrap::Scroll
                        })
                        .align(layout.text_align),
                );
            }

            if let Some(caret) = input.caret {
                // Block-local, and unclamped: what the clamp used to hold the caret
                // inside was the *widget's* box, which is the one thing here that is
                // still a frame stale. The block carries the caret with it now, and
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

/// What the block has to be sized to: the run's measurement, or the
/// placeholder's when that is what is on show, since it is what has to be
/// aligned.
fn measured(input: &PaintInput<'_>) -> Size {
    if input.text.is_empty() {
        input.geometry.display_size
    } else {
        input.geometry.content_size
    }
}

/// How tall the block is: what the shaper measured, floored at one line so an
/// empty field still has a caret's worth of height to stand up in.
///
/// The shaper's answer rather than the theme's leading, because the two differ
/// in the last thousandth of a pixel — the shaped one is quantized to 1/64 px —
/// and the field's box has to agree with the run inside it.
fn block_height(input: &PaintInput<'_>, ctx: ShapeCtx) -> f32 {
    measured(input).h.max(ctx.font.line_height_px)
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
/// only, matching [`resolve_geometry`] — a wrapped block reserves nothing,
/// because its caret has a next line to fall to.
///
/// The scroll rides as a transform rather than being folded into every shape's
/// coordinates, so the three shapes stay in one frame of reference and a
/// scrolled field is the same picture slid sideways.
fn block_node(input: &PaintInput<'_>, ctx: ShapeCtx, layout: TextLayout) -> Node {
    let room = if ctx.multiline {
        0.0
    } else {
        layout.caret_room
    };
    let mut block = Node::leaf();
    block.size = Some(
        (
            Sizing::fixed(measured(input).w + room),
            Sizing::fixed(block_height(input, ctx)),
        )
            .into(),
    );
    // Only the axes the field aligns on: a multi-line field aligns its block
    // vertically and lets the shaper align each line inside it, which is what
    // `resolve_geometry` says the same way.
    block.align = if ctx.multiline {
        Align::v(layout.text_align.valign())
    } else {
        layout.text_align
    };
    block.transform = TranslateScale::from_translation(-input.scroll);
    block
}
