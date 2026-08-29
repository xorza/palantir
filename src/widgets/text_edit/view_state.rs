//! The editor's viewport: where the text block is scrolled to, and when
//! the caret blinks.

use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::text::probe::Caret;
use crate::widgets::scroll::state::{ScrollBounds, ScrollState};
use crate::widgets::text_edit::shape_ctx::ShapeCtx;
use glam::Vec2;
use std::time::Duration;

const BLINK_HALF: Duration = Duration::from_millis(500);
const BLINK_STOP_AFTER_IDLE: Duration = Duration::from_secs(30);

#[derive(Clone, Default, Debug)]
pub(super) struct ViewState {
    pub(super) prev_focused: bool,
    /// Where the text block is scrolled to.
    ///
    /// The same [`ScrollState`] a [`Scroll`](crate::Scroll) viewport keeps,
    /// because a field *is* a scrolling viewport over its own text: the
    /// offset, the band it is clamped into, and the transform that carries
    /// the block are one implementation. What differs is only what moves
    /// it — a wheel and the caret here, a wheel and two bars there.
    pub(super) scroll: ScrollState,
    pub(super) block_offset: Vec2,
    pub(super) last_caret_change: Duration,
    /// Caret byte the view last scrolled to. Compared against the
    /// current one rather than using the pass's own `caret_moved`,
    /// because that only sees moves the widget itself made: a host that
    /// assigns `EditState::caret` between frames moves the caret without
    /// any edit or key, and the view still owes it a scroll.
    last_followed_caret: usize,
}

impl ViewState {
    /// Fold this frame's wheel delta into the offset, then keep the caret
    /// visible.
    ///
    /// **The field pans exactly one axis**: a multi-line editor wraps to
    /// its own width, and a single-line one has one line to slide along.
    /// The other axis is pinned by handing the solver no content on it,
    /// so the clamp does the pinning rather than an assignment beside it.
    ///
    /// The caret only pulls the view when it has *moved* since the view
    /// last followed it, or the buffer changed under it, or focus just
    /// arrived. Following it every frame would undo a wheel scroll the
    /// instant it happened, since the caret the user just scrolled away
    /// from is by definition off-screen. This is the ordinary editor
    /// bargain: the wheel roams freely, typing snaps back.
    fn update_scroll(&mut self, input: ViewUpdateInput) {
        let Some(viewport) = input.viewport else {
            self.scroll = ScrollState::default();
            return;
        };
        let ctx = input.ctx;
        let follow_caret =
            input.caret_byte != self.last_followed_caret || input.edited || input.gained_focus;
        self.last_followed_caret = input.caret_byte;
        let bounds = ScrollBounds {
            // A single line reserves room for the caret past its last
            // glyph, on both ends; a wrapped block has a next line to
            // fall to and reserves none.
            content: if ctx.multiline {
                Size::new(0.0, input.content_size.h)
            } else {
                Size::new(input.content_size.w + 2.0 * input.caret_width, 0.0)
            },
            viewport,
            content_margin: Spacing::ZERO,
        };
        // One line, so both wheel axes pan a single-line field
        // horizontally — a plain vertical wheel over one is the common
        // gesture, and there is nothing vertical to spend it on.
        let wheel = if ctx.multiline {
            Vec2::new(0.0, input.wheel.y)
        } else {
            Vec2::new(input.wheel.x + input.wheel.y, 0.0)
        };
        self.scroll
            .apply_wheel_pan(bounds, !ctx.multiline, ctx.multiline, wheel, false);
        if follow_caret {
            let offset = &mut self.scroll.offset;
            if ctx.multiline {
                // The whole viewport: a caret's vertical extent is its
                // line height, which `caret_bottom` already carries. The
                // X branch below reserves `caret_width` because the caret
                // stands past the last glyph there — that same
                // *horizontal* thickness is slack on the wrong axis here,
                // and `bounds.content` reserves none vertically for
                // `clamp_to_natural` to honour it with.
                let trailing = viewport.h;
                let caret_bottom = input.caret_pos.y_top + input.caret_pos.line_height;
                if input.caret_pos.y_top < offset.y {
                    offset.y = input.caret_pos.y_top;
                } else if caret_bottom > offset.y + trailing {
                    offset.y = caret_bottom - trailing;
                }
            } else {
                let trailing = (viewport.w - input.caret_width).max(0.0);
                let caret_right = input.caret_pos.x + input.caret_width;
                if input.caret_pos.x < offset.x {
                    offset.x = input.caret_pos.x;
                } else if caret_right > offset.x + trailing {
                    offset.x = caret_right - trailing;
                }
            }
        }
        self.scroll.clamp_to_natural(bounds);
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

#[derive(Clone, Copy, Debug)]
pub(super) struct ViewUpdateInput {
    /// The box the text scrolls inside — the field's rect less its
    /// padding, as [`TextLayout`](super::text_layout::TextLayout) already
    /// resolved it. `None` before the field has been arranged.
    ///
    /// Carried rather than re-derived from the rect and the padding: one
    /// deflation, one answer.
    pub(super) viewport: Option<Size>,
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
    pub(super) scroll: ScrollState,
    pub(super) caret_anim: Option<PaintAnim>,
}
