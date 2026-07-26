use crate::input::keyboard::{Key, KeyboardEvent};
use crate::input::pointer::PointerEvent;
use crate::input::shortcut::Shortcut;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::Ui;

/// A modal layer's authority over both input streams, handed to the body
/// of [`Ui::modal_layer`]. The keyboard half is owner-scoped and the
/// pointer half is layer-scoped, but they are claimed and released as
/// one, because keeping their lifecycles apart is what let three of the
/// four overlay/stream pairs go on owning input for a frame after their
/// overlay was gone.
///
/// The owner id stays internal so captured input cannot be read through
/// a mismatched widget.
///
/// `Copy` on purpose: an overlay decides whether it closed *after* its
/// layer scope ends — `Popup` reads its dismiss response outside — so
/// the claim has to survive being handed back out of the body it was
/// given to.
#[derive(Debug, Clone, Copy)]
pub struct InputClaim {
    owner: WidgetId,
    layer: Layer,
}

impl InputClaim {
    pub(super) fn new(owner: WidgetId, layer: Layer) -> Self {
        Self { owner, layer }
    }

    /// Keyboard events this owner claimed, in arrival order. Returns an
    /// empty slice when another owner holds the claim.
    pub fn keyboard_events<'ui>(&self, ui: &'ui Ui) -> &'ui [KeyboardEvent] {
        ui.input.claimed_keyboard_events(self.owner)
    }

    /// Whether this owner claimed a matching key press this frame.
    /// Watches the shortcut for wake-up like [`Ui::key_pressed`].
    pub fn key_pressed(&self, ui: &mut Ui, shortcut: Shortcut) -> bool {
        ui.input.claimed_key_pressed(self.owner, shortcut)
    }

    /// Sugar for `key_pressed(Shortcut::key(Key::Escape))`, the claimed
    /// twin of [`Ui::escape_pressed`] — every overlay in the tree reads
    /// exactly this to decide it was dismissed.
    pub fn escape_pressed(&self, ui: &mut Ui) -> bool {
        self.key_pressed(ui, Shortcut::key(Key::Escape))
    }

    /// Pointer events as seen from the claimed layer — the same stream
    /// [`Ui::pointer_events`] returns from *inside* the body.
    ///
    /// It exists for outside it. An overlay resolves whether it closed
    /// after its layer scope ends (`Popup` does), and there the ambient
    /// layer is below the claim, so a plain `ui.pointer_events()` is
    /// silenced by the overlay's own gate and quietly returns nothing.
    /// Reading through the claim is how an owner gets at its own stream
    /// from anywhere, exactly as [`Self::keyboard_events`] is.
    pub fn pointer_events<'ui>(&self, ui: &'ui Ui) -> &'ui [PointerEvent] {
        ui.input.pointer_events(self.layer)
    }

    /// Withdraw the claim so the resolution at the end of this pass does
    /// not see it — both halves, since there is only one claim.
    ///
    /// **The pass you call it in is unaffected.** Ownership is committed
    /// once per pass and read by the next, deliberately (see
    /// `InputState::finish_record`), so reads through this handle keep
    /// working and layers below stay gated until the pass ends. What
    /// release buys is the pass *after*: call it on the frame the overlay
    /// decides it is closing, or the claim it already recorded is
    /// committed anyway and the overlay owns input for one more frame
    /// after it is gone — long enough to swallow the click that lands
    /// where it used to be.
    pub fn release(&self, ui: &mut Ui) {
        ui.input.release_input(self.owner);
    }
}
