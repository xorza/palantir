//! The editor's viewport: where the text block is scrolled to, and when
//! the caret blinks.

use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::widgets::scroll::state::{ScrollBounds, ScrollState};
use crate::widgets::text_edit::text_geometry::TextGeometry;
use glam::Vec2;
use std::time::Duration;

const BLINK_HALF: Duration = Duration::from_millis(500);
const BLINK_STOP_AFTER_IDLE: Duration = Duration::from_secs(30);

#[derive(Clone, Default, Debug)]
pub(super) struct ViewState {
    /// Focus as of the end of the previous pass. Written only by
    /// [`Self::roll_focus`], which is also the only thing that reads the
    /// edges out of it — a caller that wrote it by hand on one of the
    /// two return paths would report `gained_focus` again next frame.
    prev_focused: bool,
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
    /// Focus as of the previous pass, before this one can move it.
    ///
    /// The select-all-on-focus edge fires from the input pass, which runs
    /// before this frame's focus is final, so it reads the old value
    /// here rather than waiting for [`Self::roll_focus`].
    pub(super) fn was_focused(&self) -> bool {
        self.prev_focused
    }

    /// Roll onto `focused` and report the edges that crossing yields.
    ///
    /// One call because the read and the write are one act: the edge is
    /// only true for the frame that crossed it, which is exactly the
    /// frame the stored value changes.
    pub(super) fn roll_focus(&mut self, focused: bool) -> FocusEdges {
        let was = std::mem::replace(&mut self.prev_focused, focused);
        FocusEdges {
            gained: focused && !was,
            lost: was && !focused,
        }
    }

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
        let layout = input.geometry.layout;
        let Some(viewport) = layout.inner.map(|rect| rect.size) else {
            self.scroll = ScrollState::default();
            return;
        };
        let ctx = layout.ctx;
        let caret = input.geometry.caret_pos;
        let follow_caret =
            input.caret_byte != self.last_followed_caret || input.changed || input.gained_focus;
        self.last_followed_caret = input.caret_byte;
        let bounds = ScrollBounds {
            // A single line reserves room for the caret past its last
            // glyph, on both ends; a wrapped block has a next line to
            // fall to and reserves none.
            content: if ctx.multiline {
                Size::new(0.0, input.geometry.content_size.h)
            } else {
                Size::new(input.geometry.content_size.w + layout.caret_reserve(), 0.0)
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
                // X branch below reserves the caret's room because the
                // caret stands past the last glyph there — that same
                // *horizontal* thickness is slack on the wrong axis here,
                // and `bounds.content` reserves none vertically for
                // `clamp_to_natural` to honour it with.
                let trailing = viewport.h;
                let caret_bottom = caret.y_top + caret.line_height;
                if caret.y_top < offset.y {
                    offset.y = caret.y_top;
                } else if caret_bottom > offset.y + trailing {
                    offset.y = caret_bottom - trailing;
                }
            } else {
                let trailing = (viewport.w - layout.caret_room).max(0.0);
                let caret_right = caret.x + layout.caret_room;
                if caret.x < offset.x {
                    offset.x = caret.x;
                } else if caret_right > offset.x + trailing {
                    offset.x = caret_right - trailing;
                }
            }
        }
        self.scroll.clamp_to_natural(bounds);
    }

    /// Returns the caret's blink animation, if the field has focus.
    /// The new scroll offset is read straight off [`Self::scroll`]: the
    /// caller holds this `ViewState`, so handing a copy back would be a
    /// second answer to a question it can already ask.
    pub(super) fn update(&mut self, input: ViewUpdateInput) -> Option<PaintAnim> {
        self.update_scroll(input);
        if input.focused && (input.caret_moved || input.changed || input.gained_focus) {
            self.last_caret_change = input.now;
        }
        self.block_offset = input.geometry.block_offset;
        // The idle cutoff is the anim's to apply, not ours: a blinking
        // caret wakes the host on its own, and those wakes paint without
        // recording, so this line would stop running long before the
        // cutoff arrived.
        input.focused.then_some(PaintAnim::BlinkOpacity {
            half_period: BLINK_HALF,
            started_at: self.last_caret_change,
            stop_after: BLINK_STOP_AFTER_IDLE,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ViewUpdateInput {
    /// This pass's measured geometry: the box the text scrolls inside,
    /// the shaping parameters, the caret's room, what the run measured,
    /// where the caret landed, and where the block sits.
    ///
    /// Carried whole rather than unpacked into the four fields the view
    /// reads. The view reserves the caret's room through the same
    /// [`caret_reserve`] the field's own minimums use and deflates the
    /// viewport exactly once, and no caller can hand it a caret from one
    /// measurement beside a content size from another.
    ///
    /// [`caret_reserve`]: crate::widgets::text_edit::text_layout::TextLayout::caret_reserve
    pub(super) geometry: TextGeometry,
    /// This frame's wheel delta in logical px, already resolved from
    /// pixel + line sources. Sign matches the offset, so it adds.
    pub(super) wheel: Vec2,
    /// Caret byte after this frame's input, against which the view
    /// decides whether it owes a scroll-to-caret.
    pub(super) caret_byte: usize,
    pub(super) focused: bool,
    pub(super) caret_moved: bool,
    /// The buffer changed this pass, from any source — keys, paste, the
    /// context menu. Wider than the input pass's own `edited`, and named
    /// apart from it for that reason.
    pub(super) changed: bool,
    pub(super) gained_focus: bool,
    pub(super) now: Duration,
}

/// Which way focus crossed this pass — see [`ViewState::roll_focus`].
#[derive(Clone, Copy, Debug)]
pub(super) struct FocusEdges {
    pub(super) gained: bool,
    pub(super) lost: bool,
}
