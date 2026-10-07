//! What the application answers about each tab, and the menu bundle handed to one answer.

use crate::primitives::geometry::size::Size;

use crate::icons::icon_set::IconHandle;
use crate::primitives::text::interned_str::InternedStr;
use crate::ui::Ui;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::dock::dock_operation::DockOperation;
use crate::widgets::dock::dock_tab::DockTab;
use crate::widgets::dock::tab_group::TabGroupId;
use crate::widgets::tabs::tab_item::TabBadge;

/// What a [`DockView`](crate::DockView) asks the application about each tab it draws. A trait, since six questions per tab do not fit in closures without boxing per frame; each runs per visible tab per frame, so keep them cheap.
pub trait DockTabs {
    /// The application's own tab key — a small `Copy` enum or index.
    type Tab: DockTab;

    /// The chip's label; intern through [`Ui::intern`] or [`fmt!`](crate::fmt).
    fn title(&mut self, ui: &mut Ui, tab: Self::Tab) -> InternedStr;

    /// The tab's body, recorded into the pane's content area; `size` is its arranged size, `None` on the one frame before the pane's first layout.
    fn content(&mut self, ui: &mut Ui, tab: Self::Tab, size: Option<Size>);

    /// Whether the chip carries a close button; the pinned tab refuses it regardless.
    fn closable(&mut self, _tab: Self::Tab) -> bool {
        true
    }

    /// Whether `tab` can be dragged.
    fn draggable(&mut self, _tab: Self::Tab) -> bool {
        true
    }

    /// The chip's status dot. Return [`TabBadge::Idle`] from every tab kind that can ever show one, so the chip keeps its width.
    fn badge(&mut self, _tab: Self::Tab) -> TabBadge {
        TabBadge::None
    }

    /// Icon for `tab`.
    fn icon(&mut self, _tab: Self::Tab) -> Option<IconHandle> {
        None
    }

    /// The chip's right-click menu: records [`MenuItem`](crate::MenuItem)s into the open menu; empty means none.
    fn tab_menu(&mut self, _ui: &mut Ui, _menu: DockTabMenu<'_, Self::Tab>) {}
}

/// What [`DockTabs::tab_menu`] is handed: the right-clicked chip, its pane, the operation sink and the menu's dismiss handle.
#[derive(Debug)]
pub struct DockTabMenu<'a, T> {
    /// The right-clicked tab.
    pub tab: T,
    /// Its pane.
    pub group: TabGroupId,
    /// Where an item's operation goes; drained by the application's queue or [`DockView::run`](crate::DockView::run).
    pub operations: &'a mut Vec<DockOperation<T>>,
    /// Pass to [`MenuItem::show`](crate::MenuItem::show).
    pub close: &'a CloseHandle,
}
