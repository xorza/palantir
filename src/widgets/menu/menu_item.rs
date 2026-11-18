//! One activatable row inside a menu.

use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::sense::Sense;
use crate::primitives::input::shortcut::Shortcut;
use crate::primitives::layout::align::{HAlign, VAlign};
use crate::primitives::layout::justify::Justify;
use crate::primitives::math::domain;
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::menu::menu_item::MenuItemTheme;
use crate::theme::selection::Selection;
use crate::theme::text_style_overrides::TextStyleOverrides;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use crate::widgets::text::Text;

/// One row inside a [`Menu`](crate::Menu): a check-mark gutter and the label left, optional right-aligned shortcut hint. Activating it closes the overlay it sits in, a menu or any popup, as egui's menu rows close their menu. A bound [`Self::shortcut`] synthesizes a click and closes the menu (disabled rows don't intercept).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct MenuItem<'a> {
    widget: Widget,
    label: TextInput<'a>,
    shortcut: MenuShortcut,
    selection: Selection,
    checked: bool,
    style: Option<&'a MenuItemTheme>,
}

#[derive(Clone, Copy, Debug)]
enum MenuShortcut {
    None,
    Hint(Shortcut),
    Activate(Shortcut),
}

impl<'a> MenuItem<'a> {
    #[track_caller]
    /// An item with `label`.
    pub fn new(label: impl Into<TextInput<'a>>) -> Self {
        Self {
            widget: Widget::hstack().sense(Sense::CLICK).focusable(true),
            label: label.into(),
            shortcut: MenuShortcut::None,
            selection: Selection::Unselected,
            checked: false,
            style: None,
        }
    }

    /// Per-instance override of [`Theme::menu`](crate::Theme::menu)'s `item`.
    pub fn style(mut self, s: impl Into<Option<&'a MenuItemTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Whether the row is the pick, as the current choice of a combo list: it then wears
    /// [`MenuItemTheme::selected`] and ticks its check gutter, as a macOS menu ticks its picked
    /// item. A click reports as on any row. Default `false`.
    pub const fn selected(mut self, selected: bool) -> Self {
        self.selection = if selected {
            Selection::Selected
        } else {
            Selection::Unselected
        };
        self
    }

    /// Whether the row's setting is on, shown as a check mark in the gutter every row keeps left
    /// of its label, so the labels of a menu align whichever rows tick: a row that toggles a
    /// setting, as macOS's `NSMenuItem.state`, WPF's `MenuItem.IsChecked` and Qt's checkable
    /// `QAction` show it. A [pick](Self::selected) ticks too and also wears the picked look. The
    /// caller flips its value on a click. Default `false`.
    pub const fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Attach a keyboard shortcut: renders the platform-native hint (`⌘C` / `Ctrl+C`) and intercepts that keypress while the menu is open.
    pub const fn shortcut(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = MenuShortcut::Activate(shortcut);
        self
    }

    /// Show `shortcut` as the hint without binding it, for a chord something else already handles.
    pub const fn shortcut_hint(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = MenuShortcut::Hint(shortcut);
        self
    }

    /// Record the row; activating it closes the innermost overlay recording it.
    pub fn show(mut self, ui: &mut Ui) -> Response {
        // Single `response_for` probe: the body is decorative, so the response is identical before and after the node records.
        let mut response = self.widget.activation_response(ui, Activation::Button);
        let id = response.id;
        let disabled = response.disabled;
        // A lit row, under the pointer or pressed, sits on a fill its resting hint ink may not
        // clear, so its hint takes the label's ink, as a tab's and a menu title's text lights.
        let lit = response.hovered() || response.is_pressed();

        // The look plan comes off one borrow of the theme that ends before `wear` reborrows `ui`; a menu row picks and animates like a Button.
        let theme = ui.theme();
        let item = self.style.unwrap_or(&theme.menu.item);
        let shortcut_text = item.shortcut;
        let gap = item.gap;
        let check_gap = item.check_gap;
        let check_side = item.check_side();
        let ticked = self.checked || self.selection == Selection::Selected;
        let check = ticked.then(|| item.check_polyline());
        let check_width = domain::length_at_least(item.check_width, 0.0);
        let plan = item.plan(&response, self.selection, theme.text);
        let look = self.widget.wear(ui, plan);
        // Already fallen back to `theme.text` by `WidgetLook::animate`.
        let text_style = look.text;

        // Hug+Stretch+SpaceBetween: the row hugs content, arrange stretches it to the widest row, the lead stack and the shortcut pin to opposite edges (Fill would leak INF).
        self.widget
            .configure()
            .default_align(HAlign::Stretch)
            .default_justify(Justify::SpaceBetween)
            .default_gap(gap);

        let label = ui.intern(self.label);
        // Passive hints watch for wake-up while their parent owns dispatch.
        let mut shortcut_fired = false;
        let shortcut = match self.shortcut {
            MenuShortcut::None => None,
            MenuShortcut::Hint(shortcut) => {
                ui.watch_shortcut(shortcut);
                Some(shortcut)
            }
            MenuShortcut::Activate(shortcut) => {
                shortcut_fired = !disabled && ui.shortcut_pressed(shortcut);
                Some(shortcut)
            }
        };
        let shortcut_label = shortcut.map(|s| ui.fmt(format_args!("{s}")));

        // The gutter and label as one lead stack, so `SpaceBetween` leaves them together.
        let body = |ui: &mut Ui| {
            Widget::hstack()
                .id(id.with("lead"))
                .gap(check_gap)
                .child_align(VAlign::Center)
                .record(ui, None, |ui| {
                    Widget::leaf()
                        .id(id.with("check"))
                        .size((check_side, check_side))
                        .record(ui, None, |ui| {
                            if let Some(points) = check {
                                ui.add_shape(
                                    Shape::polyline(&points),
                                    Stroke::new(text_style.color, check_width)
                                        .with_cap(LineCap::Round)
                                        .with_join(LineJoin::Round),
                                );
                            }
                        });
                    Text::new(label)
                        .id(id.with("label"))
                        .style(&text_style)
                        .show(ui);
                });
            if let Some(s) = shortcut_label {
                // A disabled row's hint dims with its label: in the hint's own ink it would be
                // the brightest text on a row that does nothing.
                let overrides = if disabled || lit {
                    TextStyleOverrides {
                        color: None,
                        ..shortcut_text
                    }
                } else {
                    shortcut_text
                };
                Text::new(s)
                    .id(id.with("shortcut"))
                    .style(&overrides.apply(&text_style))
                    .show(ui);
            }
        };
        self.widget.record(ui, Some(&look.chrome), body);

        if shortcut_fired {
            response.activate();
        }
        // `response` folds in the synthesized shortcut click, which a probe of the id would not
        // hold.
        if response.clicked() {
            ui.close_overlay();
        }
        response
    }
}

impl Configure for MenuItem<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}
