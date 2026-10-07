//! The crate's host-facing input vocabulary.

use crate::input::ime_preedit::ImePreedit;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerButton;
use crate::input::zoom_factor::ZoomFactor;
use glam::Vec2;

/// Palantir-native input event, independent of any windowing toolkit. Coordinates
/// are **logical pixels**. Scalars are screened at ingress (non-finite values and
/// non-positive zoom are discarded), so a host may forward whatever its platform
/// reported. The IME variants borrow the host's text; the event stays `Copy`.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent<'a> {
    /// Pointer position, relative to the surface origin.
    PointerMoved(Vec2),
    /// Pointer left the surface; clears `hovered`.
    PointerLeft,
    /// A button went down at the last reported position.
    PointerPressed(PointerButton),
    /// A button came back up.
    PointerReleased(PointerButton),
    /// Pixel-precise scroll delta (touchpad, `MouseScrollDelta::PixelDelta`);
    /// positive `y` scrolls content *down*.
    ScrollPixels(Vec2),
    /// Notched scroll delta (`MouseScrollDelta::LineDelta`): the raw line count,
    /// sign-flipped to match `ScrollPixels`; the consuming widget applies its line
    /// step.
    ScrollLines(Vec2),
    /// Multiplicative zoom factor from a pinch gesture: `1.0` is identity, `1.05`
    /// zooms in 5%. Wheel zoom is not translated into `Zoom`.
    Zoom(f32),
    /// Logical key pressed; `repeat` reflects OS key repeat. Modifiers (read
    /// [`Modifiers`] from `InputState`) and releases are not carried.
    KeyDown {
        /// The logical key, after the keyboard layout is applied.
        key: Key,
        /// The press came from OS key repeat.
        repeat: bool,
        /// Layout-independent physical key; see
        /// [`KeyPress::physical`](crate::KeyPress::physical).
        physical: Key,
        /// The text this press produced (see [`KeyText`]); empty means nothing can
        /// be typed.
        text: KeyText,
    },
    /// Modifier set changed; the snapshot is the new state, not a delta.
    ModifiersChanged(Modifiers),
    /// An input method's uncommitted text changed; empty `text` ends the
    /// composition. Forwarded only while a widget asks, see
    /// [`Ui::request_ime`](crate::Ui::request_ime).
    ImePreedit(ImePreedit<'a>),
    /// An input method committed `text`, typed in place among the key presses.
    ImeCommit(&'a str),
    /// The surface lost keyboard focus. **Everything held is no longer held:** an
    /// unfocused surface reports nothing, so a released button or dropped modifier
    /// is never seen and would stay latched.
    SurfaceFocusLost,
}

impl<'a> InputEvent<'a> {
    pub(crate) const fn text(&self) -> Option<&'a str> {
        match *self {
            Self::ImePreedit(ImePreedit { text, .. }) | Self::ImeCommit(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) const fn with_text(self, text: &str) -> InputEvent<'_> {
        match self {
            Self::ImePreedit(ImePreedit { cursor, .. }) => {
                InputEvent::ImePreedit(ImePreedit { text, cursor })
            }
            Self::ImeCommit(_) => InputEvent::ImeCommit(text),
            Self::PointerMoved(p) => InputEvent::PointerMoved(p),
            Self::PointerLeft => InputEvent::PointerLeft,
            Self::PointerPressed(button) => InputEvent::PointerPressed(button),
            Self::PointerReleased(button) => InputEvent::PointerReleased(button),
            Self::ScrollPixels(delta) => InputEvent::ScrollPixels(delta),
            Self::ScrollLines(delta) => InputEvent::ScrollLines(delta),
            Self::Zoom(factor) => InputEvent::Zoom(factor),
            Self::KeyDown {
                key,
                repeat,
                physical,
                text: key_text,
            } => InputEvent::KeyDown {
                key,
                repeat,
                physical,
                text: key_text,
            },
            Self::ModifiersChanged(mods) => InputEvent::ModifiersChanged(mods),
            Self::SurfaceFocusLost => InputEvent::SurfaceFocusLost,
        }
    }

    /// Whether this event's payload is one the pipeline can act on. **The screen on
    /// host input**, applied once by
    /// [`InputState::on_input`](crate::input::input_state::InputState): a
    /// non-finite scalar would poison retained state (NaN fails every hit-test; an
    /// offset holding one trips [`TranslateScale::new`](crate::TranslateScale)'s
    /// finite check later). Zoom composes by multiplication, so it must be strictly
    /// positive too.
    pub(crate) fn is_valid(&self) -> bool {
        match self {
            Self::PointerMoved(p) | Self::ScrollPixels(p) | Self::ScrollLines(p) => p.is_finite(),
            Self::Zoom(factor) => ZoomFactor::new(*factor).is_some(),
            Self::PointerLeft
            | Self::PointerPressed(_)
            | Self::PointerReleased(_)
            | Self::KeyDown { .. }
            | Self::ModifiersChanged(_)
            | Self::SurfaceFocusLost
            | Self::ImeCommit(_) => true,
            Self::ImePreedit(ImePreedit { text, cursor }) => {
                cursor.is_none_or(|span| text.get(span.range()).is_some())
            }
        }
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::input::input_event::InputEvent;
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::key_text::KeyText;

    impl InputEvent<'_> {
        /// A first press of `key`, typing what it types on a plain layout;
        /// `physical` is [`Key::Other`], read only for a non-ASCII `Char` under a
        /// command modifier.
        pub(crate) fn key_down(key: Key) -> Self {
            Self::KeyDown {
                key,
                repeat: false,
                physical: Key::Other,
                text: KeyText::of_key(key),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::input::ime_preedit::ImePreedit;
    use crate::input::input_event::InputEvent;
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::key_text::KeyText;
    use crate::input::keyboard::modifiers::Modifiers;
    use crate::input::pointer::PointerButton;
    use glam::Vec2;

    /// Every scalar-carrying variant is screened and the rest pass; payload-free
    /// arms are listed so a new variant has to be answered here.
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
        for factor in [0.0, -0.0, -1.0] {
            assert!(!InputEvent::Zoom(factor).is_valid(), "zoom {factor}");
        }

        let ok: &[InputEvent<'_>] = &[
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
                text: KeyText::from_char('a'),
            },
            InputEvent::ModifiersChanged(Modifiers::default()),
            InputEvent::SurfaceFocusLost,
            InputEvent::ImePreedit(ImePreedit {
                text: "かな",
                cursor: Some(Span::new(3, 3)),
            }),
            InputEvent::ImeCommit("かな"),
        ];
        for cursor in [Span::new(3, 4), Span::new(1, 2)] {
            let event = InputEvent::ImePreedit(ImePreedit {
                text: "かな",
                cursor: Some(cursor),
            });
            assert!(!event.is_valid(), "{event:?}");
        }
        // No `_` arm: a new variant fails to compile until indexed, and the count
        // fails until it has a case.
        let mut covered = [false; 12];
        for event in ok {
            assert!(event.is_valid(), "{event:?}");
            let index = match event {
                InputEvent::PointerMoved(_) => 0,
                InputEvent::PointerLeft => 1,
                InputEvent::PointerPressed(_) => 2,
                InputEvent::PointerReleased(_) => 3,
                InputEvent::ScrollPixels(_) => 4,
                InputEvent::ScrollLines(_) => 5,
                InputEvent::Zoom(_) => 6,
                InputEvent::KeyDown { .. } => 7,
                InputEvent::ModifiersChanged(_) => 8,
                InputEvent::SurfaceFocusLost => 9,
                InputEvent::ImePreedit(_) => 10,
                InputEvent::ImeCommit(_) => 11,
            };
            covered[index] = true;
        }
        assert!(
            covered.iter().all(|&c| c),
            "a variant has no valid case: {covered:?}"
        );
    }
}
