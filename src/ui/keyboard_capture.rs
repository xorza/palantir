use crate::input::keyboard::KeyboardEvent;
use crate::input::shortcut::Shortcut;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;

/// Access to one keyboard-capture owner. Created by
/// [`Ui::claim_keyboard`]; the owner id remains internal so captured
/// input cannot be read through a mismatched widget.
#[derive(Debug)]
pub struct KeyboardCapture {
    owner: WidgetId,
}

impl KeyboardCapture {
    pub(super) fn new(owner: WidgetId) -> Self {
        Self { owner }
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

    /// Withdraw this claim from the current record pass. Reads through
    /// the handle after this see nothing, and the owner takes no part in
    /// the topmost-wins resolution at frame end.
    pub fn release(&self, ui: &mut Ui) {
        ui.input.release_keyboard_capture(self.owner);
    }
}
