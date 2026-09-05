//! The anchored floating body: the widget and the press-outside policy.

use crate::input::sense::Sense;
use crate::layout::types::anchor::Anchor;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::overlay_response::OverlayResponse;
use crate::widgets::overlay_scope::{Backdrop, OverlayScope};
use crate::widgets::widget::Widget;
use glam::Vec2;
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
///   `OverlayResponse::dismissed` so the host can flip its open flag. Use for
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
/// Implements [`Configure`](crate::Configure) — use `.id(...)`, `.id_salt(...)`,
/// `.padding(...)`, `.size(...)`, etc. on the popup body.
#[derive(Debug)]
pub struct Popup {
    anchor: Anchor,
    click_outside: ClickOutside,
    layer: Layer,
    widget: Widget,
    chrome: Option<Background>,
}

impl Popup {
    #[track_caller]
    pub fn anchored_to(point: Vec2) -> Self {
        Self::new(Anchor::at_point(point))
    }

    #[track_caller]
    pub fn below(rect: Rect) -> Self {
        Self::new(Anchor::below(rect))
    }

    #[track_caller]
    pub fn above(rect: Rect) -> Self {
        Self::new(Anchor::above(rect))
    }

    #[track_caller]
    pub fn left_of(rect: Rect) -> Self {
        Self::new(Anchor::left_of(rect))
    }

    #[track_caller]
    pub fn right_of(rect: Rect) -> Self {
        Self::new(Anchor::right_of(rect))
    }

    #[track_caller]
    fn new(anchor: Anchor) -> Self {
        Self {
            anchor,
            click_outside: ClickOutside::Dismiss,
            layer: Layer::Popup,
            widget: Widget::vstack().sense(Sense::CLICK),
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

    /// Hold the body this far off its anchor, in logical px.
    ///
    /// A dropdown meets the trigger it drops out of, so the
    /// constructors start flush; an overlay that reads as a separate
    /// object — the way [`crate::Tooltip`] does, off
    /// [`TooltipTheme::gap`](crate::TooltipTheme) — sets its own.
    pub fn gap(mut self, px: f32) -> Self {
        self.anchor = self.anchor.gap(px);
        self
    }

    pub fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    /// Chrome to fall back on when the caller set none — the `Popup`
    /// peer of [`ThemeDefaults::default_padding`](crate::widgets::configure::ThemeDefaults::default_padding),
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

    /// Re-anchor an already-built popup.
    ///
    /// For a wrapper whose placement is late-bound: [`crate::ContextMenu`]
    /// holds its popup from the moment the caller starts configuring it,
    /// but doesn't learn where the menu was opened until `show` reads the
    /// state map. The constructors stay the canonical way in. This is the
    /// one case that cannot use them.
    pub(crate) fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<R> {
        let Self {
            anchor,
            click_outside,
            layer,
            mut widget,
            chrome,
        } = self;
        // Resolved before the layer switch below, so the body id — and the
        // eater derived from it — is parent-scoped to the trigger's site the
        // way any other widget is, not to the side layer's empty root.
        let eater_id = widget.resolve(ui).with("eater");
        // The two captures are one decision: an overlay either takes the
        // pointer *and* the keys from the layers below, or neither. Taking
        // one without the other leaves a host that is half-dead in a way
        // nothing at the call site would explain.
        let backdrop = if click_outside == ClickOutside::PassThrough {
            Backdrop::None
        } else {
            Backdrop::Eater(eater_id)
        };
        let id = widget.resolve(ui);
        let scope = OverlayScope::claim(id, layer, Some(anchor), backdrop, &mut widget);

        let theme = Rc::clone(ui.theme());
        widget.configure().default_clip(theme.panel_clip);
        let chrome = chrome.as_ref().or(theme.panel_background.as_ref());
        let handle = CloseHandle::default();
        let turn = scope.record(ui, |ui| widget.record(ui, chrome, |ui| body(ui, &handle)));
        let dismiss_mode = click_outside == ClickOutside::Dismiss;
        let response = OverlayResponse {
            // A `Dismiss` popup closes on an eaten outside-press OR an Esc
            // press — so overlay hosts (ComboBox / ContextMenu) read one
            // `closed()` signal instead of each re-deriving Esc. (`Block`
            // records a backdrop and ignores both edges.)
            dismissed: dismiss_mode && (turn.outside || turn.escape),
            close_requested: handle.requested(),
            inner: turn.inner,
        };
        scope.withdraw(ui, response.closed());
        response
    }
}

impl Popup {
    /// Paint `bg` as this widget's background.
    ///
    /// `None` is the default; theme fallback in [`Self::show`] fills it in
    /// from `ui.theme().panel_background` when unset. Pass
    /// [`Background::NONE`] to suppress that fallback for this popup.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }
}

impl Configure for Popup {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
