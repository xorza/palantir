//! [`OwnedInputEvent`]: an [`InputEvent`] that owns its text.

use crate::input::input_event::InputEvent;

/// An [`InputEvent`] whose IME text is owned, so it outlives the text it was built from and
/// crosses threads, as [`HostHandle::send_input`](crate::HostHandle::send_input) takes it.
#[derive(Clone, Debug)]
pub struct OwnedInputEvent {
    /// The event with its text emptied; `text` holds it.
    event: InputEvent<'static>,
    text: String,
}

impl OwnedInputEvent {
    /// The event, its text lent from `self`.
    pub fn as_event(&self) -> InputEvent<'_> {
        self.event.with_text(&self.text)
    }
}

impl From<InputEvent<'_>> for OwnedInputEvent {
    fn from(event: InputEvent<'_>) -> Self {
        Self {
            text: event.text().unwrap_or_default().to_owned(),
            event: event.with_text(""),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::input::ime_preedit::ImePreedit;
    use crate::input::input_event::InputEvent;
    use crate::input::keyboard::key::Key;
    use crate::input::owned_input_event::OwnedInputEvent;
    use crate::input::pointer::PointerButton;
    use glam::Vec2;

    /// Each event comes back as it went in, the text of both IME variants included; `InputEvent`
    /// has no `PartialEq`, so its `Debug` form, which prints every field, stands in.
    #[test]
    fn an_owned_event_lends_back_the_event_it_was_built_from() {
        let text = String::from("かな");
        let events = [
            InputEvent::PointerMoved(Vec2::new(-3.5, 12.0)),
            InputEvent::PointerPressed(PointerButton::Left),
            InputEvent::key_down(Key::Char('a')),
            InputEvent::ImePreedit(ImePreedit {
                text: &text,
                cursor: Some(Span::new(3, 3)),
            }),
            InputEvent::ImeCommit(&text),
            InputEvent::ImeCommit(""),
        ];
        let owned = events.map(OwnedInputEvent::from);
        drop(text);
        let expected = [
            "PointerMoved(Vec2(-3.5, 12.0))",
            "PointerPressed(Left)",
            r#"KeyDown { key: Char('a'), repeat: false, physical: Other, text: "a" }"#,
            r#"ImePreedit(ImePreedit { text: "かな", cursor: Some(Span { start: 3, len: 3 }) })"#,
            r#"ImeCommit("かな")"#,
            r#"ImeCommit("")"#,
        ];
        for (owned, expected) in owned.iter().zip(expected) {
            assert_eq!(format!("{:?}", owned.as_event()), expected);
        }
    }
}
