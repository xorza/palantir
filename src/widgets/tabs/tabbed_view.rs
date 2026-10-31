//! A strip over a content area, bound to the caller's page index.

use crate::input::keyboard::key::Key;
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
use crate::widgets::tabs::reindex;
use crate::widgets::tabs::tab_item::{TabItem, TabItemBuf};
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use std::hash::Hash;
use std::rc::Rc;

/// What one pass over a [`TabbedView`] did, each edge on the frame it happened. Only
/// [`activated`](Self::activated) is already done when reported; `closed` and `reordered` name a
/// change to the caller's own collection, which the bound index already follows.
#[derive(Debug)]
pub struct TabbedViewResponse<'a> {
    /// The view's own pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// The bound index was written with another value: a pick, or a close or a move that
    /// shifted the page showing.
    pub changed: bool,
    /// A discrete pick commits at once: always [`Self::changed`].
    pub committed: bool,
    /// The page a click or a key made visible; the bound index already holds it.
    pub activated: Option<usize>,
    /// The page whose close button was clicked: remove it from the collection. The bound index
    /// already names the page that shows next: the one still showing, else the one that takes
    /// the closed page's slot, else the one before it.
    pub closed: Option<usize>,
    /// A chip dragged onto another slot: move the page in the collection.
    pub reordered: Option<TabMove>,
}

/// A page a drag moved: move `from` to `to` in the option collection. `to` addresses it
/// **before** the move (`len` appends), so the page lands at `to - 1` when `to > from`; the bound
/// index already follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabMove {
    /// Where the chip came from.
    pub from: usize,
    /// Where it lands, addressing the collection before the move.
    pub to: usize,
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
/// an empty list none) and is rewritten only when the user picks, closes or drags.
/// A drag shows the strip's insertion caret, and Escape cancels it. Chips are keyed
/// by index unless [`keyed`](Self::keyed) names a key per page, which a list that
/// closes or reorders pages needs.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct TabbedView<'a, S, L, K = fn(usize, &S) -> u64> {
    widget: Widget,
    selected: &'a mut usize,
    options: &'a [S],
    label: L,
    key: K,
    closable: bool,
    draggable: bool,
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
            draggable: false,
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
            draggable: self.draggable,
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
    /// [`TabbedViewResponse::reordered`]. Default `false`: the caller performs the move.
    pub const fn draggable(mut self, draggable: bool) -> Self {
        self.draggable = draggable;
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
    /// page's index into `options` as passed, or not at all with no pages. A close or a
    /// move rewrites the binding for the collection after the change, so the body
    /// still reads this frame's.
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
            draggable,
            overflow,
            style: _,
        } = self;
        let mut shown = domain::idx(*selected, options.len());
        let response = widget.response(ui);
        let strip_id = response.id.with("strip");
        let page_keys = || {
            options
                .iter()
                .enumerate()
                .map(|(slot, page)| key(slot, page))
        };
        let was_carried = ui.state::<CarriedChip>(response.id).and_then(|c| c.key);
        let mut carried = was_carried;
        if carried.is_some() && ui.shortcut_pressed(Key::Escape) {
            carried = None;
        }
        let drop = carried.and_then(|_| dropped_slot(ui, strip_id, page_keys()));
        let pass = widget.record(ui, None, |ui| {
            let hit = ui.with_state::<TabItemBuf, _>(strip_id, |ui, buf| {
                buf.items.clear();
                buf.items.reserve_exact(options.len());
                for (i, option) in options.iter().enumerate() {
                    let text = ui.intern(label(option));
                    buf.items.push(TabItem {
                        key: key(i, option),
                        label: text,
                        closable,
                        draggable,
                    });
                }
                let strip = TabStrip::new(&buf.items)
                    .id(strip_id)
                    .selected(shown)
                    .overflow(overflow)
                    .drop_caret(drop)
                    .style(t)
                    .show(ui, |ui, chip| chip.show_label(ui));
                StripHit {
                    // The view owns its selection, so every way of asking for a tab
                    // is one request.
                    clicked: strip.activated(),
                    closed: strip.closed,
                    drag_started: strip.drag_started,
                    drag_stopped: strip.drag_stopped,
                }
            });
            let mut pass = ViewPass {
                closed: hit.closed,
                activated: hit.clicked.filter(|_| hit.closed.is_none()),
                ..ViewPass::default()
            };
            pass.bound = match pass.closed {
                Some(idx) => shown.map(|page| reindex::after_removal(page, idx, options.len())),
                None => pass.activated,
            };
            if pass.activated.is_some() {
                shown = pass.activated;
            }
            if let Some(slot) = hit.drag_started {
                carried = page_keys().nth(slot);
            }
            if let Some(from) = hit.drag_stopped {
                let carried_from = carried.is_some() && carried == page_keys().nth(from);
                carried = None;
                if draggable
                    && carried_from
                    && let Some(to) = drop
                    && to != from
                    && to != from + 1
                    && let Some(page) = shown
                {
                    pass.bound = Some(reindex::after_move(page, from, to));
                    pass.reordered = Some(TabMove { from, to });
                }
            }
            // Written only on a change, so a view nobody drags mints no row.
            if carried != was_carried {
                ui.with_state::<CarriedChip, _>(response.id, |_, c| c.key = carried);
            }
            Panel::vstack()
                .id(response.id.with("content"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    if let Some(page) = shown {
                        body(ui, page);
                    }
                });
            pass
        });
        let changed = pass.bound.is_some_and(|bound| bound != *selected);
        if let Some(bound) = pass.bound {
            *selected = bound;
        }
        TabbedViewResponse {
            response: Response::new(ui, response),
            changed,
            committed: changed,
            activated: pass.activated,
            closed: pass.closed,
            reordered: pass.reordered,
        }
    }
}

impl<S, L, K> Configure for TabbedView<'_, S, L, K> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// What one pass over the strip did, and the index the binding takes after it.
#[derive(Debug, Default)]
struct ViewPass {
    activated: Option<usize>,
    closed: Option<usize>,
    reordered: Option<TabMove>,
    bound: Option<usize>,
}

#[derive(Debug)]
struct StripHit {
    clicked: Option<usize>,
    closed: Option<usize>,
    drag_started: Option<usize>,
    drag_stopped: Option<usize>,
}

/// The chip a drag is carrying, by key, kept on the view's id until the release or an Escape.
#[derive(Debug, Default)]
struct CarriedChip {
    key: Option<u64>,
}

/// The slot the pointer is over, from last frame's extents of the chips keyed `keys`;
/// `None` unless over the strip.
fn dropped_slot(
    ui: &mut Ui,
    strip: WidgetId,
    keys: impl IntoIterator<Item = u64>,
) -> Option<usize> {
    let pointer = ui.pointer_pos()?;
    if !ui.response_for(strip).rect?.contains(pointer) {
        return None;
    }
    Some(TabStrip::drop_slot(ui, strip, keys, pointer.x))
}

const fn idx_key<S>(idx: usize, _: &S) -> u64 {
    idx as u64
}
