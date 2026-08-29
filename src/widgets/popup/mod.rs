//! The anchored floating body: the widget, the handle its body uses to
//! close itself, what a frame reports, and the press-outside policy.

use crate::input::sense::Sense;
use crate::layout::types::overlay::OverlayPosition;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::frame::Frame;
use crate::widgets::overlay_scope::OverlayScope;
use glam::Vec2;
use std::cell::Cell;
use std::rc::Rc;

/// What happens when the user presses outside the popup's body.
///
/// [`Self::Block`] and [`Self::Dismiss`] are the modal pair: each
/// installs a full-surface "click-eater" leaf in the `Popup` layer behind
/// the popup body — outside presses hit the eater (it senses
/// `CLICK | DRAG | SCROLL | PINCH`) and don't propagate to the `Main`
/// tree underneath — and each takes the layer's whole key scope, cutting
/// off every layer below. They differ only in whether the popup widget
/// signals dismissal:
///
/// - [`Self::Block`] — eater consumes the click; no signal (and Esc is
///   ignored). Use for confirm dialogs, stop-the-world prompts.
/// - [`Self::Dismiss`] — an eaten outside-click **or** an Esc press sets
///   `PopupResponse.dismissed` so the host can flip its open flag. Use for
///   dropdowns, context menus, autocomplete.
/// - [`Self::PassThrough`] — neither capture: no eater, no key-scope
///   claim. Presses and keys outside the body reach `Main` untouched and
///   never signal dismissal. Use for overlays that *annotate* rather than
///   interrupt — toasts, notifications, hover cards — where the host has
///   to stay live underneath.
///
/// The distinction bites hardest for an overlay recorded unconditionally
/// every frame. Under the modal pair that is a permanently dead host: the
/// eater swallows every pointer event and the key claim silences the
/// keyboard, with no interaction able to reach whatever would close it.
/// Under `PassThrough` it is exactly the harmless always-on banner it
/// looks like.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickOutside {
    Block,
    Dismiss,
    PassThrough,
}

/// The dismissal request handed to the body closure, so content widgets
/// can close the popup they are inside.
///
/// Lives on the stack for the duration of one [`Popup::show`] call —
/// no ambient `Ui` state, no nested-popup signal-leak.
///
/// Carries only the close request. Reading input needs no handle: a body
/// records *inside* the popup's layer and its scope, so plain
/// [`Ui::key_pressed`] / [`Ui::keyboard_events`] already answer as the
/// popup. Owner-scoped forwarders on this handle would only re-arrange
/// what the scope already provides.
#[derive(Debug)]
pub struct PopupHandle {
    requested: Cell<bool>,
}

impl PopupHandle {
    fn new() -> Self {
        Self {
            requested: Cell::new(false),
        }
    }

    /// Ask the enclosing popup to dismiss.
    pub fn close(&self) {
        self.requested.set(true);
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
/// Which layer is a field rather than a constant, because this is the engine
/// under every anchored overlay and not only the plain one: a context menu is
/// a popup on [`Layer::Menu`], which is what lets one be raised from inside a
/// popup.
///
/// Outside clicks are handled per [`ClickOutside`]. Under the modal pair
/// (`Block` / `Dismiss`, the default) a full-surface "click-eater" leaf
/// is recorded in the `Popup` layer underneath the body, so clicks
/// anywhere outside the body don't leak through to the `Main` tree.
/// Inside-body clicks route to the body's own leaves first (popup
/// hit-test priority).
///
/// While recorded, such a popup owns both input streams — keyboard and
/// the pointer watches — for every layer below it. Focus remains
/// unchanged, so context-menu commands can still operate on their
/// trigger without also reaching the focused widget. Choose
/// [`ClickOutside::PassThrough`] for an overlay that must not take
/// either stream.
///
/// Implements [`Configure`] — use `.id(...)`, `.id_salt(...)`,
/// `.padding(...)`, `.size(...)`, etc. on the popup body.
#[derive(Debug)]
pub struct Popup {
    position: OverlayPosition,
    click_outside: ClickOutside,
    layer: Layer,
    node: Node,
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
            layer: Layer::Popup,
            node,
            chrome: None,
        }
    }

    /// Record into `layer` rather than [`Layer::Popup`].
    ///
    /// In-crate, because which layer an overlay belongs on is a fact about the
    /// kind of overlay it is and not about where a caller wants it: the ranks
    /// are what keeps a menu above the popup that raised it, and a caller free
    /// to pick would be free to invert them.
    pub(crate) fn on(mut self, layer: Layer) -> Self {
        self.layer = layer;
        self
    }

    pub fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    /// Chrome to fall back on when the caller set none — the `Popup`
    /// peer of [`ThemeDefaults::default_padding`](crate::scene::node::theme_defaults::ThemeDefaults::default_padding),
    /// since chrome is a field here rather than on the node.
    ///
    /// Takes a borrow so a wrapper's themed panel is cloned only where
    /// it is used — the caller holds the whole theme bundle and reads
    /// the rest of it.
    pub(crate) fn default_background(mut self, bg: &Background) -> Self {
        if self.chrome.is_none() {
            self.chrome = Some(bg.clone());
        }
        self
    }

    /// Re-place an already-built popup at `anchor`.
    ///
    /// For a wrapper whose placement is late-bound: [`crate::ContextMenu`]
    /// holds its popup from the moment the caller starts configuring it,
    /// but doesn't learn where the menu was opened until `show` reads the
    /// state map. The constructors stay the canonical way in; this is the
    /// one case that can't use them.
    pub(crate) fn anchored_at(mut self, anchor: Vec2) -> Self {
        self.position = OverlayPosition::at_point(anchor);
        self
    }

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &PopupHandle)) -> PopupResponse {
        let Self {
            position,
            click_outside,
            layer,
            node,
            chrome,
        } = self;
        // Resolved before the layer switch below, so the body id — and the
        // eater derived from it — is parent-scoped to the trigger's site the
        // way any other widget is, not to the side layer's empty root.
        let mut widget = ui.widget(node);
        let eater_id = widget.id().with("eater");
        // The two captures are one decision: an overlay either takes the
        // pointer *and* the keys from the layers below, or neither. Taking
        // one without the other leaves a host that is half-dead in a way
        // nothing at the call site would explain.
        let modal = click_outside != ClickOutside::PassThrough;
        let scope = if modal {
            OverlayScope::claim(widget.id(), layer, position, &mut widget.node)
        } else {
            OverlayScope::passive(widget.id(), layer, position)
        };
        if modal {
            ui.layer(layer).show(|ui| {
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
            });
        }

        {
            let theme = Rc::clone(ui.theme());
            let chrome = widget.node.resolve_container_chrome(
                chrome.as_ref(),
                theme.panel_background.as_ref(),
                theme.panel_clip,
            );
            let handle = PopupHandle::new();
            let escape = scope.record(ui, |ui| {
                widget.record(ui, chrome, |ui| body(ui, &handle));
            });
            let dismiss_mode = click_outside == ClickOutside::Dismiss;
            // The eater senses all four pointer interactions, so a
            // secondary press outside is absorbed either way — see
            // `ResponseState::any_clicked` for why absorbing it and
            // ignoring it is the case that matters.
            //
            // Gated on the mode that reads it: the other two record no
            // eater at all (`Block` has one but ignores it), so this would
            // be a lookup for an id that isn't in the tree.
            let eater_clicked = dismiss_mode && ui.response_for(eater_id).any_clicked();
            let response = PopupResponse {
                // A `Dismiss` popup closes on an eaten outside-press OR an Esc
                // press — so overlay hosts (ComboBox / ContextMenu) read one
                // `closed()` signal instead of each re-deriving Esc. (`Block`
                // short-circuits, so it neither dismisses on nor watches Esc.)
                dismissed: dismiss_mode && (eater_clicked || escape),
                close_requested: handle.requested.get(),
            };
            scope.withdraw(ui, response.closed());
            response
        }
    }
}

impl_background!(
    Popup,
    "`None` is the default; theme fallback in [`Self::show`] fills it in from \
     `ui.theme().panel_background` when unset. Pass [`Background::NONE`] to \
     suppress that fallback for this popup.",
);
impl_configure!(Popup);

#[cfg(test)]
mod tests;
