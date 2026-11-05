//! The application menu bar: a row of titles, each opening its menu, one open at a time.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::theme::Theme;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::menu::MenuTheme;
use crate::theme::menu_bar::{MenuBarTheme, MenuState};
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::{Configure, ThemeDefaults};
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::response::InnerResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::menu::Menu;
use crate::widgets::menu::menu_landing::MenuLanding;
use crate::widgets::popup::Popup;
use crate::widgets::popup::trigger_state::{Opening, TriggerState};
use glam::Vec2;
use std::mem;
use std::rc::Rc;

/// A row of menu titles, each opening its [`MenuItem`](crate::MenuItem) rows below it, and one
/// menu open at a time, as a desktop application's menu bar.
///
/// A click on a title opens its menu and a click on the open title closes it. While a menu is
/// open the bar tracks the pointer over its titles, as Qt's `QMenuBar` maps a move to a title
/// with `actionAt`: moving onto another title opens that one's menu, and a click on one opens it
/// or, on the open title, closes it. Left and Right move to the neighboring menu, wrapping, and
/// walk the titles while one holds focus, as the
/// [WAI-ARIA menubar](https://www.w3.org/WAI/ARIA/apg/patterns/menubar/) asks. A menu closes
/// as a [`Menu`] does: on Esc, a click outside, or a picked row.
///
/// ```
/// # use palantir::{MenuBar, MenuItem, Ui};
/// # fn demo(ui: &mut Ui) {
/// MenuBar::new().show(ui, |bar| {
///     bar.menu("File", |ui| {
///         if MenuItem::new("Quit").show(ui).clicked() {
///             // …
///         }
///     });
///     bar.menu("Edit", |ui| {
///         MenuItem::new("Undo").show(ui);
///     });
/// });
/// # }
/// ```
///
/// Implements [`Configure`]: the bar is a horizontal stack of its titles, as wide as its parent
/// and on [`MenuBarTheme::bar`] unless set otherwise, as a window's menu bar spans it.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct MenuBar<'a> {
    widget: Widget,
    style: Option<&'a MenuBarTheme>,
    menu_style: Option<&'a MenuTheme>,
}

/// What a [`MenuBar`] keeps between frames: the titles it recorded, in order, the one menu open,
/// and where the pointer was at the end of a frame with one open, so only a move onto another
/// title switches it. One slot for the open menu, so two cannot be open.
#[derive(Debug, Default)]
struct MenuBarRow {
    /// The arrows walk them and the pointer is found over them. Refilled each frame, so its
    /// buffer is reused.
    titles: Vec<WidgetId>,
    open: Option<OpenMenu>,
    pointer: Option<Vec2>,
}

/// The bar's open menu: its title and the trigger state its popup settles.
#[derive(Clone, Copy, Debug, PartialEq)]
struct OpenMenu {
    title: WidgetId,
    trigger: TriggerState<MenuBar<'static>>,
}

impl OpenMenu {
    /// The menu of `title`, opening below it.
    const fn below(title: WidgetId) -> Self {
        Self {
            title,
            trigger: TriggerState::opened(Opening::Below),
        }
    }
}

/// Where the bar sends its open menu this frame, settled before any title records.
#[derive(Clone, Copy, Debug)]
enum Switch {
    Keep,
    Close,
    Open(WidgetId),
}

impl<'a> MenuBar<'a> {
    /// A menu bar.
    #[track_caller]
    pub fn new() -> Self {
        Self {
            widget: Widget::hstack().arrow_focus(Axis::X),
            style: None,
            menu_style: None,
        }
    }

    /// Per-instance override of [`Theme::menu_bar`](crate::Theme::menu_bar), the bar and its
    /// titles; [`Self::menu_style`] restyles the menus.
    pub fn style(mut self, s: impl Into<Option<&'a MenuBarTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Per-instance override of [`Theme::menu`](crate::Theme::menu) for the menus the titles drop.
    pub fn menu_style(mut self, s: impl Into<Option<&'a MenuTheme>>) -> Self {
        self.menu_style = s.into();
        self
    }

    /// Record the bar and the menus `body` adds through [`MenuBarMenus::menu`].
    pub fn show<R>(
        mut self,
        ui: &mut Ui,
        body: impl FnOnce(&mut MenuBarMenus<'_>) -> R,
    ) -> InnerResponse<R> {
        let bar = self.widget.resolve(ui);
        let mut row = ui.with_state::<MenuBarRow, _>(bar, |_, row| mem::take(row));
        let mut open = row.open;
        if let Some(menu) = open {
            match switch(ui, &row, menu.title) {
                Switch::Keep => {}
                Switch::Close => {
                    menu.trigger.release(ui);
                    open = None;
                }
                Switch::Open(next) => {
                    menu.trigger.release(ui);
                    open = Some(OpenMenu::below(next));
                    // A title holding focus follows the open menu, which follows a focused title.
                    if focused_title(ui, &row.titles).is_some() {
                        ui.set_focus(next);
                    }
                }
            }
        }
        let mut titles = mem::take(&mut row.titles);
        titles.clear();

        let theme = Rc::clone(ui.theme());
        let style = self.style.unwrap_or(&theme.menu_bar);
        self.widget = self.widget.default_size((Sizing::FILL, Sizing::HUG));
        self.widget
            .configure()
            .default_gap(style.gap)
            .default_padding(style.bar_padding);
        let (inner, mut open, step) = self.widget.record(ui, Some(&style.bar), |ui| {
            let mut menus = MenuBarMenus {
                ui,
                bar,
                theme: &theme,
                style,
                menu_style: self.menu_style,
                titles: &mut titles,
                open,
                step: 0,
            };
            let inner = body(&mut menus);
            (inner, menus.open, menus.step)
        });
        // A menu whose title this frame did not record, as the bar lost it, closes.
        let at = open.and_then(|menu| titles.iter().position(|&title| title == menu.title));
        if let Some(menu) = open
            && at.is_none()
        {
            menu.trigger.release(ui);
            open = None;
        }
        if step != 0
            && let (Some(menu), Some(at)) = (open, at)
        {
            menu.trigger.release(ui);
            let len = titles.len();
            let to = (if step > 0 { at + 1 } else { at + len - 1 }) % len;
            open = Some(OpenMenu::below(titles[to]));
            ui.request_repaint();
        }
        // Read only while a menu is open, as the read wakes a frame on every pointer move.
        let pointer = open.and_then(|_| ui.pointer_pos());
        ui.with_state::<MenuBarRow, _>(bar, |_, row| {
            *row = MenuBarRow {
                titles,
                open,
                pointer,
            };
        });
        InnerResponse {
            response: ui.response_for(bar),
            inner,
        }
    }
}

/// The title holding focus among `titles`, if one does.
fn focused_title(ui: &Ui, titles: &[WidgetId]) -> Option<WidgetId> {
    let focus = ui.focus()?;
    titles.iter().copied().find(|&title| title == focus)
}

/// Where the open menu `open` goes. Focus moved to another title, as the arrows walk the titles,
/// opens that one's menu. Then the pointer over the titles: a click on the open title closes it
/// and one on another title opens that one, both landing on the open menu's barrier; a move
/// onto another title opens it. A pointer that rests where it was when the menu opened, as one
/// the keyboard opened under it, moves nothing.
fn switch(ui: &mut Ui, row: &MenuBarRow, open: WidgetId) -> Switch {
    // A modal that opened after the menu closes it before its title records, so the title
    // drops its open look in the same pass.
    if ui.overlays().cancels(Menu::menu_id(open)) {
        return Switch::Close;
    }
    if let Some(focused) = focused_title(ui, &row.titles)
        && focused != open
    {
        return Switch::Open(focused);
    }
    let pointer = ui.pointer_pos();
    let Some(at) = pointer else {
        return Switch::Keep;
    };
    let Some(over) = row.titles.iter().copied().find(|&title| {
        ui.response_for(title)
            .rect
            .is_some_and(|rect| rect.contains(at))
    }) else {
        return Switch::Keep;
    };
    let barrier = Popup::barrier_id(Menu::menu_id(open));
    if ui.response_for(barrier).left.clicked() {
        if over == open {
            Switch::Close
        } else {
            Switch::Open(over)
        }
    } else if over != open && row.pointer.is_some_and(|was| was != at) {
        Switch::Open(over)
    } else {
        Switch::Keep
    }
}

/// What [`MenuBar::show`] hands its body: the bar's titles, added in order by [`Self::menu`].
#[derive(Debug)]
pub struct MenuBarMenus<'u> {
    ui: &'u mut Ui,
    bar: WidgetId,
    theme: &'u Theme,
    style: &'u MenuBarTheme,
    menu_style: Option<&'u MenuTheme>,
    titles: &'u mut Vec<WidgetId>,
    open: Option<OpenMenu>,
    /// Left (−1) or Right (+1) pressed in the open menu, else 0.
    step: i8,
}

impl MenuBarMenus<'_> {
    /// Add a title reading `label`, and while its menu is open record `body` in it, where
    /// [`Ui::close_overlay`] closes the menu, as an activated [`MenuItem`](crate::MenuItem) does.
    /// Returns the menu's [`OverlayResponse`], `None` while the menu
    /// is closed. The label names the menu, so it is one of a kind
    /// in its bar.
    pub fn menu<'l, R>(
        &mut self,
        label: impl Into<TextInput<'l>>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Option<OverlayResponse<R>> {
        let ui = &mut *self.ui;
        let label = ui.intern(label.into());
        let title = MenuBar::title_id(self.bar, ui.text(label));
        self.titles.push(title);
        let mut widget = Widget::leaf().id(title).sense(Sense::CLICK).focusable(true);
        let mut response = widget.activation_response(ui, Activation::Button);
        let here = self.open.filter(|open| open.title == title);
        // The APG menubar's title keys: Down opens the menu on its first item, Up on its last
        // (https://www.w3.org/WAI/ARIA/apg/patterns/menubar/). Read as the bar, whose arrow scope
        // takes the arrows and walks the titles with Left and Right.
        let mut landing = MenuLanding::First;
        if !response.disabled && here.is_none() && ui.is_focus_within(title) {
            let down = ui.shortcut_pressed(Shortcut::key(Key::ArrowDown));
            let up = ui.shortcut_pressed(Shortcut::key(Key::ArrowUp));
            if up {
                landing = MenuLanding::Last;
            }
            if (down || up) && !response.clicked() {
                response.state.activate();
            }
        }
        let plan = self.style.plan(
            &response.state,
            MenuState::from(here.is_some()),
            self.theme.text,
        );
        let look = widget.wear(ui, plan);
        widget.record(ui, Some(&look.chrome), |ui| {
            ui.add_shape(Shape::text(label, look.text.font()).color(look.text.color));
        });

        let mut step = 0;
        let settled = Menu::below(&response)
            .id(Menu::menu_id(title))
            .style(self.menu_style)
            .landing(landing)
            .settle(
                ui,
                here.map_or_else(TriggerState::default, |open| open.trigger),
                |ui| {
                    let inner = body(ui);
                    if ui.key_pressed(Key::ArrowLeft) {
                        step = -1;
                    } else if ui.key_pressed(Key::ArrowRight) {
                        step = 1;
                    }
                    inner
                },
            );
        if settled.state.is_open_now() {
            // A title opening while another menu is open takes the bar's one slot from it.
            if let Some(other) = self.open.filter(|open| open.title != title) {
                other.trigger.release(ui);
            }
            self.open = Some(OpenMenu {
                title,
                trigger: settled.state,
            });
            self.step = step;
        } else if here.is_some() {
            self.open = None;
        }
        settled.response
    }
}

/// Driving a menu bar from outside, by the bar's id, and the ids of its parts.
impl MenuBar<'static> {
    /// Open the menu of the bar `bar` whose title reads `label`, closing the one open; it
    /// records on the bar's next `show`. A label the bar does not show opens nothing.
    pub fn open(ui: &mut Ui, bar: WidgetId, label: &str) {
        let next = Self::title_id(bar, label);
        ui.with_state::<MenuBarRow, _>(bar, |ui, row| {
            if let Some(open) = row.open.filter(|open| open.title != next) {
                open.trigger.release(ui);
            }
            if row.open.is_none_or(|open| open.title != next) {
                row.open = Some(OpenMenu::below(next));
            }
        });
    }

    /// Close the open menu of the bar `bar`, giving back the input it held at once. No-op if
    /// none is open.
    pub fn close(ui: &mut Ui, bar: WidgetId) {
        if !Self::is_open(ui, bar) {
            return;
        }
        ui.with_state::<MenuBarRow, _>(bar, |ui, row| {
            if let Some(open) = row.open.take() {
                open.trigger.release(ui);
            }
        });
    }

    /// Whether the bar `bar` has a menu open. Allocates no row.
    pub fn is_open(ui: &Ui, bar: WidgetId) -> bool {
        ui.state::<MenuBarRow>(bar)
            .is_some_and(|row| row.open.is_some())
    }

    /// The title of the bar `bar`'s menu reading `label`. Keyed by its text, so a menu added or
    /// dropped before it leaves its hover, its look's animation and its open menu in place.
    pub fn title_id(bar: WidgetId, label: &str) -> WidgetId {
        bar.with(("menu-title", label))
    }
}

impl Configure for MenuBar<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
