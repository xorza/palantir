//! What the application answers about each tab, and the menu bundle one
//! of those answers is handed.

use glam::Vec2;

use crate::icons::icon_set::IconHandle;
use crate::primitives::interned_str::InternedStr;
use crate::ui::Ui;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::dock::dock_op::DockOp;
use crate::widgets::dock::dock_tab::DockTab;
use crate::widgets::dock::tab_group::TabGroupId;
use crate::widgets::tabs::tab_item::TabBadge;

/// What a [`DockView`](crate::DockView) asks the application about each
/// tab it is about to draw.
///
/// Two required methods and five defaulted ones. The two are exactly
/// what a closure cannot carry — what a tab is called, and what it draws
/// — and they are why the dock takes a trait where
/// [`TabbedView`](crate::TabbedView) takes a value binding: six
/// questions per tab do not fit in builder closures without boxing one
/// per frame.
///
/// The dock stores only a key, so every one of these runs once per
/// visible tab per frame. Keep them cheap: a match arm, a field read.
pub trait DockTabs {
    type Tab: DockTab;

    /// The chip's label. Intern through [`Ui::intern`] or
    /// [`fmt!`](crate::fmt) — nothing here needs to own its text.
    fn title(&mut self, ui: &mut Ui, tab: Self::Tab) -> InternedStr;

    /// The tab's body, recorded into the pane's content area.
    ///
    /// `size` is that area's arranged size, or `None` on the one frame
    /// in a pane's life that has not been laid out yet. It is the
    /// *group's* content area, which outlives the tab in it, so a view
    /// that first records on this pass is still handed a size.
    fn content(&mut self, ui: &mut Ui, tab: Self::Tab, size: Option<Vec2>);

    /// Whether the chip carries a close button. The pinned tab is
    /// refused by the model whatever this answers.
    fn closable(&mut self, _tab: Self::Tab) -> bool {
        true
    }

    /// Whether the chip may be dragged to another pane.
    fn draggable(&mut self, _tab: Self::Tab) -> bool {
        true
    }

    /// The chip's status dot. Return [`TabBadge::Idle`] from every tab
    /// kind that can *ever* show one, so the chip keeps its width when
    /// the dot goes out.
    fn badge(&mut self, _tab: Self::Tab) -> TabBadge {
        TabBadge::None
    }

    /// Artwork drawn before the label.
    fn icon(&mut self, _tab: Self::Tab) -> Option<IconHandle> {
        None
    }

    /// The chip's right-click menu. Records [`MenuItem`](crate::MenuItem)s
    /// into the open menu; an empty body means the chip has no menu.
    ///
    /// The dock ships no default items, because the wording of a split
    /// command belongs to the application, not to the widget.
    fn tab_menu(&mut self, _ui: &mut Ui, _menu: DockTabMenu<'_, Self::Tab>) {}
}

/// What [`DockTabs::tab_menu`] is handed: which chip was right-clicked,
/// the pane it sits in, the sink its items push ops onto, and the handle
/// that dismisses the menu.
///
/// A bundle rather than four parameters — `tab` and `group` are the two
/// addresses a split op is built from, and an item that reached for one
/// without the other could not name its own drop.
#[derive(Debug)]
pub struct DockTabMenu<'a, T> {
    pub tab: T,
    pub group: TabGroupId,
    /// Where an item's op goes. The application's own queue drains it,
    /// or [`DockView::run`](crate::DockView::run) does.
    pub ops: &'a mut Vec<DockOp<T>>,
    /// Pass to [`MenuItem::show`](crate::MenuItem::show).
    pub close: &'a CloseHandle,
}
