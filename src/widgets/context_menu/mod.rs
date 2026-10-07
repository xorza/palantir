//! The right-click popup menu, its rows, and the rule between groups.

pub(crate) mod menu_item;
pub(crate) mod menu_separator;

use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::axis::Axis;
use crate::primitives::math::domain::vec2;
use crate::primitives::paint::background::Background;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::response::ResponseSnapshot;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::popup::Popup;
use crate::widgets::theme::context_menu::ContextMenuTheme;

use glam::Vec2;
use std::rc::Rc;

/// Open state for one context-menu site, keyed off the trigger's id.
#[derive(Default, Clone, Copy, Debug)]
struct ContextMenuState {
    open_at: Option<Vec2>,
}

/// A right-click or programmatically opened popup menu attached to a trigger.
///
/// Unlike [`Popup`] and [`Modal`](crate::Modal), it owns its open state
/// ([`Self::is_open`], [`Self::open`], [`Self::close`]), since a gesture
/// raises it rather than application state.
///
/// [`Self::on`] opens it at the pointer on a right-click:
///
/// ```
/// # use palantir::{Button, ContextMenu, Configure, MenuItem, Ui};
/// # fn demo(ui: &mut Ui) {
/// let trigger = Button::new().label("…").show(ui).snapshot();
/// ContextMenu::on(&trigger)
///     .max_size((280.0, 400.0))
///     .show(ui, |ui, popup| { MenuItem::new("Delete").show(ui, popup); });
/// # }
/// ```
///
/// For programmatic opens call [`Self::open`] before
/// [`Self::for_id`]`(id).show(...)`.
///
/// Closes on outside-click, Esc, a clicked item, or an item's matching
/// [`Shortcut`](crate::input::shortcut::Shortcut).
///
/// [`Self::style`] restyles the panel only; pass sub-themes to the rows.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ContextMenu<'a> {
    for_id: WidgetId,
    /// The trigger reported a right-click this frame.
    open_on_show: bool,
    /// The popup this menu is. Its anchor is a placeholder until `show`; a
    /// closed menu returns before recording. It owns the chrome too, so
    /// `.background(..)` and the theme fallback land in one place.
    popup: Popup,
    style: Option<&'a ContextMenuTheme>,
}

impl<'a> ContextMenu<'a> {
    /// Identity is the trigger's, settled here because the popup's
    /// `#[track_caller]` id would resolve to this line for every menu.
    pub fn for_id(for_id: WidgetId) -> Self {
        Self {
            for_id,
            open_on_show: false,
            popup: Popup::new(Anchor::at_point(Vec2::ZERO)).default_id(for_id.with("body")),
            style: None,
        }
    }

    /// Attach a menu to the trigger `snapshot` was taken from; `show` opens
    /// it at the pointer on a right-click. Use `trigger.snapshot()` to
    /// release the `&Ui` borrow.
    pub fn on(snapshot: &ResponseSnapshot) -> Self {
        Self {
            open_on_show: snapshot.right.clicked(),
            ..Self::for_id(snapshot.id)
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `context_menu`. Restyles
    /// the panel only; rows take their own sub-themes.
    pub fn style(mut self, s: impl Into<Option<&'a ContextMenuTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the menu and return the popup's outcome.
    ///
    /// `inner` is `None` when the menu is closed and the body did not run.
    /// The body records in [`Layer::Menu`], so a menu can be raised from
    /// inside a popup or dialog.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<Option<R>> {
        if self.open_on_show
            && let Some(p) = ui.pointer_pos()
        {
            ContextMenu::open(ui, self.for_id, p);
        }
        // Read via `state` so a never-opened menu stores no StateMap row.
        let Some(open_at) = ui
            .state::<ContextMenuState>(self.for_id)
            .and_then(|st| st.open_at)
        else {
            return OverlayResponse::default();
        };

        let ui_theme = Rc::clone(ui.theme());
        let ctx = self.style.unwrap_or(&ui_theme.context_menu);

        // The caller's `Configure` calls already landed on the popup; the
        // theme fills the rest. Identity falls back to the trigger's.
        let resp = self
            .popup
            .layer(Layer::Menu)
            .anchor(Anchor::at_point(open_at))
            .default_background(ctx.panel.clone())
            .default_padding(ctx.padding)
            .default_min_size(Size::new(ctx.min_width, 0.0))
            .default_gap(ctx.gap)
            .arrow_focus(Axis::Y)
            .show(ui, |ui, handle| Some(body(ui, handle)));
        if resp.closed() {
            ContextMenu::close(ui, self.for_id);
        }

        resp
    }

    /// Open the menu keyed off `for_id` at surface-space `point`. Idempotent;
    /// repeated calls move an open menu.
    ///
    /// # Panics
    ///
    /// Panics unless both axes of `point` are [offsets](crate::widget::domain::offset).
    #[track_caller]
    pub fn open(ui: &mut Ui, for_id: WidgetId, point: Vec2) {
        ui.with_state::<ContextMenuState, _>(for_id, |_, s| s.open_at = Some(vec2::offset(point)));
    }

    /// Close the menu keyed off `for_id`. No-op if closed.
    pub fn close(ui: &mut Ui, for_id: WidgetId) {
        if ui
            .state::<ContextMenuState>(for_id)
            .is_some_and(|s| s.open_at.is_some())
        {
            ui.with_state::<ContextMenuState, _>(for_id, |_, s| s.open_at = None);
        }
    }

    /// Whether the menu keyed off `for_id` is open. Allocates no row.
    pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool {
        ui.state::<ContextMenuState>(for_id)
            .is_some_and(|st| st.open_at.is_some())
    }
}

impl ContextMenu<'_> {
    /// Paint `background` as the menu panel's background; the theme's `panel`
    /// fills it when unset. [`Background::NONE`] suppresses the chrome.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn background(mut self, background: Background) -> Self {
        self.popup = self.popup.background(background);
        self
    }

    /// Paint `background` unless the caller set one, for a wrapper theming
    /// a widget after the caller's setters ran. An explicit
    /// [`Self::background`] wins in either order.
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn default_background(mut self, background: Background) -> Self {
        self.popup = self.popup.default_background(background);
        self
    }
}

/// Forwards to the popup this menu wraps; the menu keeps no node of its own.
impl Configure for ContextMenu<'_> {
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.popup.configure()
    }
}

#[cfg(test)]
mod tests;
