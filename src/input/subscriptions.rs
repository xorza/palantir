//! Off-target wake gates + unified pointer event stream.
//!
//! [`Subscriptions`] holds two pieces of state — both cleared
//! pre-record, both re-asserted by widgets each frame they're
//! active (symmetric to `Sense` on a node):
//!
//! 1. [`Subscriptions::any_mask`] — category flags ([`PointerSense`])
//!    answering "does this event class wake the frame?"
//! 2. [`Subscriptions::keys`] — specific `(Key, Modifiers)` chords for
//!    modal Escape / command-palette shortcuts.
//!
//! Across silent (PaintOnly / skipped) frames the set **persists** —
//! that's the wake signal: a dormant popup needs `BUTTONS`
//! to still be set when the next click outside lands.
//!
//! Delivery isn't routed through subscriptions. Pointer events flow
//! into [`InputState::frame_pointer_events`](super::InputState),
//! keys into [`InputState::frame_keys`](super::InputState). Both
//! buffers are populated only when a relevant subscription is active
//! (the `any_mask` check short-circuits the push), so idle frames
//! pay nothing.

use crate::input::keyboard::{Key, Modifiers};
use bitflags::bitflags;

bitflags! {
    /// Wake-gate categories. Granular so a popup subscribing for
    /// clicks doesn't wake on every pointer move; canvases that want
    /// every move opt in explicitly.
    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct PointerSense: u8 {
        /// Wakes on [`PointerEvent::Down`] / [`PointerEvent::Up`].
        /// Popup dismiss-on-press, focus traps.
        const BUTTONS = 1 << 0;
        /// Wakes on [`PointerEvent::Move`]. Eyedropper, custom
        /// crosshair, drag-anywhere overlays. Expensive in event
        /// count — opt in only when needed.
        const MOVE = 1 << 1;
        /// Wakes on [`PointerEvent::Scroll`] / [`PointerEvent::Zoom`].
        /// Global scroll capture (minimap, debug overlay).
        const SCROLL = 1 << 2;
    }
}

impl PointerSense {
    pub const NONE: Self = Self::empty();
}

bitflags! {
    /// Keyboard wake-gate categories. Orthogonal to focus routing —
    /// a focused widget always wakes on `KeyDown` / `Text` regardless
    /// of these flags; subscribing here is for **off-focus** consumers
    /// (hotkey recorder, debug overlay, accel-underline UIs).
    /// Specific `(Key, Modifiers)` chords use the finer
    /// [`Subscriptions::keys`] path instead.
    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct KeyboardSense: u8 {
        /// Wakes on
        /// [`KeyboardEvent::Text`](crate::input::keyboard::KeyboardEvent::Text)
        /// regardless of focus. Command palette filtering before
        /// focus, post-IME-commit consumers.
        const TEXT = 1 << 0;
        /// Wakes on any
        /// [`KeyboardEvent::Down`](crate::input::keyboard::KeyboardEvent::Down)
        /// regardless of focus. Hotkey recorder, cheat codes, debug
        /// key overlay.
        const KEY = 1 << 1;
        /// Wakes on `ModifiersChanged`. Accel-underline UIs that
        /// reveal on Alt-press, modifier-state debug overlays.
        const MODIFIER = 1 << 2;
    }
}

impl KeyboardSense {
    pub const NONE: Self = Self::empty();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub key: Key,
    pub mods: Modifiers,
}

/// Per-`Ui` wake-gate registry. Cleared pre-record; widgets re-OR /
/// re-push their declarations during record.
#[derive(Default)]
pub(crate) struct Subscriptions {
    pub(crate) pointer_mask: PointerSense,
    pub(crate) keyboard_mask: KeyboardSense,
    pub(crate) keys: Vec<KeyChord>,
}

impl Subscriptions {
    /// Idempotent push — duplicate chords from multiple subscribers
    /// collapse to one entry. Linear `contains` is fine at the
    /// expected count. (Direct field assignment is the pattern for
    /// `pointer_mask` / `keyboard_mask` — both are `pub(crate)` and
    /// `Ui::subscribe_pointer` / `Ui::subscribe_keyboard` OR into them
    /// inline; the dedup logic here is the only non-trivial bit.)
    pub(crate) fn subscribe_key(&mut self, chord: KeyChord) {
        if !self.keys.contains(&chord) {
            self.keys.push(chord);
        }
    }

    /// Test whether a key event would wake any specific-chord
    /// subscriber.
    pub(crate) fn matches_key(&self, key: Key, mods: Modifiers) -> bool {
        self.keys.iter().any(|c| c.key == key && c.mods == mods)
    }

    /// Capacity-retained pre-record clear. Called from
    /// [`Ui::record_pass`](crate::Ui) before every full record
    /// (including pass B of a double-layout frame).
    pub(crate) fn clear(&mut self) {
        self.pointer_mask = PointerSense::NONE;
        self.keyboard_mask = KeyboardSense::NONE;
        self.keys.clear();
    }
}
