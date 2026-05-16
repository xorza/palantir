use crate::forest::element::{Configure, Element, LayoutMode};
use crate::forest::tree::Layer;
use crate::input::sense::Sense;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::ui::Ui;
use crate::widgets::frame::Frame;
use glam::Vec2;
use std::cell::Cell;

/// What happens when the user presses outside the popup's body.
///
/// Both modes install a full-surface "click-eater" leaf in the
/// `Popup` layer behind the popup body — outside clicks hit the
/// eater (`Sense::CLICK`) and don't propagate to the `Main` tree
/// underneath. They differ only in whether the popup widget signals
/// dismissal:
///
/// - [`Self::Block`] — eater consumes the click; no signal. Use for
///   confirm dialogs, stop-the-world prompts.
/// - [`Self::Dismiss`] — eater consumes the click AND
///   `PopupResponse.dismissed` is set so the host can flip its open
///   flag. Use for dropdowns, context menus, autocomplete.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickOutside {
    Block,
    Dismiss,
}

/// Per-frame close signal handed to the body closure. Content
/// widgets (e.g. [`crate::widgets::context_menu::MenuItem`]) call
/// [`Self::close`] to ask the enclosing popup to dismiss without
/// threading state through their caller.
///
/// Lives on the stack for the duration of one [`Popup::show`] call —
/// no ambient `Ui` state, no nested-popup signal-leak.
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

/// Result of [`Popup::show`]. `dismissed` is set when an outside
/// click was eaten this frame and the popup was configured for
/// [`ClickOutside::Dismiss`]. `close_requested` is set when a
/// content widget inside the body called [`PopupHandle::close`].
/// Hosts read either to flip their open flag in the same frame.
#[derive(Copy, Clone, Debug, Default)]
pub struct PopupResponse {
    pub dismissed: bool,
    pub close_requested: bool,
}

/// A side-layer container placed at a screen-space point. Records
/// into [`Layer::Popup`] so it draws above all `Main` siblings,
/// escapes ancestor clip, and hit-tests on top.
///
/// `anchor` is the body's top-left, typically derived from a trigger
/// widget's last-frame `Response.state.rect` (e.g. its bottom-left
/// for a dropdown). Sizing is governed by the body's own `Sizing`
/// chain — `Hug` shrinks to content, `Fill` fills the remaining
/// surface, `Fixed` is exact. Mid-recording is supported.
///
/// Outside clicks are handled per [`ClickOutside`]: a full-surface
/// "click-eater" leaf is recorded in the `Popup` layer underneath
/// the body, so clicks anywhere outside the body don't leak through
/// to the `Main` tree. Inside-body clicks route to the body's own
/// leaves first (popup hit-test priority).
///
/// Implements [`Configure`] — use `.id(...)`, `.id_salt(...)`,
/// `.padding(...)`, `.size(...)`, etc. on the popup body.
pub struct Popup {
    anchor: Vec2,
    click_outside: ClickOutside,
    element: Element,
    pub(crate) chrome: Option<Background>,
}

impl Popup {
    #[track_caller]
    pub fn anchored_to(anchor: Vec2) -> Self {
        let mut element = Element::new(LayoutMode::VStack);
        element.set_sense(Sense::CLICK);
        Self {
            anchor,
            click_outside: ClickOutside::Dismiss,
            element,
            chrome: None,
        }
    }

    pub fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None`
    /// is the default; theme fallback in [`Self::show`] fills it in
    /// from `ui.theme.panel_background` when unset.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &PopupHandle)) -> PopupResponse {
        let body_id = self.element.id;
        let eater_id = body_id.with("eater");
        // Eater records first → paints under the body. Hit-test runs
        // reverse-iter so the body's leaves still win inside its rect.
        ui.layer(Layer::Popup, Vec2::ZERO, None, |ui| {
            Frame::new()
                .id(eater_id)
                .size((Sizing::FILL, Sizing::FILL))
                .sense(Sense::CLICK)
                .show(ui);
        });
        let mut element = self.element;
        let chrome = self.chrome.or(ui.theme.panel_background);
        if matches!(element.clip_mode(), ClipMode::None) {
            element.set_clip(ui.theme.panel_clip);
        }
        let handle = PopupHandle::new();
        ui.layer(Layer::Popup, self.anchor, None, |ui| match chrome {
            Some(c) => {
                ui.node_with_chrome(element, c, |ui| body(ui, &handle));
            }
            None => {
                ui.node(element, |ui| body(ui, &handle));
            }
        });
        let eater_clicked = ui.response_for(eater_id).clicked;
        PopupResponse {
            dismissed: eater_clicked && self.click_outside == ClickOutside::Dismiss,
            close_requested: handle.requested.get(),
        }
    }
}

impl Configure for Popup {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
