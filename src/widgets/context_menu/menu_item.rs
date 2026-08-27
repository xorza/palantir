//! One activatable row inside a context menu.

use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::align::{Align, HAlign};
use crate::layout::types::justify::Justify;
use crate::primitives::text_input::TextInput;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::context_menu::menu_separator::MenuSeparator;
use crate::widgets::popup::PopupHandle;
use crate::widgets::response::Response;
use crate::widgets::text::Text;
use crate::widgets::theme::context_menu::menu_item::MenuItemTheme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::theme_slot::ThemeSlot;

/// One row inside a [`ContextMenu`](crate::widgets::context_menu::ContextMenu). Label on the left, optional
/// right-aligned shortcut hint, theme-driven hover chrome. Reports
/// `Response` so callers branch on `clicked()`; the row also calls
/// [`PopupHandle::close`] on click so the parent `ContextMenu`
/// auto-closes without the caller threading response.
///
/// If [`Self::shortcut`] is set, the row also intercepts that
/// shortcut from this frame's key events: matching keypresses
/// synthesize a click (so `if item.left.clicked() { … }` fires) AND
/// close the menu, mirroring native menu behaviour. Disabled rows
/// don't intercept.
#[derive(Debug)]
pub struct MenuItem<'a> {
    node: Node,
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
    pub fn new(label: impl Into<TextInput<'a>>) -> Self {
        let mut node = Node::hstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            node,
            label: label.into(),
            shortcut: MenuShortcut::None,
            style: None,
        }
    }

    style_setter!('a, MenuItemTheme, context_menu.item);

    /// Attach a keyboard shortcut. Renders the right-aligned hint
    /// using the platform's native form (`⌘C` / `Ctrl+C`) and
    /// intercepts that keypress while the menu is open. Glyph-only
    /// hints (no modifier, e.g. `Backspace → ⌫`) are expressed as
    /// `Shortcut::new(Mods::NONE, Key::Backspace)`.
    pub fn shortcut(mut self, s: Shortcut) -> Self {
        self.shortcut = MenuShortcut::Activate(s);
        self
    }

    pub(crate) fn shortcut_hint(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = MenuShortcut::Hint(shortcut);
        self
    }

    pub fn enabled(self, e: bool) -> Self {
        self.disabled(!e)
    }

    /// Thin horizontal divider between groups — no label, no input.
    /// Chain `.show(ui)` and ignore the response. See
    /// [`MenuSeparator`].
    pub fn separator<'s>() -> MenuSeparator<'s> {
        MenuSeparator::new()
    }

    pub fn show<'ui>(self, ui: &'ui mut Ui, popup: &PopupHandle) -> Response<'ui> {
        // Single `response_for` probe via the shared entry helper: the
        // row's body records only decorative `Text` leaves, so the response
        // is identical before and after the node records.
        let mut widget = ui.widget(self.node);
        let mut response = widget.response(ui);
        let id = widget.id();
        let disabled = response.disabled;

        // Row-only scalars and the look plan come off one borrow of the row's
        // theme, which ends before `apply` reborrows `ui` mutably. Everything
        // response-varying — the four-response pick, the padding/margin
        // defaults, the transition — rides the shared plan, so a menu row
        // picks and animates exactly like a Button.
        let theme = ui.theme();
        let item = self.slot(theme);
        let shortcut_color = item.shortcut;
        let gap = item.gap;
        let look = item.plan(&response, (), &theme.text).apply(ui, &mut widget);
        // Already fallen back to `theme.text` by `WidgetLook::animate`.
        let text_style = look.text;
        // Shortcut hint reads muted — same style as the label but the
        // theme's `shortcut` color.
        let shortcut_style = TextStyle {
            color: shortcut_color,
            ..text_style.clone()
        };

        let node = &mut widget.node;
        // Hug+Stretch+SpaceBetween: row hugs content (the default
        // `Sizes` — respects an explicit `.size(...)`), arrange
        // stretches to widest row, label/shortcut pin to opposite
        // edges. Fill would leak INF.
        node.align = Align::h(HAlign::Stretch);
        node.justify = Justify::SpaceBetween;
        node.gaps.set_gap(gap);

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
        let shortcut_label = shortcut.map(|s| ui.fmt(format_args!("{s}")));

        // Label + optional right-aligned shortcut hint as `Text` leaves;
        // the row's `SpaceBetween` pins them to opposite edges. Both
        // hug their content (Text defaults to `Hug × Hug` and a
        // `SingleLine` wrap), matching what the row layout expects.
        let body = |ui: &mut Ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(&text_style)
                .show(ui);
            if let Some(s) = shortcut_label {
                Text::new(s)
                    .id(id.with("shortcut"))
                    .style(&shortcut_style)
                    .show(ui);
            }
        };
        widget.record(ui, Some(&look.background), body);

        if shortcut_fired {
            response.mark_clicked();
        }
        // Eager: `response` folds in the synthesized shortcut click, which
        // a lazy re-probe would drop.
        let resp = Response::eager(id, ui, response);
        if resp.left.clicked() {
            popup.close();
        }
        resp
    }
}

impl_configure!(MenuItem<'_>);
