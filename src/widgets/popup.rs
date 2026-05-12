use crate::forest::element::{Configure, Element, LayoutMode};
use crate::forest::tree::Layer;
use crate::forest::widget_id::WidgetId;
use crate::input::sense::Sense;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::frame::Frame;
use glam::Vec2;

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

/// Result of [`Popup::show`]. `body` reflects the popup container's
/// own `Response` (pass-through of the wrapping vstack's state).
/// `dismissed` is set when an outside click was eaten this frame and
/// the popup was configured for [`ClickOutside::Dismiss`]; hosts
/// read it to flip their open flag in the same frame.
pub struct PopupResponse {
    pub body: Response,
    pub dismissed: bool,
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
    /// When `Some`, the body runs inside [`Ui::with_popup_id`] so
    /// content widgets can read [`Ui::current_popup_id`] and dismiss
    /// their host without threading the id through their builder.
    /// The id is whatever the caller treats as the close target —
    /// e.g. `ContextMenu` passes the trigger's `WidgetId` because
    /// that's where `ContextMenuState.anchor` lives.
    owner: Option<WidgetId>,
    element: Element,
}

impl Popup {
    pub fn anchored_to(anchor: Vec2) -> Self {
        let mut element = Element::new(LayoutMode::VStack);
        // Inside-body clicks land here so they don't fall through to the
        // eater leaf underneath. User can override with `.sense(...)`.
        element.sense = Sense::CLICK;
        // Caller must chain `.id_salt(...)` / `.id(...)` / `.auto_id()`
        // before `show()` — the `Ui::node` write-path asserts on default id.
        // Multiple popups sharing one show-site need distinct keys to
        // avoid colliding (the anchor is no longer auto-mixed in).
        Self {
            anchor,
            click_outside: ClickOutside::Dismiss,
            owner: None,
            element,
        }
    }

    pub fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    /// Mark `owner_id` as the popup's close target. The body then
    /// records inside [`Ui::with_popup_id`], so content widgets can
    /// look up the owner via [`Ui::current_popup_id`] and dismiss it
    /// without an extra parameter. `ContextMenu::show` uses this to
    /// let `MenuItem::show` self-close the menu on click.
    pub fn owned_by(mut self, owner_id: WidgetId) -> Self {
        self.owner = Some(owner_id);
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> PopupResponse {
        let body_id = self.element.id;
        let eater_id = body_id.with("eater");
        // Eater root: full-surface invisible `Sense::CLICK` leaf.
        // Records first in the `Popup` layer scope so it paints
        // *under* the body and (via reverse-iter hit-test) the
        // body's deeper leaves get visited first — only clicks
        // outside the body's rect fall through to the eater.
        // Anchored at the surface origin with Fill/Fill, so it
        // covers the whole surface regardless of `Display`.
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
        let mut body_resp: Option<Response> = None;
        let owner = self.owner;
        ui.layer(Layer::Popup, self.anchor, None, |ui| {
            let node = ui.node(element, |ui| match owner {
                Some(o) => ui.with_popup_id(o, body),
                None => body(ui),
            });
            body_resp = Some(Response {
                node,
                id: body_id,
                state: ui.response_for(body_id),
            });
        });
        let body = body_resp.expect("popup body did not record a root widget");
        // The eater fires `clicked` only when the press landed on
        // the eater's rect — i.e. outside the body. (Body's rect
        // sits on top of the eater in hit-test order.)
        let eater_clicked = ui.response_for(eater_id).clicked;
        let dismissed = eater_clicked && self.click_outside == ClickOutside::Dismiss;
        PopupResponse { body, dismissed }
    }
}

impl Configure for Popup {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
