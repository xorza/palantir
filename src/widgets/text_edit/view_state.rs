//! The editor's viewport: where the text block is scrolled to, and when the caret
//! blinks.

use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::scene::tree::paint_anims::curves;
use crate::scene::tree::paint_anims::paint_animation::PaintAnimation;
use crate::scene::tree::paint_anims::paint_animation::PaintRepeat;
use crate::widgets::scroll::state::{ScrollBounds, ScrollState};
use crate::widgets::text_edit::text_geometry::TextGeometry;
use glam::Vec2;
use std::mem;
use std::time::Duration;

const BLINK_HALF: Duration = Duration::from_millis(500);
const BLINK_STOP_AFTER_IDLE: Duration = Duration::from_secs(30);

#[derive(Clone, Default, Debug)]
pub(super) struct ViewState {
    /// Focus as of the end of the previous pass, written only by
    /// [`Self::roll_focus`], which also reads the edges out of it (a hand-written
    /// update on one return path would report `focus_gained` again).
    prev_focused: bool,
    /// Where the text block is scrolled to: the same [`ScrollState`] a
    /// [`Scroll`](crate::Scroll) viewport keeps, since a field is a scrolling
    /// viewport over its own text.
    pub(super) scroll: ScrollState,
    /// The wheel axes [`Self::scroll`] could pan as of the last update, so a wheel
    /// the field cannot spend reaches the container behind it.
    pub(super) wheel_axes: Sense,
    pub(super) block_offset: Vec2,
    pub(super) last_caret_change: Duration,
    /// Caret byte the view last scrolled to. Compared against the current one, not
    /// the pass's `caret_moved`, which misses a caller shortening the bound
    /// `String` (the caret moves with no edit or key, and the view still owes a
    /// scroll).
    last_followed_caret: usize,
}

impl ViewState {
    /// Focus as of the previous pass: the select-all-on-focus edge fires from the
    /// input pass, before this frame's focus is final.
    pub(super) const fn was_focused(&self) -> bool {
        self.prev_focused
    }

    /// Roll onto `focused` and report the edges crossed; an edge is true only for
    /// the frame the stored value changes.
    pub(super) const fn roll_focus(&mut self, focused: bool) -> FocusEdges {
        let was = mem::replace(&mut self.prev_focused, focused);
        FocusEdges {
            gained: focused && !was,
            lost: was && !focused,
        }
    }

    /// Fold this frame's wheel delta into the offset, then keep the caret visible.
    /// **The field pans exactly one axis** (multi-line wraps to its width,
    /// single-line slides along its line); the other is pinned by handing the
    /// solver no content on it. The caret pulls the view only when it moved, the
    /// buffer changed, or focus just arrived: following it every frame would undo a
    /// wheel scroll instantly.
    fn update_scroll(&mut self, input: ViewUpdateInput) {
        let layout = input.geometry.layout;
        let ctx = layout.ctx;
        let Some(viewport) = layout.inner.map(|rect| rect.size) else {
            self.scroll = ScrollState::default();
            // No box yet, so no overflow to read: claim the axis the field pans.
            self.wheel_axes = if ctx.multiline {
                Sense::SCROLL_Y
            } else {
                Sense::SCROLL_X
            };
            return;
        };
        let caret = input.geometry.caret_pos;
        let follow_caret =
            input.caret_byte != self.last_followed_caret || input.changed || input.focus_gained;
        self.last_followed_caret = input.caret_byte;
        let bounds = ScrollBounds {
            // A single line reserves room for the caret past its last glyph on both
            // ends; a wrapped block has a next line to fall to.
            content: if ctx.multiline {
                Size::new(0.0, input.geometry.content_size.h)
            } else {
                Size::new(input.geometry.content_size.w + layout.caret_reserve(), 0.0)
            },
            viewport,
            content_margin: Spacing::ZERO,
        };
        // A plain vertical wheel over a single-line field arrives on x already:
        // routing moves it there while nothing under the pointer pans vertically.
        self.scroll
            .apply_wheel_pan(bounds, !ctx.multiline, ctx.multiline, input.wheel, false);
        if follow_caret {
            let offset = &mut self.scroll.offset;
            if ctx.multiline {
                // The whole viewport: a caret's vertical extent is its line height,
                // which `caret_bottom` carries. The X branch reserves caret room
                // because the caret stands past the last glyph there; that
                // thickness is slack on the wrong axis here.
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
        self.wheel_axes = self
            .scroll
            .wheel_sense(bounds, !ctx.multiline, ctx.multiline);
    }

    /// Returns the caret's blink animation, if the field has focus. The new scroll
    /// offset is read off [`Self::scroll`].
    pub(super) fn update(&mut self, input: ViewUpdateInput) -> Option<PaintAnimation> {
        self.update_scroll(input);
        if input.focused && (input.caret_moved || input.changed || input.focus_gained) {
            self.last_caret_change = input.now;
        }
        self.block_offset = input.geometry.block_offset;
        // The idle cutoff is the anim's to apply: a blinking caret wakes the host
        // on its own and those wakes paint without recording, so this line would
        // stop running long before it.
        input.focused.then_some(
            PaintAnimation::alpha(0.0, 1.0)
                .with_started_at(self.last_caret_change)
                .with_period(BLINK_HALF * 2)
                .with_steps(2)
                .with_repeat(PaintRepeat::Settle(BLINK_STOP_AFTER_IDLE))
                .with_curve(curves::square),
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ViewUpdateInput {
    /// This pass's measured geometry: the box the text scrolls inside, shaping
    /// parameters, the caret's room, the run's measure, caret and block placement.
    /// Carried whole so the view reserves caret room through the same
    /// [`caret_reserve`] the field's minimums use, and no caller can pair a caret
    /// from one measurement with a content size from another.
    ///
    /// [`caret_reserve`]:
    /// crate::widgets::text_edit::text_layout::TextLayout::caret_reserve
    pub(super) geometry: TextGeometry,
    /// This frame's wheel delta in logical px, resolved from pixel + line sources;
    /// sign matches the offset.
    pub(super) wheel: Vec2,
    /// Caret byte after this frame's input, against which the view decides whether
    /// it owes a scroll-to-caret.
    pub(super) caret_byte: usize,
    pub(super) focused: bool,
    pub(super) caret_moved: bool,
    /// The buffer changed this pass from any source (keys, paste, context menu);
    /// wider than the input pass's `edited`.
    pub(super) changed: bool,
    pub(super) focus_gained: bool,
    pub(super) now: Duration,
}

/// Which way focus crossed this pass; see [`ViewState::roll_focus`].
#[derive(Clone, Copy, Debug)]
pub(super) struct FocusEdges {
    pub(super) gained: bool,
    pub(super) lost: bool,
}
