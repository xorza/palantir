//! A strip over a content area, bound to the caller's page index.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::theme::tabs::TabsTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::panel::Panel;
use crate::widgets::tabs::tab_item::{TabBadge, TabItem, TabItemBuf};
use crate::widgets::tabs::tab_strip;
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use std::hash::Hash;
use std::rc::Rc;

/// What one pass over a [`TabbedView`] asks its caller to do. Only
/// [`Self::Activated`] is already done when reported; the other two name a change
/// to the caller's own collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabsAction {
    /// The visible page changed; the bound index already holds `idx`.
    Activated {
        /// The page now visible.
        idx: usize,
    },
    /// The page's close button was clicked: remove it and re-derive the bound
    /// index.
    Closed {
        /// The page to remove.
        idx: usize,
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
/// # fn color(_: &mut Ui) {}
/// # fn geometry(_: &mut Ui) {}
/// # fn metadata(_: &mut Ui) {}
/// # fn demo(ui: &mut Ui, page: &mut usize) {
/// TabbedView::new(page, &["Color", "Geometry", "Metadata"])
///     .closable(false)
///     .show(ui, |ui, page| match page {
///         0 => color(ui),
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
            key: idx_key::<S>,
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
        let mut shown = domain::idx(*selected, options.len());
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
            if let Some(idx) = hit.closed {
                action = Some(TabsAction::Closed { idx });
            } else if let Some(idx) = hit.clicked {
                *selected = idx;
                shown = Some(idx);
                action = Some(TabsAction::Activated { idx });
            }
            if let Some(from) = hit.drag_stopped
                && reorderable
                && let Some(to) = dropped_slot(
                    ui,
                    strip_id,
                    options
                        .iter()
                        .enumerate()
                        .map(|(slot, page)| key(slot, page)),
                )
                && to != from
                && to != from + 1
                && let Some(page) = shown
            {
                *selected = moved_idx(page, from, to);
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
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[derive(Debug)]
struct StripHit {
    clicked: Option<usize>,
    closed: Option<usize>,
    drag_stopped: Option<usize>,
}

/// The slot the pointer released over, from last frame's rects of the chips keyed
/// `keys`; `None` unless over the strip.
fn dropped_slot(
    ui: &mut Ui,
    strip: WidgetId,
    keys: impl IntoIterator<Item = u64>,
) -> Option<usize> {
    let pointer = ui.pointer_pos()?;
    if !ui.response_for(strip).rect?.contains(pointer) {
        return None;
    }
    let chips = keys
        .into_iter()
        .filter_map(|key| ui.response_for(TabStrip::chip_id(strip, key)).rect);
    Some(tab_strip::insertion_slot(chips, pointer.x))
}

const fn idx_key<S>(idx: usize, _: &S) -> u64 {
    idx as u64
}

/// Where the page at `idx` sits after [`TabsAction::Reordered`] moves `from` into
/// gap `to`.
const fn moved_idx(idx: usize, from: usize, to: usize) -> usize {
    let landing = if to > from { to - 1 } else { to };
    if idx == from {
        return landing;
    }
    let without = if idx > from { idx - 1 } else { idx };
    if without >= landing {
        without + 1
    } else {
        without
    }
}

#[cfg(test)]
pub(crate) mod internals {
    pub(crate) const fn moved_idx(idx: usize, from: usize, to: usize) -> usize {
        super::moved_idx(idx, from, to)
    }
}
