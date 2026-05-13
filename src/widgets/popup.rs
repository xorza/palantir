use crate::forest::element::{Configure, Element, LayoutMode};
use crate::forest::tree::Layer;
use crate::forest::widget_id::WidgetId;
use crate::input::sense::Sense;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::ui::Ui;
use crate::widgets::frame::Frame;
use glam::Vec2;

/// Per-frame "content asked to dismiss" flag, read and cleared at
/// `Popup::show` boundaries. Stored in `StateMap` under
/// [`POPUP_CTX_ID`] so popup machinery doesn't bloat `Ui`. Empty
/// between frames in steady state.
///
/// Single global bool: `request_close` targets the *innermost*
/// currently-recording `Popup::show`. Nested popups in the same
/// frame can't propagate a close up.
#[derive(Default)]
pub(crate) struct PopupCtx {
    pub(crate) close_requested: bool,
}

/// Sentinel `WidgetId` for [`PopupCtx`] inside `StateMap`. Picked to
/// avoid colliding with any user id (auto ids hash file/line/column,
/// explicit ids hash user keys — neither produces this fixed value).
pub(crate) const POPUP_CTX_ID: WidgetId = WidgetId(0xC0FFEE_BADC0DE_u64);

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

/// Result of [`Popup::show`]. `dismissed` is set when an outside
/// click was eaten this frame and the popup was configured for
/// [`ClickOutside::Dismiss`]; hosts read it to flip their open flag
/// in the same frame. `close_requested` is set when a content widget
/// inside the body called [`Popup::request_close`] (e.g. a `MenuItem`
/// reporting a click) — hosts handle it the same way as `dismissed`.
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
}

impl Popup {
    pub fn anchored_to(anchor: Vec2) -> Self {
        let mut element = Element::new(LayoutMode::VStack);
        element.sense = Sense::CLICK;
        Self {
            anchor,
            click_outside: ClickOutside::Dismiss,
            element,
        }
    }

    pub fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> PopupResponse {
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
        if element.chrome.is_none() {
            element.chrome = ui.theme.panel_background;
        }
        if matches!(element.clip, ClipMode::None) {
            element.clip = ui.theme.panel_clip;
        }
        // Cleared before the body so a `request_close` inside is
        // attributable to this Popup::show.
        Popup::ctx_mut(ui).close_requested = false;
        ui.layer(Layer::Popup, self.anchor, None, |ui| {
            ui.node(element, body);
        });
        let close_requested = Popup::ctx_mut(ui).close_requested;
        Popup::ctx_mut(ui).close_requested = false;
        let eater_clicked = ui.response_for(eater_id).clicked;
        let dismissed = eater_clicked && self.click_outside == ClickOutside::Dismiss;
        PopupResponse {
            dismissed,
            close_requested,
        }
    }

    /// Ask the enclosing popup to dismiss. Read and cleared by
    /// [`Popup::show`]; content widgets (e.g. `MenuItem`) call this
    /// on click without knowing which popup hosts them.
    pub fn request_close(ui: &mut Ui) {
        Popup::ctx_mut(ui).close_requested = true;
    }

    fn ctx_mut(ui: &mut Ui) -> &mut PopupCtx {
        ui.state_mut::<PopupCtx>(POPUP_CTX_ID)
    }
}

impl Configure for Popup {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
