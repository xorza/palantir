//! One activatable row inside a context menu.

use crate::input::interaction::button_phase::ButtonPhase;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::primitives::layout::align::{Align, HAlign};
use crate::primitives::layout::justify::Justify;
use crate::primitives::text::text_input::TextInput;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::theme_slot::ThemeSlot;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::text::Text;
use crate::widgets::theme::context_menu::menu_item::MenuItemTheme;

/// One row inside a [`ContextMenu`](crate::widgets::context_menu::ContextMenu): label left, optional right-aligned shortcut hint. Clicking calls [`CloseHandle::close`]. A bound [`Self::shortcut`] synthesizes a click and closes the menu (disabled rows don't intercept).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct MenuItem<'a> {
    widget: Widget,
    label: TextInput<'a>,
    shortcut: MenuShortcut,
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
            // A Tab stop that a focused Enter or Space activates, as WAI-ARIA's menu item does.
            widget: Widget::hstack()
                .sense(Sense::CLICK)
                .focusable(true)
                .input_scope(KeyFilter::TEXT),
            label: label.into(),
            shortcut: MenuShortcut::None,
            style: None,
        }
    }

    /// Per-instance override of `context_menu.item`; takes an `Option` as readily as a reference.
    pub fn style(mut self, s: impl Into<Option<&'a MenuItemTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Attach a keyboard shortcut: renders the platform-native hint (`⌘C` / `Ctrl+C`) and intercepts that keypress while the menu is open.
    pub const fn shortcut(mut self, s: Shortcut) -> Self {
        self.shortcut = MenuShortcut::Activate(s);
        self
    }

    /// Show `shortcut` as the hint without binding it, for a chord something else already handles.
    pub const fn shortcut_hint(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = MenuShortcut::Hint(shortcut);
        self
    }

    /// Record the row inside an open menu; activating it closes the menu through `popup`.
    pub fn show<'ui>(mut self, ui: &'ui mut Ui, popup: &CloseHandle) -> Response<'ui> {
        // Single `response_for` probe: the body is decorative, so the response is identical before and after the node records.
        let mut response = self.widget.response(ui);
        let id = self.widget.resolve(ui);
        let disabled = response.disabled;

        // The look plan comes off one borrow of the theme that ends before `apply` reborrows `ui`; a menu row picks and animates like a Button.
        let theme = ui.theme();
        let item = self.style.unwrap_or(&theme.context_menu.item);
        let shortcut_color = item.shortcut;
        let gap = item.gap;
        let look = item
            .plan(&response, (), theme.text)
            .apply(ui, &mut self.widget);
        // Already fallen back to `theme.text` by `WidgetLook::animate`.
        let text_style = look.text;

        // Hug+Stretch+SpaceBetween: the row hugs content, arrange stretches it to the widest row, label and shortcut pin to opposite edges (Fill would leak INF).
        self.widget
            .configure()
            .align(Align::h(HAlign::Stretch))
            .justify(Justify::SpaceBetween)
            .gap(gap);

        let label = ui.intern(self.label);
        // Passive hints watch for wake-up while their parent owns dispatch.
        let mut shortcut_fired = false;
        let shortcut = match self.shortcut {
            MenuShortcut::None => None,
            MenuShortcut::Hint(shortcut) => {
                ui.watch_key(shortcut);
                Some(shortcut)
            }
            MenuShortcut::Activate(shortcut) => {
                shortcut_fired = !disabled && ui.key_pressed(shortcut);
                Some(shortcut)
            }
        };
        if !disabled && ui.is_focus_within(id) {
            // Both sampled: `key_pressed` also keeps each chord subscribed
            // for the wake gate.
            let enter = self.widget.key_pressed(ui, Shortcut::key(Key::Enter));
            let space = self.widget.key_pressed(ui, Shortcut::key(Key::Char(' ')));
            shortcut_fired |= enter || space;
        }
        let shortcut_label = shortcut.map(|s| ui.fmt(format_args!("{s}")));

        // Label and optional shortcut hint as `Text` leaves, both hugging their content.
        let body = |ui: &mut Ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(&text_style)
                .show(ui);
            if let Some(s) = shortcut_label {
                Text::new(s)
                    .id(id.with("shortcut"))
                    .style(&text_style)
                    .color(shortcut_color)
                    .show(ui);
            }
        };
        self.widget.record(ui, Some(&look.background), body);

        // A shortcut or activation key is a click the pointer pipeline never saw; callers must not care which device produced it.
        if shortcut_fired {
            response.left.phase = ButtonPhase::Up { click: Some(1) };
        }
        // Eager: `response` folds in the synthesized shortcut click, which
        // a lazy re-probe would drop.
        let resp = Response::new(id, ui, response);
        if resp.clicked() {
            popup.close();
        }
        resp
    }
}

impl Configure for MenuItem<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}
