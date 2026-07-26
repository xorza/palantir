use crate::input::keyboard::KeyboardEvent;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::overlay::OverlayPosition;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::scene::layer::Layer;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::ui::Ui;
use crate::ui::input_claim::InputClaim;
use crate::widgets::frame::Frame;
use crate::widgets::resolve_container_chrome;
use glam::Vec2;
use std::cell::Cell;

/// What happens when the user presses outside the popup's body.
///
/// Both modes install a full-surface "click-eater" leaf in the
/// `Popup` layer behind the popup body — outside presses hit the
/// eater (it senses `CLICK | DRAG | SCROLL | PINCH`) and don't
/// propagate to the `Main` tree underneath. They differ only in
/// whether the popup widget signals
/// dismissal:
///
/// - [`Self::Block`] — eater consumes the click; no signal (and Esc is
///   ignored). Use for confirm dialogs, stop-the-world prompts.
/// - [`Self::Dismiss`] — an eaten outside-click **or** an Esc press sets
///   `PopupResponse.dismissed` so the host can flip its open flag. Use for
///   dropdowns, context menus, autocomplete.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickOutside {
    Block,
    Dismiss,
}

/// Scoped popup capabilities handed to the body closure. Content
/// widgets can request dismissal and read the keyboard stream this
/// popup claimed, without handling its owner id.
///
/// Lives on the stack for the duration of one [`Popup::show`] call —
/// no ambient `Ui` state, no nested-popup signal-leak.
#[derive(Debug)]
pub struct PopupHandle {
    requested: Cell<bool>,
    claim: InputClaim,
}

impl PopupHandle {
    fn new(claim: InputClaim) -> Self {
        Self {
            requested: Cell::new(false),
            claim,
        }
    }

    /// Ask the enclosing popup to dismiss.
    pub fn close(&self) {
        self.requested.set(true);
    }

    /// Keyboard events this popup claimed, in arrival order. Returns an
    /// empty slice when another overlay holds the claim. Use
    /// [`Ui::watch_keyboard`] for off-focus event categories;
    /// [`Self::key_pressed`] watches its shortcut automatically.
    pub fn keyboard_events<'ui>(&self, ui: &'ui Ui) -> &'ui [KeyboardEvent] {
        self.claim.keyboard_events(ui)
    }

    /// Whether this popup claimed a matching key press this frame.
    /// Watches the shortcut for wake-up like [`Ui::key_pressed`].
    pub fn key_pressed(&self, ui: &mut Ui, shortcut: Shortcut) -> bool {
        self.claim.key_pressed(ui, shortcut)
    }

    /// Sugar for `key_pressed(Shortcut::key(Key::Escape))`.
    pub fn escape_pressed(&self, ui: &mut Ui) -> bool {
        self.claim.escape_pressed(ui)
    }
}

/// Result of [`Popup::show`]. `dismissed` is set when a
/// [`ClickOutside::Dismiss`] popup is dismissed this frame — an eaten
/// outside-press or an Esc press. `close_requested` is set when a
/// content widget inside the body called [`PopupHandle::close`].
/// Hosts read either to flip their open flag in the same frame.
#[derive(Copy, Clone, Debug, Default)]
pub struct PopupResponse {
    pub dismissed: bool,
    pub close_requested: bool,
}

impl PopupResponse {
    /// `true` when the popup asked to close this frame — either an
    /// outside click dismissed it ([`Self::dismissed`]) or a content
    /// widget called [`PopupHandle::close`] ([`Self::close_requested`]).
    /// The single close-signal predicate shared by overlay-trigger
    /// widgets (`ComboBox`, `ContextMenu`) so the dismiss contract lives
    /// in one place.
    pub fn closed(&self) -> bool {
        self.dismissed || self.close_requested
    }
}

/// A side-layer container placed relative to a screen-space anchor.
/// Records into [`Layer::Popup`] so it draws above all `Main` siblings,
/// escapes ancestor clip, and hit-tests on top. Placement is resolved
/// from the body's current measured size, then flipped or shifted to fit
/// the surface.
///
/// Outside clicks are handled per [`ClickOutside`]: a full-surface
/// "click-eater" leaf is recorded in the `Popup` layer underneath
/// the body, so clicks anywhere outside the body don't leak through
/// to the `Main` tree. Inside-body clicks route to the body's own
/// leaves first (popup hit-test priority).
///
/// While recorded, the topmost popup owns both input streams — keyboard
/// and the pointer watches — for every layer below it. Focus remains
/// unchanged, so context-menu commands can still operate on their
/// trigger without also reaching the focused widget.
///
/// Implements [`Configure`] — use `.id(...)`, `.id_salt(...)`,
/// `.padding(...)`, `.size(...)`, etc. on the popup body.
#[derive(Debug)]
pub struct Popup {
    position: OverlayPosition,
    click_outside: ClickOutside,
    pub(crate) node: Node,
    chrome: Option<Background>,
}

impl Popup {
    #[track_caller]
    pub fn anchored_to(anchor: Vec2) -> Self {
        Self::positioned(OverlayPosition::at_point(anchor))
    }

    #[track_caller]
    pub fn below(anchor: Rect) -> Self {
        Self::positioned(OverlayPosition::below(anchor, 0.0))
    }

    #[track_caller]
    pub fn above(anchor: Rect) -> Self {
        Self::positioned(OverlayPosition::above(anchor, 0.0))
    }

    #[track_caller]
    pub fn left_of(anchor: Rect) -> Self {
        Self::positioned(OverlayPosition::left_of(anchor, 0.0))
    }

    #[track_caller]
    pub fn right_of(anchor: Rect) -> Self {
        Self::positioned(OverlayPosition::right_of(anchor, 0.0))
    }

    #[track_caller]
    fn positioned(position: OverlayPosition) -> Self {
        let mut node = Node::vstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            position,
            click_outside: ClickOutside::Dismiss,
            node,
            chrome: None,
        }
    }

    pub fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None`
    /// is the default; theme fallback in [`Self::show`] fills it in
    /// from `ui.theme.panel_background` when unset. Pass
    /// [`Background::NONE`] to suppress that fallback for this popup.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &PopupHandle)) -> PopupResponse {
        let Self {
            position,
            click_outside,
            node,
            chrome,
        } = self;
        // Popup body resolves at the root of `Layer::Popup` (no
        // open frames in that layer), so `Ui::widget`'s
        // parent-scoping is a no-op — the body id equals the bare
        // salt hash. That keeps the eater id (and any persistent
        // popup-side state) stable regardless of where in `Main`
        // the trigger lives.
        let mut widget = ui.widget(node);
        let keyboard_owner = widget.id();
        let eater_id = widget.id().with("eater");
        // The claim records the popup layer, which is what orders it
        // against overlays above and what stops it silencing its own body
        // (a `TextEdit` in a popup drains the uncaptured stream and would
        // otherwise get nothing).
        let claim = ui.modal_layer(
            Layer::Popup,
            Vec2::ZERO,
            None,
            keyboard_owner,
            |ui, claim| {
                // Eater records first → paints under the body. Hit-test runs
                // reverse-iter so the body's leaves still win inside its rect.
                //
                // Senses all four pointer interactions so the popup is truly
                // modal-over-`Main`: pan-drag, scroll, and pinch over the
                // surrounding area can't leak through to the host (e.g. a
                // graph canvas underneath that pans on middle-drag and zooms
                // on scroll/pinch). `Sense::CLICK` is the dismiss trigger;
                // the other three never produce visible behavior on the
                // eater itself — they're absorbed and discarded so the host
                // doesn't see them.
                Frame::new()
                    .id(eater_id)
                    .size((Sizing::FILL, Sizing::FILL))
                    .sense(Sense::ABSORB_POINTER)
                    .show(ui);
                claim
            },
        );

        {
            let chrome = resolve_container_chrome(
                &mut widget.node,
                chrome,
                ui.theme.panel_background.as_ref(),
                ui.theme.panel_clip,
            );
            let handle = PopupHandle::new(claim);
            ui.overlay_layer(Layer::Popup, position, |ui| {
                widget.record(ui, chrome.as_ref(), |ui| body(ui, &handle));
            });
            let dismiss_mode = click_outside == ClickOutside::Dismiss;
            let eater_clicked = ui.response_for(eater_id).left.clicked();
            let response = PopupResponse {
                // A `Dismiss` popup closes on an eaten outside-press OR an Esc
                // press — so overlay hosts (ComboBox / ContextMenu) read one
                // `closed()` signal instead of each re-deriving Esc. (`Block`
                // short-circuits, so it neither dismisses on nor watches Esc.)
                dismissed: dismiss_mode && (eater_clicked || handle.escape_pressed(ui)),
                close_requested: handle.requested.get(),
            };
            // Releases both streams, so the frame after dismissal reaches
            // `Main` intact rather than being swallowed by a popup that
            // is already gone.
            if response.closed() {
                claim.release(ui);
            }
            response
        }
    }
}

impl Configure for Popup {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
    }
}

#[cfg(test)]
mod tests;
