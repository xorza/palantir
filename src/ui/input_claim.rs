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

    /// Withdraw **both** halves from the current record pass. Reads
    /// through the handle after this see nothing, the owner takes no part
    /// in the topmost-wins keyboard resolution at frame end, and the
    /// layer stops gating pointer watchers below it.
    ///
    /// Call it on the frame the overlay decides it is closing. Claims
    /// resolve at the end of a pass and are read by the *next* one, so an
    /// overlay that records its claim and then dismisses keeps owning
    /// input for one frame after it is gone — long enough to swallow the
    /// click that lands where it used to be.
    pub fn release(&self, ui: &mut Ui) {
        ui.input.release_keyboard(self.owner);
        ui.input.release_pointer(self.layer);
    }
}
