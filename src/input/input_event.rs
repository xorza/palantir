//! The crate's host-facing input vocabulary.

use crate::input::keyboard::{Key, Modifiers, TextChunk};
use crate::input::pointer::PointerButton;
use crate::input::zoom;
use glam::Vec2;

/// Palantir-native input event. Independent of any windowing toolkit.
/// All coordinates are in **logical pixels** (DIPs). Backends are responsible
/// for any physical→logical conversion before dispatching.
///
/// Every scalar a variant carries is screened at ingress — a non-finite
/// coordinate or delta, and a zoom factor that is not strictly positive,
/// are discarded before anything reads them — so a host may forward
/// whatever its platform reported without filtering first.
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
    /// record time whether wheel ticks count as pan or zoom.
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

impl InputEvent {
    /// Whether this event's payload is one the pipeline can act on.
    ///
    /// **The screen on host input**, applied once by
    /// [`InputState::on_input`](crate::input::input_state::InputState) before
    /// any arm reads the event. Every scalar a variant carries lands in
    /// retained state — a pointer position becomes a hit-test coordinate, a
    /// scroll delta a viewport offset, a zoom factor a running product — and a
    /// non-finite one does not merely produce a wrong frame, it poisons that
    /// state: NaN compares false against every rect it is later tested
    /// against, and an offset holding one fails
    /// [`TranslateScale::new`](crate::TranslateScale)'s finite-translation
    /// contract several passes downstream, where nothing is left to name the
    /// event that caused it.
    ///
    /// One screen rather than one per arm: the arms differ in what they do
    /// with a value, not in whether they can hold a NaN. Zoom asks the
    /// stricter of the two questions — a factor composes by multiplication,
    /// so it must be strictly positive as well as finite.
    pub(crate) fn is_valid(&self) -> bool {
        match self {
            Self::PointerMoved(p) | Self::ScrollPixels(p) | Self::ScrollLines(p) => p.is_finite(),
            Self::Zoom(factor) => zoom::is_valid(*factor),
            Self::PointerLeft
            | Self::PointerPressed(_)
            | Self::PointerReleased(_)
            | Self::KeyDown { .. }
            | Self::Text(_)
            | Self::ModifiersChanged(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::input::input_event::InputEvent;
    use crate::input::keyboard::{Key, Modifiers, TextChunk};
    use crate::input::pointer::PointerButton;
    use glam::Vec2;

    /// Every variant carrying a scalar is screened, and every variant that
    /// carries none passes. The payload-free arms are listed out rather than
    /// sampled: the point of one gate is that adding a variant has to be
    /// answered here, and a sampled test would let a new float-carrying one
    /// through unnoticed.
    #[test]
    fn ingress_screens_every_scalar_payload_and_admits_the_rest() {
        let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        for value in bad {
            for axis in [Vec2::new(value, 0.0), Vec2::new(0.0, value)] {
                for event in [
                    InputEvent::PointerMoved(axis),
                    InputEvent::ScrollPixels(axis),
                    InputEvent::ScrollLines(axis),
                ] {
                    assert!(!event.is_valid(), "{event:?}");
                }
            }
            assert!(!InputEvent::Zoom(value).is_valid(), "zoom {value}");
        }
        // Zoom is the stricter question: finite is not enough.
        for factor in [0.0, -0.0, -1.0] {
            assert!(!InputEvent::Zoom(factor).is_valid(), "zoom {factor}");
        }

        let ok: &[InputEvent] = &[
            InputEvent::PointerMoved(Vec2::new(-3.5, 12.0)),
            InputEvent::ScrollPixels(Vec2::new(0.0, -40.0)),
            InputEvent::ScrollLines(Vec2::ZERO),
            InputEvent::Zoom(f32::MIN_POSITIVE),
            InputEvent::Zoom(1.05),
            InputEvent::PointerLeft,
            InputEvent::PointerPressed(PointerButton::Left),
            InputEvent::PointerReleased(PointerButton::Right),
            InputEvent::KeyDown {
                key: Key::Char('a'),
                repeat: false,
                physical: Key::Char('a'),
            },
            InputEvent::Text(TextChunk::new("a").unwrap()),
            InputEvent::ModifiersChanged(Modifiers::default()),
        ];
        for event in ok {
            assert!(event.is_valid(), "{event:?}");
        }
    }
}
