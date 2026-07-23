use crate::input::keyboard::KeyboardEvent;
use crate::input::shortcut::Shortcut;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use std::cell::Cell;

/// Scoped access to one keyboard-capture owner. Created by
/// [`Ui::with_keyboard_capture`]; the owner id remains internal so
/// captured input cannot be read through a mismatched widget.
#[derive(Debug)]
pub struct KeyboardCapture {
    owner: WidgetId,
    pub(crate) release_requested: Cell<bool>,
}

impl KeyboardCapture {
    pub(crate) fn new(owner: WidgetId) -> Self {
        Self {
            owner,
            release_requested: Cell::new(false),
        }
    }

    /// Keyboard events captured by this owner in arrival order.
    /// Returns an empty slice when another owner holds capture.
    pub fn keyboard_events<'ui>(&self, ui: &'ui Ui) -> &'ui [KeyboardEvent] {
        ui.input.captured_keyboard_events(self.owner)
    }

    /// Whether this owner captured a matching key press this frame.
    /// Subscribes the shortcut for wake-up like [`Ui::key_pressed`].
    pub fn key_pressed(&self, ui: &mut Ui, shortcut: Shortcut) -> bool {
        ui.input.captured_key_pressed(self.owner, shortcut)
    }

    /// Request withdrawal from the current record pass after the
    /// capture closure returns. Captured input remains readable for
    /// the rest of the closure.
    pub fn release(&self) {
        self.release_requested.set(true);
    }
}
