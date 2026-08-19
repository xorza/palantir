//! The crate's host-facing input vocabulary.

use crate::input::keyboard::{Key, Modifiers, TextChunk};
use crate::input::pointer::PointerButton;
use glam::Vec2;

/// Palantir-native input event. Independent of any windowing toolkit.
/// All coordinates are in **logical pixels** (DIPs). Backends are responsible
/// for any physical→logical conversion before dispatching.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    /// Pointer position in logical pixels, relative to the surface origin.
    PointerMoved(Vec2),
    /// Pointer left the surface; clears `hovered`.
    PointerLeft,
    PointerPressed(PointerButton),
    PointerReleased(PointerButton),
    /// Pixel-precise scroll delta — touchpad / precision wheel /
    /// `MouseScrollDelta::PixelDelta`. Logical pixels. Positive `y`
    /// means the user wants content to scroll *down* (a scroll widget
    /// should add to its vertical offset). Multiple events in one frame
    /// accumulate on the scroll target active when each event arrived.
    ScrollPixels(Vec2),
    /// Notched scroll delta — classic wheel /
    /// `MouseScrollDelta::LineDelta`. Carries the raw line count
    /// (sign-flipped to match `ScrollPixels`); the consuming widget
    /// multiplies by its own font-derived line step at record time
    /// rather than this layer baking in a constant. Multiple events
    /// in one frame accumulate on their event-time scroll targets.
    ScrollLines(Vec2),
    /// Multiplicative zoom factor from a touch / touchpad pinch gesture.
    /// `1.0` is identity; `1.05` zooms in 5%, `0.95` zooms out 5%.
    /// Multiple events in one frame multiply into their event-time
    /// pinch targets' zoom totals. Wheel-based zoom is *not*
    /// translated into `Zoom` — the active scroll widget decides at
    /// record time whether wheel ticks count as pan or zoom. Non-positive
    /// and non-finite factors are discarded at ingress.
    Zoom(f32),
    /// Logical key was pressed. `repeat` reflects OS-level key repeat
    /// (held keys re-emit). Modifier state isn't carried on the event;
    /// consumers read the latest [`Modifiers`] from `InputState`. We
    /// don't carry releases — no consumer needs them yet.
    KeyDown {
        key: Key,
        repeat: bool,
        /// Layout-independent physical key — see
        /// [`KeyPress::physical`](crate::input::keyboard::KeyPress::physical).
        physical: Key,
    },
    /// Committed text — a typed character or an IME composition that
    /// just finalized. Distinct from `KeyDown` because IME / dead-key
    /// composition produces text without a physical keypress, and
    /// because keys like `Enter` produce a logical key but no text we
    /// want to insert. Editors should consume `Text` for character
    /// input and `KeyDown` for navigation/control keys.
    Text(TextChunk),
    /// Modifier-key set changed. The carried snapshot is the new state
    /// (not a delta). Consumers track the latest snapshot to disambiguate
    /// e.g. ctrl+'a' (shortcut) from 'a' (text).
    ModifiersChanged(Modifiers),
}
