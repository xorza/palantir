//! Off-target wake gates + unified pointer event stream.
//!
//! [`Watches`] holds two pieces of state — both cleared
//! pre-record, both re-asserted by widgets each frame they're
//! active (symmetric to `Sense` on a node):
//!
//! 1. [`Watches::pointer_mask`] / [`Watches::keyboard_mask`]
//!    — category flags ([`PointerWake`]) answering "does this event
//!    class wake the frame?"
//! 2. [`Watches::keys`] — specific `(Key, Modifiers)` chords for
//!    modal Escape / command-palette shortcuts.
//!
//! Across silent (PaintOnly / skipped) frames the set **persists** —
//! that's the wake signal: a dormant popup needs `BUTTONS`
//! to still be set when the next click outside lands.
//!
//! Delivery isn't routed through watches. Pointer and keyboard
//! events flow into their per-frame [`InputState`](crate::input::input_state::InputState)
//! queues. Both buffers are populated only when a relevant watch
//! is active (the mask check short-circuits the push), so idle frames
//! pay nothing.

use crate::input::keyboard::key_press::KeyPress;
use crate::input::shortcut::Shortcut;
use bitflags::bitflags;

bitflags! {
    /// Wake-gate categories. Granular so a popup watching for
    /// clicks doesn't wake on every pointer move; canvases that want
    /// every move opt in explicitly.
    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct PointerWake: u8 {
        /// Wakes on [`PointerEvent::Down`] / [`PointerEvent::Up`].
        /// Popup dismiss-on-press, focus traps.
        const BUTTONS = 1 << 0;
        /// Wakes on [`PointerEvent::Move`]. Eyedropper, custom
        /// crosshair, drag-anywhere overlays. Expensive in event
        /// count — opt in only when needed.
        const MOVE = 1 << 1;
        /// Wakes on [`PointerEvent::Scroll`]. Global scroll capture
        /// (minimap, debug overlay).
        const SCROLL = 1 << 2;
        /// Wakes on [`PointerEvent::Zoom`]. Separate from `SCROLL` for
        /// the same reason [`Sense::PINCH`](crate::Sense::PINCH) is
        /// separate from `Sense::SCROLL`: a wheel tick and a touchpad
        /// pinch are different gestures with different targets, and a
        /// watcher that wants one rarely wants to wake on the other.
        const PINCH = 1 << 3;
    }
}

impl PointerWake {
    pub const NONE: Self = Self::empty();
}

bitflags! {
    /// Keyboard wake-gate categories. Orthogonal to focus routing —
    /// a focused widget always wakes on `KeyDown` regardless of these
    /// flags; watching here is for **off-focus** consumers
    /// (hotkey recorder, debug overlay, accel-underline UIs).
    /// Specific `(Key, Modifiers)` chords use the finer
    /// [`Watches::keys`] path instead.
    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct KeyboardWake: u8 {
        /// Wakes on any [`KeyPress`](crate::KeyPress) regardless of
        /// focus. Hotkey recorder, cheat codes, debug key overlay.
        const KEY = 1 << 0;
        /// Wakes on `ModifiersChanged`. Accel-underline UIs that
        /// reveal on Alt-press, modifier-state debug overlays.
        const MODIFIER = 1 << 1;
    }
}

impl KeyboardWake {
    pub const NONE: Self = Self::empty();
}

/// Per-`Ui` wake-gate registry. Cleared pre-record; widgets re-OR /
/// re-push their declarations during record.
#[derive(Debug, Default)]
pub(super) struct Watches {
    pub(super) pointer_mask: PointerWake,
    pub(super) keyboard_mask: KeyboardWake,
    /// Specific-chord wake list. [`Shortcut`] carries platform-aware
    /// `Mods` (Cmd↔Ctrl) + ignore-case `Char` matching — the same
    /// vocabulary menus / context-menus use, so watches and
    /// menu shortcuts share one type.
    pub(super) keys: Vec<Shortcut>,
}

impl Watches {
    /// Idempotent push — duplicate shortcuts from multiple
    /// watchers collapse to one entry. Linear `contains` is fine
    /// at the expected count.
    pub(super) fn watch_key(&mut self, sc: Shortcut) {
        if !self.keys.contains(&sc) {
            self.keys.push(sc);
        }
    }

    /// Test whether a key press would wake any specific-chord watcher.
    /// Takes the whole [`KeyPress`] so [`Shortcut::matches`]'s non-Latin
    /// layout fallback applies — an off-focus Cmd/Ctrl chord on e.g. a
    /// Russian layout still wakes its watcher.
    pub(super) fn matches_press(&self, kp: KeyPress) -> bool {
        self.keys.iter().any(|s| s.matches(kp))
    }

    /// Capacity-retained pre-record clear. Called from
    /// `FrameCycle::record_pass` before every full record
    /// (including pass B of a double-layout frame).
    pub(super) fn clear(&mut self) {
        self.pointer_mask = PointerWake::NONE;
        self.keyboard_mask = KeyboardWake::NONE;
        self.keys.clear();
    }
}
