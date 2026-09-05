//! The rule a context menu draws between groups of rows.

use crate::layout::axis::Axis;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::response::Response;
use crate::widgets::separator::Separator;
use crate::widgets::theme::separator::SeparatorTheme;
use crate::widgets::widget::Widget;
use std::rc::Rc;

/// The rule [`MenuItem::separator`](crate::widgets::context_menu::menu_item::MenuItem::separator)
/// records between menu groups: a
/// [`crate::Separator`] wearing [`crate::Theme::context_menu`]'s
/// `separator` slot instead of the app-wide `theme.separator`.
///
/// A menu rule *is* a separator — the two differ only in which
/// [`SeparatorTheme`] they read, so this hands the bundle straight down
/// rather than unpacking it field by field.
///
/// ```
/// # use palantir::{MenuItem, Ui};
/// # fn demo(ui: &mut Ui) {
/// MenuItem::separator().show(ui);
/// # }
/// ```
#[derive(Debug)]
pub struct MenuSeparator<'a> {
    widget: Widget,
    style: Option<&'a SeparatorTheme>,
}

impl<'a> MenuSeparator<'a> {
    /// An unstyled rule. The public way to one is
    /// [`MenuItem::separator`](crate::widgets::context_menu::menu_item::MenuItem::separator),
    /// which is where the rule reads as part of the menu vocabulary.
    #[track_caller]
    pub(super) fn new() -> Self {
        Self {
            widget: Widget::leaf(),
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `context_menu.separator`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a SeparatorTheme>>) -> Self {
        self.style = s.into();
        self
    }

    pub fn show<'ui>(self, ui: &'ui mut Ui) -> Response<'ui> {
        // Handle, not a borrow: `Separator::style` holds the reference
        // across `show`'s `&mut Ui`, and this one may point into the
        // `Ui`'s own theme.
        let ui_theme = Rc::clone(ui.theme());
        let style = self.style.unwrap_or(&ui_theme.context_menu.separator);
        Separator::over(self.widget, Axis::X).style(style).show(ui)
    }
}

impl Configure for MenuSeparator<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}
