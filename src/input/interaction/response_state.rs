//! Everything a widget asks about itself for one frame: where it arranged, and what the pointer and keyboard did to it.

use crate::input::interaction::button_state::ButtonState;
use crate::input::interaction::scroll_delta::ScrollDelta;
use crate::input::pointer::PointerButton;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::math::domain::vec2;
use glam::Vec2;

/// Snapshot of one widget's interaction state for the current frame.
///
/// `disabled` is the cascaded flag (self or any ancestor), one frame stale, with the widget's own `NodeFlags::is_disabled` folded on top by `Widget::response`. `focused` is current, not stale.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResponseState {
    /// Last frame's *visible* rect in surface space, after ancestor transforms and clipping; `None` on the first frame. See [`Self::layout_rect`] for untrimmed geometry.
    pub rect: Option<Rect>,
    /// Pre-transform, unclipped layout rect in world coords: a widget's true position regardless of parent scroll or clip.
    pub layout_rect: Option<Rect>,
    /// Cumulative ancestor transform mapping `layout_rect` into unclipped surface space; identity under none.
    pub transform: TranslateScale,
    /// Cursor position in widget-local logical coordinates relative to [`Self::layout_rect`]'s origin; `None` when off-surface or unarranged.
    pub pointer_local: Option<Vec2>,
    /// Pointer is over this widget's visible rect and nothing above took it (previous frame's cascade).
    ///
    /// **The observation; [`Self::hovered`] is the reaction.** A disabled widget still covers what is behind it, which a tooltip explaining why it is disabled needs, so this survives the disabled fold and `hovered` does not.
    pub pointer_over: bool,
    /// Disabled: this widget or any ancestor.
    ///
    /// **`true` empties the interaction half:** `left`, `right`, `middle` and `scroll` are default, so `clicked()`, `held()`, `pressed()` and drags read `false`; guarding with `!state.disabled &&` is dead code. `pointer_over`, `rect` and `pointer_local` survive as geometry.
    pub disabled: bool,
    /// This widget holds keyboard focus; current, not one frame stale.
    pub focused: bool,
    /// Primary-button state (`clicked`, `held`, press runs, drags).
    pub left: ButtonState,
    /// Secondary-button state; `right.clicked` is the context-menu trigger.
    pub right: ButtonState,
    /// Middle / wheel-button state, same surface as [`Self::left`].
    pub middle: ButtonState,
    /// Wheel / touchpad / pinch deltas routed to this widget.
    pub scroll: ScrollDelta,
}

impl ResponseState {
    /// Folds one source of "disabled" in, dropping the interaction half once any says so.
    ///
    /// Three sources arrive at different times (cascade flag, this frame's ancestor scratch, the widget's own flag, visible only to `Widget::response`); folding by hand let a widget disabled this frame report `disabled` beside `hovered` and `left.clicked()`. Idempotent: a later `false` re-enables nothing.
    ///
    /// **This is the whole of what disabling does:** the cascade keeps a disabled widget in the hit index, so input still routes to it, and emptying what it reads makes that nothing happen. `pointer_over` stays; `hovered` goes by reading `disabled`.
    #[inline]
    pub(crate) fn merge_disabled(&mut self, disabled: bool) {
        self.disabled |= disabled;
        if self.disabled {
            self.clear_interaction();
        }
    }

    /// Everything the pointer and wheel contributed this frame, back to default. `pointer_local` is geometry and stays (a literal listing what to *keep* once dropped it, so [`Ui::peek_pointer_local`](crate::Ui::peek_pointer_local) disagreed), as do `rect`, `transform` and `pointer_over`.
    #[inline]
    fn clear_interaction(&mut self) {
        self.left = ButtonState::default();
        self.right = ButtonState::default();
        self.middle = ButtonState::default();
        self.scroll = ScrollDelta::default();
    }

    /// Pointer is over this widget and it can react: `pointer_over` minus disabled widgets. Derived, so the two can't disagree.
    #[inline]
    pub const fn hovered(&self) -> bool {
        self.pointer_over && !self.disabled
    }

    /// One-frame edge: a primary-button press+release landed on the widget without latching a drag.
    ///
    /// The activation predicate for buttons, menu rows and tabs, reading `left` like [`Self::pressed`] and [`Self::press_fraction`]. A disabled widget's slices are empty, so no `disabled` guard is needed.
    #[inline]
    pub const fn clicked(&self) -> bool {
        self.left.clicked()
    }

    /// One-frame edge: this primary click completed a double; [`Self::clicked`] fires too. See [`ButtonState::click_count`] for triple and beyond.
    #[inline]
    pub const fn double_clicked(&self) -> bool {
        self.left.double_clicked()
    }

    /// One-frame edge: a press+release landed on the widget on **any** button without latching a drag; the dismissal question for overlay backdrops, where `clicked` would leave a menu opened by a secondary button un-closable by it.
    #[inline]
    pub const fn any_clicked(&self) -> bool {
        let mut i = 0;
        while i < PointerButton::COUNT {
            if self.button(PointerButton::ALL[i]).clicked() {
                return true;
            }
            i += 1;
        }
        false
    }

    /// The per-button slice for a **runtime** `button`; for a known button read the field directly (`state.left`).
    #[inline]
    pub const fn button(&self, button: PointerButton) -> &ButtonState {
        match button {
            PointerButton::Left => &self.left,
            PointerButton::Right => &self.right,
            PointerButton::Middle => &self.middle,
        }
    }

    /// [`Self::button`], mutably; the one way the router writes a slot. Indexing by `PointerButton::idx()` would make the enum's declaration order a silent contract.
    #[inline]
    pub(crate) const fn button_mut(&mut self, button: PointerButton) -> &mut ButtonState {
        match button {
            PointerButton::Left => &mut self.left,
            PointerButton::Right => &mut self.right,
            PointerButton::Middle => &mut self.middle,
        }
    }

    /// Primary-button press with the pointer still over the widget ("shows pressed visuals"): `left.held && hovered`.
    #[inline]
    pub const fn pressed(&self) -> bool {
        self.left.held() && self.hovered()
    }

    /// Where the primary button's gesture sits across the widget, a `0..1` share per axis, on the press, every drag frame and the release; `None` otherwise, while disabled, and before arrangement.
    ///
    /// Shared by pointer-driven axis widgets. `band` is the width of a centred knob that comes off each end (zero when the pointer is the position), see [`domain::band_fraction`](crate::widget::domain::band_fraction). Clamped.
    #[inline]
    pub fn press_fraction(&self, band: f32) -> Option<Vec2> {
        let in_gesture = self.pressed() || self.left.drag.is_live() || self.left.released();
        if self.disabled || !in_gesture {
            return None;
        }
        let local = self.pointer_local?;
        let rect = self.layout_rect?;
        Some(vec2::fraction_or(
            vec2::band_fraction(local, rect.size.into(), Vec2::splat(band)),
            Vec2::ZERO,
        ))
    }
}
