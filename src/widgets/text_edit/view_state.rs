//! The editor's viewport: where the text block is scrolled to, and when
//! the caret blinks.

use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::text::probe::Caret;
use crate::widgets::text_edit::shape_ctx::ShapeCtx;
use glam::Vec2;
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
