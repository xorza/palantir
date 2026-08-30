//! The right-click popup menu, its rows, and the rule between groups.

pub(crate) mod menu_item;
pub(crate) mod menu_separator;

use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::scene::node::configure::ConfigureNode;
use crate::scene::node::theme_defaults::ThemeDefaults;
use crate::ui::Ui;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::overlay_response::OverlayResponse;
use crate::widgets::popup::Popup;
use crate::widgets::response::ResponseSnapshot;
use crate::widgets::theme::context_menu::ContextMenuTheme;

use glam::Vec2;

/// Cross-frame response for one context-menu site, keyed off the trigger
/// widget's id in [`StateMap`](crate::ui::state::StateMap). `anchor = Some`
/// is the single source of truth for "menu open".
#[derive(Default, Clone, Copy, Debug)]
struct ContextMenuState {
    anchor: Option<Vec2>,
}

/// A right-click / programmatically-opened popup menu attached to a
/// trigger widget. State lives in `StateMap` keyed off the trigger
/// id, so opening / dismissing survives across frames without the
/// caller threading a flag.
///
/// Typical usage chains [`Self::attach`] off a trigger's `Response`,
/// which auto-opens at the pointer on a right-click (`right.clicked()`):
///
/// ```
/// # use palantir::{Button, ContextMenu, Configure, MenuItem, Ui};
/// # fn demo(ui: &mut Ui) {
/// let trigger = Button::new().label("…").show(ui).snapshot();
/// ContextMenu::attach(ui, &trigger)
///     .max_size((280.0, 400.0))
///     .show(ui, |ui, popup| { MenuItem::new("Delete").show(ui, popup); });
/// # }
/// ```
///
/// For programmatic opens (keyboard shortcut, custom gesture) call
/// [`Self::open`] before [`Self::for_id`]`(id).show(...)`.
///
/// Closes on outside-click, on Esc, when any [`MenuItem`](crate::widgets::context_menu::menu_item::MenuItem) inside
/// reports `clicked()`, or when a [`MenuItem`](crate::widgets::context_menu::menu_item::MenuItem)'s declared
/// [`Shortcut`](crate::input::shortcut::Shortcut) matches a keypress this frame.
///
/// Chain `.size(...)`, `.max_size(...)`, `.min_size(...)`, `.padding(...)`,
/// `.gap(...)`, and `.background(...)` to configure the menu body. Theme-driven
/// defaults fill in any field the caller leaves untouched (`chrome`, `padding`,
/// `min_size.w`, `gap`). Identity and input behavior remain owned by the trigger.
///
/// [`Self::style`] swaps the whole [`ContextMenuTheme`] for one instance.
/// It restyles the *panel* only — the rows are recorded by the caller's
/// body closure, so pass the matching sub-themes down to them
/// ([`MenuItem::style`](crate::widgets::context_menu::menu_item::MenuItem::style), [`MenuSeparator::style`](crate::widgets::context_menu::menu_separator::MenuSeparator::style)).
#[derive(Debug)]
pub struct ContextMenu<'a> {
    for_id: WidgetId,
    /// The popup this menu *is*. It owns the body node from the start,
    /// so the caller's [`Configure`] calls land on the node that
    /// actually records — there is no second node to keep in sync or
    /// swap in at `show`. Its anchor is a placeholder until `show`
    /// re-places it (see [`Popup::anchored_at`]); a closed menu returns
    /// before recording, so the placeholder never places anything.
    ///
    /// It owns the chrome too, so `.background(..)` and the theme
    /// fallback land in one place rather than in a field here that
    /// `show` has to remember to copy across.
    popup: Popup,
    style: Option<&'a ContextMenuTheme>,
}

impl<'a> ContextMenu<'a> {
    /// The menu's identity is the trigger's, settled here rather than at
    /// `show`: the popup's `#[track_caller]` id would resolve to *this*
    /// line for every menu in the program, so nothing may be left
    /// depending on it.
    pub fn for_id(for_id: WidgetId) -> Self {
        Self {
            for_id,
            popup: Popup::anchored_to(Vec2::ZERO).default_id(for_id.with("body")),
            style: None,
        }
    }

    style_setter!(
        'a,
        ContextMenuTheme,
        context_menu,
        "Restyles the *panel* only — the rows are recorded by the caller's \
         body closure, so pass the matching sub-bundles to them \
         ([`MenuItem::style`](crate::widgets::context_menu::menu_item::MenuItem::style), [`MenuSeparator::style`](crate::widgets::context_menu::menu_separator::MenuSeparator::style)).",
    );

    /// Derive `for_id` from a trigger widget's response snapshot, and
    /// auto-open at the current pointer position if the trigger
    /// reported a right-click this frame. Pass via
    /// `trigger.snapshot()` to detach from the trigger's `&Ui`
    /// borrow before attaching the menu.
    pub fn attach(ui: &mut Ui, snapshot: &ResponseSnapshot) -> Self {
        if snapshot.right.clicked()
            && let Some(p) = ui.pointer_pos()
        {
            ContextMenu::open(ui, snapshot.id, p);
        }
        ContextMenu::for_id(snapshot.id)
    }

    /// Record the menu and return the popup's own per-frame outcome.
    ///
    /// [`OverlayResponse`] directly rather than a menu-specific wrapper:
    /// a context menu *is* a popup here, so it reports
    /// [`closed`](OverlayResponse::closed) — the same close predicate
    /// every other overlay-trigger widget branches on. Its `inner` is an
    /// `Option` because a closed menu never runs the body.
    ///
    /// The body closure records [`MenuItem`](crate::widgets::context_menu::menu_item::MenuItem)s inside
    /// [`Layer::Menu`], which is what lets a menu be raised from inside a
    /// popup or a dialog; the menu auto-closes on outside-click, Esc, or an
    /// item click.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<Option<R>> {
        // Esc dismissal is owned by the `Dismiss` popup below — it folds into
        // `resp.closed()`, so no hand-rolled `escape_pressed` here.
        //
        // Read via `try_state` so a never-opened menu doesn't materialize a
        // StateMap row every frame `show` is called (matches `is_open`'s no-alloc
        // path); the row only needs to exist after `open`.
        let Some(raw_anchor) = ui
            .try_state::<ContextMenuState>(self.for_id)
            .and_then(|st| st.anchor)
        else {
            return OverlayResponse::default();
        };

        let ui_theme = ui.theme().clone();
        let ctx = self.slot(&ui_theme);

        // The menu is the popup, configured: the caller's `Configure`
        // calls already landed on it, the menu theme fills in whatever
        // they left alone, and `Popup::show` resolves the result against
        // the surface. Identity falls back to the trigger's — a menu has
        // no call site of its own worth keying on.
        let resp = self
            .popup
            .on(Layer::Menu)
            .anchored_at(raw_anchor)
            .default_background(&ctx.panel)
            .default_padding(ctx.padding)
            .default_min_size(Size::new(ctx.min_width, 0.0))
            .default_gap(ctx.gap)
            .show(ui, |ui, handle| Some(body(ui, handle)));
        if resp.closed() {
            ContextMenu::close(ui, self.for_id);
        }

        resp
    }

    /// Open the context menu keyed off `for_id` at surface-space
    /// `anchor`. Idempotent — repeated calls refresh the anchor.
    pub fn open(ui: &mut Ui, for_id: WidgetId, anchor: Vec2) {
        ui.state_mut::<ContextMenuState>(for_id).anchor = Some(anchor);
    }

    /// Close the context menu keyed off `for_id`. No-op if already closed.
    pub fn close(ui: &mut Ui, for_id: WidgetId) {
        if let Some(response) = ui.try_state_mut::<ContextMenuState>(for_id) {
            response.anchor = None;
        }
    }

    /// `true` while the menu keyed off `for_id` has an active anchor.
    /// Cheap immutable probe — no row is allocated for triggers that
    /// have never been opened.
    pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool {
        ui.try_state::<ContextMenuState>(for_id)
            .is_some_and(|st| st.anchor.is_some())
    }
}

impl ContextMenu<'_> {
    /// Paint `bg` as the menu panel's background.
    ///
    /// Unset is the default; the theme fallback in [`Self::show`] fills
    /// it in from the resolved theme's `panel`. Pass
    /// [`Background::NONE`] to suppress the themed menu chrome.
    pub fn background(mut self, bg: Background) -> Self {
        self.popup = self.popup.background(bg);
        self
    }
}

/// Forwards to the popup this menu wraps, so `.size(...)` /
/// `.padding(...)` / `.id(...)` configure the node that actually
/// records — the menu keeps no node of its own.
impl Configure for ContextMenu<'_> {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.popup.node_mut()
    }
}

#[cfg(test)]
mod tests;
