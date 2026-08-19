//! The rule a context menu draws between groups of rows.

use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::separator::Separator;
use crate::widgets::theme::separator::SeparatorTheme;

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
    style: Option<&'a SeparatorTheme>,
}

impl<'a> MenuSeparator<'a> {
    /// An unstyled rule. The public way to one is
    /// [`MenuItem::separator`](crate::widgets::context_menu::menu_item::MenuItem::separator),
    /// which is where the rule reads as part of the menu vocabulary.
    pub(super) const fn new() -> Self {
        Self { style: None }
    }

    style_setter!('a, SeparatorTheme, context_menu.separator);

    #[track_caller]
    pub fn show<'ui>(self, ui: &'ui mut Ui) -> Response<'ui> {
        // Handle, not a borrow: `Separator::style` holds the reference
        // across `show`'s `&mut Ui`, and this one may point into the
        // `Ui`'s own theme.
        let ui_theme = ui.theme().clone();
        Separator::horizontal().style(self.slot(&ui_theme)).show(ui)
    }
}
