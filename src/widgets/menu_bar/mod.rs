//! The application menu bar: a row of titles, each opening its menu, one open at a time.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::text::text_input::TextInput;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::theme::Theme;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::menu_bar::{MenuBarTheme, MenuState};
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::{Configure, ThemeDefaults};
use crate::widget_core::response::{InnerResponse, Response};
use crate::widget_core::widget::Widget;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::popup::Popup;
use crate::widgets::popup::trigger_state::{Opening, TriggerFrame, TriggerState};
use glam::Vec2;
use std::rc::Rc;

/// A row of menu titles, each opening its [`MenuItem`](crate::MenuItem) rows below it, and one
/// menu open at a time, as a desktop application's menu bar.
///
/// A click on a title opens its menu and a click on the open title shuts it. While a menu is
/// open the bar tracks the pointer over its titles, as Qt's `QMenuBar` maps a move to a title
/// with `actionAt`: moving onto another title opens that one's menu, and a click on one opens it
/// or, on the open title, shuts it. Left and Right move to the neighboring menu, wrapping, and
/// walk the titles while one holds focus, as the
/// [WAI-ARIA menubar](https://www.w3.org/WAI/ARIA/apg/patterns/menubar/) asks. A menu closes
/// as a [`ContextMenu`](crate::ContextMenu) does: on Esc, a click outside, or a picked row.
///
/// ```
/// # use palantir::{MenuBar, MenuItem, Ui};
/// # fn demo(ui: &mut Ui) {
/// MenuBar::new().show(ui, |bar| {
///     bar.menu("File", |ui, close| {
///         if MenuItem::new("Quit").show(ui, close).clicked() {
///             // …
///         }
///     });
///     bar.menu("Edit", |ui, close| {
///         MenuItem::new("Undo").show(ui, close);
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
}

/// What a [`MenuBar`] keeps between frames: how many menus it recorded, the one menu open, and
/// where the pointer was at the end of a frame with one open, so only a move onto another title
/// switches it. One slot for the open menu, so two cannot be open.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct MenuBarRow {
    count: u16,
    open: Option<OpenMenu>,
    pointer: Option<Vec2>,
}

/// The bar's open menu: its index and the trigger state its popup settles.
#[derive(Clone, Copy, Debug, PartialEq)]
struct OpenMenu {
    idx: u16,
    trigger: TriggerState<MenuBar<'static>>,
}

impl OpenMenu {
    /// Menu `idx`, opening below its title.
    const fn below(idx: u16) -> Self {
        Self {
            idx,
            trigger: TriggerState::opened(Opening::Below),
        }
    }
}

/// Where the bar sends its open menu this frame, settled before any title records.
#[derive(Clone, Copy, Debug)]
enum Switch {
    Keep,
    Shut,
    Open(u16),
}

impl<'a> MenuBar<'a> {
    /// A menu bar.
    #[track_caller]
    pub fn new() -> Self {
        Self {
            widget: Widget::hstack().arrow_focus(Axis::X),
            style: None,
        }
    }

    /// Per-instance override of [`Theme::menu_bar`](crate::Theme::menu_bar).
    pub fn style(mut self, s: impl Into<Option<&'a MenuBarTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the bar and the menus `body` adds through [`MenuBarMenus::menu`].
    pub fn show<R>(
        mut self,
        ui: &mut Ui,
        body: impl FnOnce(&mut MenuBarMenus<'_>) -> R,
    ) -> InnerResponse<'_, R> {
        let bar = self.widget.resolve(ui);
        let row = ui.state::<MenuBarRow>(bar).copied().unwrap_or_default();
        let mut open = row.open;
        if let Some(menu) = open {
            match switch(ui, bar, row, menu.idx) {
                Switch::Keep => {}
                Switch::Shut => {
                    menu.trigger.shut(ui);
                    open = None;
                }
                Switch::Open(next) => {
                    menu.trigger.shut(ui);
                    open = Some(OpenMenu::below(next));
                    // A title holding focus follows the open menu, which follows a focused title.
                    if focused_title(ui, bar, row.count).is_some() {
                        ui.set_focus(title_id(bar, next));
                    }
                }
            }
        }

        let theme = Rc::clone(ui.theme());
        let style = self.style.unwrap_or(&theme.menu_bar);
        self.widget = self.widget.default_size((Sizing::FILL, Sizing::HUG));
        self.widget
            .configure()
            .default_gap(style.gap)
            .default_padding(style.bar_padding);
        let (inner, count, mut open, step) = self.widget.record(ui, Some(&style.bar), |ui| {
            let mut menus = MenuBarMenus {
                ui,
                bar,
                theme: &theme,
                style,
                count: 0,
                open,
                step: 0,
            };
            let inner = body(&mut menus);
            (inner, menus.count, menus.open, menus.step)
        });
        // A menu whose title this frame did not record, as the bar lost it, shuts.
        if let Some(menu) = open
            && menu.idx >= count
        {
            menu.trigger.shut(ui);
            open = None;
        }
        if step != 0
            && let Some(menu) = open
        {
            menu.trigger.shut(ui);
            let to = (i32::from(menu.idx) + i32::from(step)).rem_euclid(i32::from(count));
            open = Some(OpenMenu::below(
                u16::try_from(to).expect("a menu index fits the count"),
            ));
            ui.request_repaint();
        }
        // Read only while a menu is open, as the read wakes a frame on every pointer move.
        let pointer = open.and_then(|_| ui.pointer_pos());
        let next = MenuBarRow {
            count,
            open,
            pointer,
        };
        if next != row {
            ui.with_state::<MenuBarRow, _>(bar, |_, row| *row = next);
        }
        InnerResponse {
            response: Response::lazy(bar, ui),
            inner,
        }
    }
}

/// The title holding focus among the bar's `count`, if one does.
fn focused_title(ui: &Ui, bar: WidgetId, count: u16) -> Option<u16> {
    let focus = ui.focus()?;
    (0..count).find(|&idx| title_id(bar, idx) == focus)
}

/// Where the open menu `open` goes. Focus moved to another title, as the arrows walk the titles,
/// opens that one's menu. Then the pointer over the titles: a click on the open title shuts it
/// and one on another title opens that one, both landing on the open menu's click-eater; a move
/// onto another title opens it. A pointer that rests where it was when the menu opened, as one
/// the keyboard opened under it, moves nothing.
fn switch(ui: &mut Ui, bar: WidgetId, row: MenuBarRow, open: u16) -> Switch {
    if let Some(focused) = focused_title(ui, bar, row.count)
        && focused != open
    {
        return Switch::Open(focused);
    }
    let pointer = ui.pointer_pos();
    let Some(at) = pointer else {
        return Switch::Keep;
    };
    let Some(over) = (0..row.count).find(|&idx| {
        ui.response_for(title_id(bar, idx))
            .rect
            .is_some_and(|rect| rect.contains(at))
    }) else {
        return Switch::Keep;
    };
    let eater = Popup::eater_id(menu_id(title_id(bar, open)));
    if ui.response_for(eater).left.clicked() {
        if over == open {
            Switch::Shut
        } else {
            Switch::Open(over)
        }
    } else if over != open && row.pointer.is_some_and(|was| was != at) {
        Switch::Open(over)
    } else {
        Switch::Keep
    }
}

/// The title of the bar's menu `idx`.
fn title_id(bar: WidgetId, idx: u16) -> WidgetId {
    bar.with(("menu-title", idx))
}

/// The menu a title opens.
fn menu_id(title: WidgetId) -> WidgetId {
    title.with("menu")
}

/// What [`MenuBar::show`] hands its body: the bar's titles, added in order by [`Self::menu`].
#[derive(Debug)]
pub struct MenuBarMenus<'u> {
    ui: &'u mut Ui,
    bar: WidgetId,
    theme: &'u Theme,
    style: &'u MenuBarTheme,
    count: u16,
    open: Option<OpenMenu>,
    /// Left (−1) or Right (+1) pressed in the open menu, else 0.
    step: i8,
}

impl MenuBarMenus<'_> {
    /// Add a title reading `label`, and while its menu is open record `body` in it, which gets
    /// the menu's [`CloseHandle`] for its [`MenuItem`](crate::MenuItem)s. Returns what `body`
    /// returned, `None` while the menu is shut.
    pub fn menu<'l, R>(
        &mut self,
        label: impl Into<TextInput<'l>>,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> Option<R> {
        let ui = &mut *self.ui;
        let idx = self.count;
        self.count += 1;
        let title = title_id(self.bar, idx);
        let mut widget = Widget::leaf().id(title).sense(Sense::CLICK).focusable(true);
        let response = widget.activation_response(ui, Activation::Button);
        let here = self.open.filter(|open| open.idx == idx);
        let state = if here.is_some() {
            MenuState::Open
        } else {
            MenuState::Shut
        };
        let plan = self.style.plan(&response.state, state, self.theme.text);
        let look = widget.wear(ui, plan);
        let label = ui.intern(label.into());
        widget.record(ui, Some(&look.chrome), |ui| {
            ui.add_shape(Shape::text(label, look.text.font()).color(look.text.color));
        });

        let popup = Popup::new(Anchor::below(Rect::ZERO))
            .id(menu_id(title))
            .layer(Layer::Menu)
            .menu_look(&self.theme.context_menu);
        let mut step = 0;
        let settled = TriggerFrame {
            for_id: title,
            toggled: response.state.clicked(),
            opened: None,
            disabled: response.state.disabled,
            rect: response.state.rect,
        }
        .settle(
            ui,
            here.map_or_else(TriggerState::default, |open| open.trigger),
            popup,
            |ui, close| {
                let inner = body(ui, close);
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
            if let Some(other) = self.open.filter(|open| open.idx != idx) {
                other.trigger.shut(ui);
            }
            self.open = Some(OpenMenu {
                idx,
                trigger: settled.state,
            });
            self.step = step;
        } else if here.is_some() {
            self.open = None;
        }
        settled.response.inner
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
