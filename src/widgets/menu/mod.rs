//! The menu, its rows, and the rule between groups.

pub(crate) mod menu_item;
pub(crate) mod menu_landing;
pub(crate) mod menu_separator;

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::math::domain::vec2;
use crate::primitives::paint::chrome::Chrome;
use crate::scene::layer::Layer;
use crate::theme::menu::MenuTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::response::Response;
use crate::widgets::menu::menu_landing::MenuLanding;
use crate::widgets::popup::Popup;
use crate::widgets::popup::trigger_state::{Opening, Settled, TriggerFrame, TriggerState};

use glam::Vec2;
use std::rc::Rc;

/// A menu: a popup of [`MenuItem`](crate::MenuItem) rows on [`Layer::Menu`], in the menu
/// look, whose rows the arrow keys walk. One type with two placements, as egui's
/// `Popup::context_menu` and `Popup::menu` are
/// (<https://docs.rs/egui/latest/egui/containers/struct.Popup.html>):
///
/// - [`Self::context`] opens it at the pointer on a right-click of its trigger;
/// - [`Self::below`] drops it below its trigger, which a click toggles, as a menu button's menu:
///   the frame a [`PopupTrigger`](crate::PopupTrigger) runs, in the menu look on [`Layer::Menu`].
///
/// A trigger has one menu, so the two placements on one trigger share its open state and its id.
///
/// ```
/// # use palantir::{Button, Menu, Configure, MenuItem, Ui};
/// # fn demo(ui: &mut Ui) {
/// let trigger = Button::new().label("…").show(ui);
/// Menu::context(&trigger)
///     .max_size((280.0, 400.0))
///     .show(ui, |ui| { MenuItem::new("Delete").show(ui); });
/// # }
/// ```
///
/// Their open state lives in the state map keyed off the trigger's id, so a gesture raises it
/// rather than application state; [`Self::open`], [`Self::close`] and [`Self::is_open`] drive it
/// from outside, and [`Self::for_id`] shows a menu only they open. [`Self::at`] is a menu whose
/// open state the caller keeps, as a [`Popup`]'s is, for a menu no one widget raises.
///
/// Closes on outside-click, Esc, a clicked row, or a row's matching
/// [`Shortcut`](crate::Shortcut). A disabled trigger closes it.
///
/// [`Self::style`] restyles the panel only; pass sub-themes to the rows.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Menu<'a> {
    opener: Opener,
    landing: MenuLanding,
    /// The popup this menu is. Its anchor is set at `show`; a closed menu returns before
    /// recording. It owns the chrome too, so `.chrome(..)` and the theme fallback land in one
    /// place.
    popup: Popup<'a>,
    style: Option<&'a MenuTheme>,
}

/// Who keeps a menu's open state, and what opened it this frame.
#[derive(Clone, Copy, Debug)]
enum Opener {
    /// The trigger's row in the state map; `at_pointer` is a right-click on the trigger this
    /// frame, which opens the menu at the pointer.
    Trigger {
        frame: TriggerFrame,
        at_pointer: bool,
    },
    /// The caller, which shows the menu at this point while it keeps it open.
    Caller(Vec2),
}

impl<'a> Menu<'a> {
    /// A menu on the trigger `for_id` that only [`Self::open`] opens, at the point it names. A
    /// menu on a trigger has the id [`Self::menu_id`] of the trigger's unless its `Configure` names
    /// another, settled here because the popup's `#[track_caller]` id would resolve to this line
    /// for every menu.
    pub fn for_id(for_id: WidgetId) -> Self {
        Self::on_trigger(
            TriggerFrame {
                for_id,
                toggled: false,
                opened: None,
                disabled: false,
                rect: None,
            },
            false,
        )
    }

    /// A context menu on the trigger whose response is `trigger`: `show` opens it at the pointer
    /// on a right-click.
    pub fn context(trigger: &Response) -> Self {
        Self::on_trigger(
            TriggerFrame {
                for_id: trigger.id,
                toggled: false,
                opened: None,
                disabled: trigger.disabled,
                rect: None,
            },
            trigger.right.clicked(),
        )
    }

    /// A menu dropped below the trigger whose response is `trigger`: a click on the trigger opens
    /// it and a second closes it. It hangs from the trigger's rect each frame and flips above it
    /// when it does not fit below.
    pub fn below(trigger: &Response) -> Self {
        Self::on_trigger(TriggerFrame::below(trigger), false)
    }

    /// A menu at surface-space `point` whose open state the caller keeps: it shows on every
    /// `show`, as a [`Popup`] does, and the caller stops showing it on the frame its response
    /// reads [`closed`](OverlayResponse::closed). Held where its first frame landed it, so a body
    /// that grows or shrinks does not move it under the pointer.
    ///
    /// # Panics
    ///
    /// Panics unless both axes of `point` are [offsets](crate::widget::domain::offset).
    #[track_caller]
    pub fn at(point: Vec2) -> Self {
        Self {
            opener: Opener::Caller(vec2::offset(point)),
            landing: MenuLanding::First,
            popup: Popup::new(),
            style: None,
        }
    }

    fn on_trigger(frame: TriggerFrame, at_pointer: bool) -> Self {
        Self {
            opener: Opener::Trigger { frame, at_pointer },
            landing: MenuLanding::First,
            popup: Popup::new().default_id(Menu::menu_id(frame.for_id)),
            style: None,
        }
    }

    /// Per-instance override of [`Theme::menu`](crate::Theme::menu). Restyles the panel only;
    /// rows take their own sub-themes.
    pub fn style(mut self, s: impl Into<Option<&'a MenuTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// The row focus lands on when the keyboard opens the menu; [`MenuLanding::First`] by
    /// default.
    pub const fn landing(mut self, landing: MenuLanding) -> Self {
        self.landing = landing;
        self
    }

    /// Record the menu and return the popup's outcome.
    ///
    /// `None` while the menu is closed, as the body did not run; a menu [`Self::at`] a point
    /// always shows, so it answers `Some`. A menu opened while
    /// focus shows the ring (from the keyboard) takes focus on its [landing](Self::landing) row,
    /// as WAI-ARIA's menu button does, and focus returns to where it was on close.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Option<OverlayResponse<R>> {
        self.show_as::<Menu<'static>, R>(ui, body)
    }

    /// [`Self::show`] with a trigger's open state in the row of `K`, the widget that wraps the
    /// menu, so the row is reached through that widget's own `open`, `close` and `is_open` alone.
    pub(crate) fn show_as<K: 'static, R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Option<OverlayResponse<R>> {
        let landing = self.landing;
        let opener = self.opener;
        let resp = match opener {
            Opener::Trigger { frame, at_pointer } => Self::opened_frame(ui, frame, at_pointer)
                .show::<K, R>(ui, |ui| self.dressed(ui), body),
            Opener::Caller(point) => Some(
                self.dressed(ui)
                    .anchor(Anchor::at_point(point))
                    .show(ui, body),
            ),
        };
        landing.land(ui, resp.as_ref());
        resp
    }

    /// [`Self::show`] over a `state` its owner keeps rather than the trigger's row, as a menu bar
    /// keeps one for the one menu it has open; the state to keep comes back.
    pub(crate) fn settle<K, R>(
        self,
        ui: &mut Ui,
        state: TriggerState<K>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Settled<K, R> {
        let landing = self.landing;
        let Opener::Trigger { frame, at_pointer } = self.opener else {
            unreachable!("a menu an owner settles hangs from its trigger");
        };
        let settled = Self::opened_frame(ui, frame, at_pointer).settle(
            ui,
            state,
            |ui| self.dressed(ui),
            body,
        );
        landing.land(ui, settled.response.as_ref());
        settled
    }

    /// `frame` with what the trigger opened this frame.
    fn opened_frame(ui: &mut Ui, mut frame: TriggerFrame, at_pointer: bool) -> TriggerFrame {
        if at_pointer {
            frame.opened = ui.pointer_pos().map(|p| Opening::At(vec2::offset(p)));
        }
        frame
    }

    /// The popup on [`Layer::Menu`] in the menu look; the caller's `Configure` calls already
    /// landed on it, and the theme fills the rest.
    fn dressed(self, ui: &Ui) -> Popup<'a> {
        let theme = Rc::clone(ui.theme());
        let menu = self.style.unwrap_or(&theme.menu);
        self.popup.layer(Layer::Menu).menu_look(menu)
    }

    /// Open the menu keyed off `for_id` at surface-space `point`. Idempotent;
    /// repeated calls move an open menu.
    ///
    /// # Panics
    ///
    /// Panics unless both axes of `point` are [offsets](crate::widget::domain::offset).
    #[track_caller]
    pub fn open(ui: &mut Ui, for_id: WidgetId, point: Vec2) {
        TriggerState::<Menu<'static>>::open(ui, for_id, Opening::At(vec2::offset(point)));
    }

    /// The menu of the trigger `trigger`, unless its `Configure` names another.
    pub fn menu_id(trigger: WidgetId) -> WidgetId {
        trigger.with("menu")
    }

    /// Close the menu keyed off `for_id`, giving back the input it held at once. No-op if closed.
    pub fn close(ui: &mut Ui, for_id: WidgetId) {
        TriggerState::<Menu<'static>>::close(ui, for_id);
    }

    /// Whether the menu keyed off `for_id` is open. Allocates no row.
    pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool {
        TriggerState::<Menu<'static>>::is_open(ui, for_id)
    }
}

impl Menu<'_> {
    /// Paint `chrome` as the menu panel's chrome; the theme's `panel`
    /// fills it when unset. [`Chrome::NONE`] suppresses the chrome.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn chrome(mut self, chrome: Chrome) -> Self {
        self.popup = self.popup.chrome(chrome);
        self
    }

    /// Paint `chrome` unless the caller set one, for a wrapper theming
    /// a widget after the caller's setters ran. An explicit
    /// [`Self::chrome`] wins in either order.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn default_chrome(mut self, chrome: Chrome) -> Self {
        self.popup = self.popup.default_chrome(chrome);
        self
    }
}

/// Forwards to the popup this menu wraps; the menu keeps no node of its own.
impl Configure for Menu<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.popup.configure()
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::ui::Ui;
    use crate::widgets::menu::Menu;
    use crate::widgets::popup::trigger_state::{Opening, TriggerState};

    impl Menu<'static> {
        /// Open the menu keyed off `for_id` below its trigger, as a click on it does.
        pub(crate) fn open_below(ui: &mut Ui, for_id: WidgetId) {
            TriggerState::<Self>::open(ui, for_id, Opening::Below);
        }
    }
}

#[cfg(test)]
mod tests;
