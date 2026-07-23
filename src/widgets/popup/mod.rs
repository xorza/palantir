use crate::input::keyboard::{Key, KeyboardEvent};
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::overlay::OverlayPosition;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::scene::layer::Layer;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::ui::Ui;
use crate::ui::keyboard_capture::KeyboardCapture;
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
/// widgets can request dismissal and consume the keyboard stream
/// exclusively captured by this popup without handling its owner id.
///
/// Lives on the stack for the duration of one [`Popup::show`] call —
/// no ambient `Ui` state, no nested-popup signal-leak.
#[derive(Debug)]
pub struct PopupHandle<'capture> {
    requested: Cell<bool>,
    keyboard: &'capture KeyboardCapture,
}

impl<'capture> PopupHandle<'capture> {
    fn new(keyboard: &'capture KeyboardCapture) -> Self {
        Self {
            requested: Cell::new(false),
            keyboard,
        }
    }

    /// Ask the enclosing popup to dismiss.
    pub fn close(&self) {
        self.requested.set(true);
    }

    /// Keyboard events captured by this popup in arrival order.
    /// Returns an empty slice when another popup owns capture. Use
    /// [`Ui::subscribe_keyboard`] for off-focus event categories;
    /// [`Self::key_pressed`] subscribes its shortcut automatically.
    pub fn keyboard_events<'ui>(&self, ui: &'ui Ui) -> &'ui [KeyboardEvent] {
        self.keyboard.keyboard_events(ui)
    }

    /// Whether this popup captured a matching key press this frame.
    /// Subscribes the shortcut for wake-up like [`Ui::key_pressed`].
    pub fn key_pressed(&self, ui: &mut Ui, shortcut: Shortcut) -> bool {
        self.keyboard.key_pressed(ui, shortcut)
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
/// While recorded, the topmost popup exclusively owns keyboard input.
/// Focus remains unchanged, so context-menu commands can still operate
/// on their trigger without also reaching the focused widget.
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

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &PopupHandle<'_>)) -> PopupResponse {
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
        ui.with_keyboard_capture(keyboard_owner, |ui, keyboard| {
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
            ui.layer(Layer::Popup, Vec2::ZERO, None, |ui| {
                Frame::new()
                    .id(eater_id)
                    .size((Sizing::FILL, Sizing::FILL))
                    .sense(Sense::CLICK | Sense::DRAG | Sense::SCROLL | Sense::PINCH)
                    .show(ui);
            });
            let chrome = resolve_container_chrome(
                &mut widget.node,
                chrome,
                ui.theme.panel_background.as_ref(),
                ui.theme.panel_clip,
            );
            let handle = PopupHandle::new(keyboard);
            ui.overlay_layer(Layer::Popup, position, |ui| {
                widget.record(ui, chrome.as_ref(), |ui| body(ui, &handle));
            });
            let dismiss_mode = click_outside == ClickOutside::Dismiss;
            let eater_clicked = ui.response_for(eater_id).left.clicked();
            let response = PopupResponse {
                // A `Dismiss` popup closes on an eaten outside-press OR an Esc
                // press — so overlay hosts (ComboBox / ContextMenu) read one
                // `closed()` signal instead of each re-deriving Esc. (`Block`
                // short-circuits, so it neither dismisses on nor subscribes Esc.)
                dismissed: dismiss_mode
                    && (eater_clicked || handle.key_pressed(ui, Shortcut::key(Key::Escape))),
                close_requested: handle.requested.get(),
            };
            if response.closed() {
                keyboard.release();
            }
            response
        })
    }
}

impl Configure for Popup {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
    }
}

#[cfg(test)]
mod tests;
