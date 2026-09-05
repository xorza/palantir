//! A strip over a content area, bound to the caller's page index.

use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::response::Response;
use crate::widgets::tabs::tab_item::{TabBadge, TabItem, TabItemBuf};
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip, TabStripResponse};
use crate::widgets::theme::tabs::TabsTheme;
use crate::widgets::widget::Widget;
use std::rc::Rc;

/// What one pass over a [`TabbedView`] asks its caller to do.
///
/// Only [`Self::Activated`] is already done when it is reported — the
/// view owns the selection and has written it. The other two name a
/// change to the caller's own collection, which the view cannot make
/// through the shared slice it was handed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabsAction {
    /// The visible page changed. The bound index already holds `index`.
    Activated { index: usize },
    /// The page's close button was clicked. Remove it from the option
    /// collection, and re-derive the bound index alongside it.
    Closed { index: usize },
    /// A chip was dragged onto another slot. Move `from` to `to` in the
    /// option collection; `to` addresses the collection **as it is now**,
    /// before the move.
    Reordered { from: usize, to: usize },
}

/// A [`TabbedView`]'s pass: the view's own response, and at most one
/// [`TabsAction`].
#[derive(Debug)]
pub struct TabbedViewResponse<'a> {
    pub response: Response<'a>,
    pub action: Option<TabsAction>,
}

/// A tab strip over a content area, bound to a `&mut usize` page index.
///
/// Mirrors [`ComboBox`](crate::ComboBox) exactly: the same value
/// binding, the same `&[S: AsRef<str>]` option slice, and the same
/// [`labeled`](Self::labeled) escape for rows that merely *carry* a
/// label. A dialog with three pages should not have to implement a
/// trait; a docked pane tree should, which is what
/// [`DockView`](crate::DockView) is for.
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
/// **`*selected` must index `options`.** Showing the current page is the
/// view's whole contract and there is no empty state, so an out-of-range
/// index — including any index into an empty list — is a caller bug and
/// panics. A caller whose option list shrinks between frames owns
/// re-deriving the index alongside it.
#[derive(Debug)]
pub struct TabbedView<'a, S> {
    widget: Widget,
    selected: &'a mut usize,
    options: &'a [S],
    /// Reads one option's label. `new` fills this with `S::as_ref`.
    label: fn(&S) -> &str,
    closable: bool,
    reorderable: bool,
    overflow: TabOverflow,
    style: Option<&'a TabsTheme>,
}

impl<'a, S: AsRef<str>> TabbedView<'a, S> {
    /// A tabbed view over pages that are themselves named by text.
    #[track_caller]
    pub fn new(selected: &'a mut usize, options: &'a [S]) -> Self {
        Self::labeled(selected, options, S::as_ref)
    }
}

impl<'a, S> TabbedView<'a, S> {
    /// A tabbed view over rows that *carry* a label rather than being
    /// one: `label` reads each row's text.
    ///
    /// A plain `fn` pointer rather than a closure keeps `TabbedView`
    /// non-generic over the projection; every real label is a field
    /// read.
    #[track_caller]
    pub fn labeled(selected: &'a mut usize, options: &'a [S], label: fn(&S) -> &str) -> Self {
        Self {
            widget: Widget::vstack().size((Sizing::FILL, Sizing::FILL)),
            selected,
            options,
            label,
            closable: true,
            reorderable: false,
            overflow: TabOverflow::default(),
            style: None,
        }
    }

    /// Whether each chip carries a close button. Default `true`; a view
    /// over a fixed set of pages passes `false`.
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// Whether a chip may be dragged onto another slot, reported as
    /// [`TabsAction::Reordered`]. Default `false` — the view holds a
    /// shared slice and cannot perform the move itself, so it is the
    /// caller who opts in to receiving one.
    pub fn reorderable(mut self, reorderable: bool) -> Self {
        self.reorderable = reorderable;
        self
    }

    /// What the strip does with chips that do not fit. Default
    /// [`TabOverflow::Scroll`].
    pub fn overflow(mut self, overflow: TabOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    style_setter!('a, TabsTheme, tabs);

    /// Record the strip and the page under it. `body` is called once,
    /// with the visible page's index.
    #[track_caller]
    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, usize)) -> TabbedViewResponse<'_> {
        let theme = Rc::clone(ui.theme());
        let t = self.slot(&theme);
        let Self {
            mut widget,
            selected,
            options,
            label,
            closable,
            reorderable,
            overflow,
            style: _,
        } = self;
        assert!(
            *selected < options.len(),
            "TabbedView selection {} is out of range for {} page(s)",
            *selected,
            options.len(),
        );
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
                        key: i as u64,
                        label: text,
                        closable,
                        draggable: reorderable,
                        badge: TabBadge::None,
                        icon: None,
                    });
                }
                let TabStripResponse {
                    clicked,
                    keyed,
                    closed,
                    drag_stopped,
                    response: _,
                    drag_started: _,
                } = TabStrip::new(&buf.items)
                    .id(strip_id)
                    .selected(*selected)
                    .overflow(overflow)
                    .style(t)
                    .show(ui);
                StripHit {
                    // A tabbed view owns its selection outright, so a
                    // keyboard move and a click are the same request.
                    clicked: clicked.or(keyed),
                    closed,
                    drag_stopped,
                }
            });
            if let Some(index) = hit.closed {
                action = Some(TabsAction::Closed { index });
            } else if let Some(index) = hit.clicked {
                *selected = index;
                action = Some(TabsAction::Activated { index });
            }
            if let Some(from) = hit.drag_stopped
                && reorderable
                && let Some(to) = dropped_slot(ui, strip_id, options.len())
                && to != from
            {
                action = Some(TabsAction::Reordered { from, to });
            }
            Panel::vstack()
                .id(id.with("content"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| body(ui, *selected));
        });
        TabbedViewResponse {
            response: Response::eager(id, ui, response),
            action,
        }
    }
}

impl_configure!(<S> TabbedView<'_, S>);

/// The three edges [`TabbedView`] reads back out of its strip, carried
/// past the state scope the strip was recorded inside.
#[derive(Debug)]
struct StripHit {
    clicked: Option<usize>,
    closed: Option<usize>,
    drag_stopped: Option<usize>,
}

/// The slot the pointer released over, read straight out of last
/// frame's chip rects — no buffer, because a release happens once per
/// gesture rather than once per frame.
fn dropped_slot(ui: &mut Ui, strip: WidgetId, len: usize) -> Option<usize> {
    let x = ui.pointer_pos()?.x;
    let chips =
        (0..len).filter_map(|slot| ui.response_for(TabStrip::chip_id(strip, slot as u64)).rect);
    Some(TabStrip::insertion_slot(chips, x))
}
