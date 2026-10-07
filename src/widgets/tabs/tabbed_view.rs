//! A strip over a content area, bound to the caller's page index.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::panel::Panel;
use crate::widgets::tabs::tab_item::{TabBadge, TabItem, TabItemBuf};
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use crate::widgets::theme::tabs::TabsTheme;
use std::hash::Hash;
use std::rc::Rc;

/// What one pass over a [`TabbedView`] asks its caller to do. Only
/// [`Self::Activated`] is already done when reported; the other two name a change
/// to the caller's own collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabsAction {
    /// The visible page changed; the bound index already holds `index`.
    Activated {
        /// The page now visible.
        index: usize,
    },
    /// The page's close button was clicked: remove it and re-derive the bound
    /// index.
    Closed {
        /// The page to remove.
        index: usize,
    },
    /// A chip was dragged onto another slot: move `from` to `to` in the option
    /// collection. `to` addresses it **before** the move (`len` appends), so the
    /// page lands at `to - 1` when `to > from`; the bound index already follows.
    Reordered {
        /// Where the chip came from.
        from: usize,
        /// Where it lands, addressing the collection before the move.
        to: usize,
    },
}

/// A [`TabbedView`]'s pass: its own response and at most one [`TabsAction`].
#[derive(Debug)]
pub struct TabbedViewResponse<'a> {
    /// The view's own pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// What the strip asks the caller to do, if anything.
    pub action: Option<TabsAction>,
}

/// A tab strip over a content area, bound to a `&mut usize` page index. Mirrors
/// [`ComboBox`](crate::ComboBox) (same binding, option slice and
/// [`labeled`](Self::labeled) escape); for a docked pane tree use
/// [`DockView`](crate::DockView).
///
/// ```
/// # use palantir::{Configure, TabbedView, Ui};
/// # fn colour(_: &mut Ui) {}
/// # fn geometry(_: &mut Ui) {}
/// # fn metadata(_: &mut Ui) {}
/// # fn demo(ui: &mut Ui, page: &mut usize) {
/// TabbedView::new(page, &["Colour", "Geometry", "Metadata"])
///     .closable(false)
///     .show(ui, |ui, page| match page {
///         0 => colour(ui),
///         1 => geometry(ui),
///         _ => metadata(ui),
///     });
/// # }
/// ```
///
/// `*selected` is an *index* coerced for display (past the end shows the last page,
/// an empty list none) and is rewritten only when the user picks or drags. Chips
/// are keyed by index unless [`keyed`](Self::keyed) names a key per page, which a
/// list that closes or reorders pages needs.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct TabbedView<'a, S, L, K = fn(usize, &S) -> u64> {
    widget: Widget,
    selected: &'a mut usize,
    options: &'a [S],
    label: L,
    key: K,
    closable: bool,
    reorderable: bool,
    overflow: TabOverflow,
    style: Option<&'a TabsTheme>,
}

impl<'a, S: AsRef<str>> TabbedView<'a, S, fn(&S) -> &str> {
    /// A tabbed view over pages that are themselves named by text.
    #[track_caller]
    pub fn new(selected: &'a mut usize, options: &'a [S]) -> Self {
        Self::labeled(selected, options, S::as_ref)
    }
}

impl<'a, S, L: Fn(&S) -> &str> TabbedView<'a, S, L> {
    /// A tabbed view over rows that *carry* a label; `label` reads each row's text.
    #[track_caller]
    pub fn labeled(selected: &'a mut usize, options: &'a [S], label: L) -> Self {
        Self {
            widget: Widget::vstack().size((Sizing::FILL, Sizing::FILL)),
            selected,
            options,
            label,
            key: index_key::<S>,
            closable: true,
            reorderable: false,
            overflow: TabOverflow::default(),
            style: None,
        }
    }

    /// Key each page's chip by `key(page)` rather than by index, so hover and
    /// animation follow the page when pages close or reorder; hashed as
    /// [`DockView::tab_key`](crate::DockView::tab_key) does, keys unique.
    pub fn keyed<H: Hash>(
        self,
        key: impl Fn(&S) -> H,
    ) -> TabbedView<'a, S, L, impl Fn(usize, &S) -> u64> {
        TabbedView {
            widget: self.widget,
            selected: self.selected,
            options: self.options,
            label: self.label,
            key: move |_: usize, page: &S| WidgetId::from_hash(key(page)).0,
            closable: self.closable,
            reorderable: self.reorderable,
            overflow: self.overflow,
            style: self.style,
        }
    }
}

impl<'a, S, L: Fn(&S) -> &str, K: Fn(usize, &S) -> u64> TabbedView<'a, S, L, K> {
    /// Whether each chip carries a close button. Default `true`.
    pub const fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// Whether a chip may be dragged onto another slot, reported as
    /// [`TabsAction::Reordered`]. Default `false`: the caller performs the move.
    pub const fn reorderable(mut self, reorderable: bool) -> Self {
        self.reorderable = reorderable;
        self
    }

    /// What the strip does with chips that do not fit; default
    /// [`TabOverflow::Scroll`].
    pub const fn overflow(mut self, overflow: TabOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `tabs`.
    pub fn style(mut self, s: impl Into<Option<&'a TabsTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the strip and the page under it; `body` runs once with the visible
    /// page's index, or not at all with no pages.
    #[track_caller]
    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, usize)) -> TabbedViewResponse<'_> {
        let theme = Rc::clone(ui.theme());
        let t = self.style.unwrap_or(&theme.tabs);
        let Self {
            mut widget,
            selected,
            options,
            label,
            key,
            closable,
            reorderable,
            overflow,
            style: _,
        } = self;
        let mut shown = domain::index(*selected, options.len());
        let id = widget.resolve(ui);
        let response = widget.response(ui);
        let strip_id = id.with("strip");
        let mut action = None;
        widget.record(ui, None, |ui| {
            let hit = ui.with_state::<TabItemBuf, _>(strip_id, |ui, buf| {
                buf.items.clear();
                buf.items.reserve_exact(options.len());
                for (i, option) in options.iter().enumerate() {
                    let text = ui.intern(label(option));
                    buf.items.push(TabItem {
                        key: key(i, option),
                        label: text,
                        closable,
                        draggable: reorderable,
                        badge: TabBadge::None,
                        icon: None,
                    });
                }
                let strip = TabStrip::new(&buf.items)
                    .id(strip_id)
                    .selected(shown)
                    .overflow(overflow)
                    .style(t)
                    .show(ui);
                StripHit {
                    // The view owns its selection, so every way of asking for a tab
                    // is one request.
                    clicked: strip.activated(),
                    closed: strip.closed,
                    drag_stopped: strip.drag_stopped,
                }
            });
            if let Some(index) = hit.closed {
                action = Some(TabsAction::Closed { index });
            } else if let Some(index) = hit.clicked {
                *selected = index;
                shown = Some(index);
                action = Some(TabsAction::Activated { index });
            }
            if let Some(from) = hit.drag_stopped
                && reorderable
                && let Some(to) = dropped_slot(ui, strip_id, options, &key)
                && to != from
                && to != from + 1
                && let Some(page) = shown
            {
                *selected = moved_index(page, from, to);
                shown = Some(*selected);
                action = Some(TabsAction::Reordered { from, to });
            }
            Panel::vstack()
                .id(id.with("content"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    if let Some(page) = shown {
                        body(ui, page);
                    }
                });
        });
        TabbedViewResponse {
            response: Response::new(id, ui, response),
            action,
        }
    }
}

impl<S, L, K> Configure for TabbedView<'_, S, L, K> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[derive(Debug)]
struct StripHit {
    clicked: Option<usize>,
    closed: Option<usize>,
    drag_stopped: Option<usize>,
}

/// The slot the pointer released over, from last frame's chip rects; `None` unless
/// over the strip.
fn dropped_slot<S>(
    ui: &mut Ui,
    strip: WidgetId,
    options: &[S],
    key: impl Fn(usize, &S) -> u64,
) -> Option<usize> {
    let pointer = ui.pointer_pos()?;
    if !ui.response_for(strip).rect?.contains(pointer) {
        return None;
    }
    let x = pointer.x;
    let chips = options.iter().enumerate().filter_map(|(slot, page)| {
        ui.response_for(TabStrip::chip_id(strip, key(slot, page)))
            .rect
    });
    Some(TabStrip::insertion_slot(chips, x))
}

const fn index_key<S>(index: usize, _: &S) -> u64 {
    index as u64
}

/// Where the page at `index` sits after [`TabsAction::Reordered`] moves `from` into
/// gap `to`.
const fn moved_index(index: usize, from: usize, to: usize) -> usize {
    let landing = if to > from { to - 1 } else { to };
    if index == from {
        return landing;
    }
    let without = if index > from { index - 1 } else { index };
    if without >= landing {
        without + 1
    } else {
        without
    }
}

#[cfg(test)]
pub(crate) mod internals {
    pub(crate) const fn moved_index(index: usize, from: usize, to: usize) -> usize {
        super::moved_index(index, from, to)
    }
}
