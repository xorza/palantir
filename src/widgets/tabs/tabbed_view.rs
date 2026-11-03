//! A strip over a content area, bound to the caller's page index.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::theme::menu::MenuTheme;
use crate::theme::tabs::TabsTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::panel::Panel;
use crate::widgets::tabs::reindex;
use crate::widgets::tabs::tab_item::{TabItem, TabItemBuf};
use crate::widgets::tabs::tab_move::TabMove;
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use std::hash::Hash;
use std::rc::Rc;

/// What one pass over a [`TabbedView`] did, each edge on the frame it happened. Only
/// [`activated`](Self::activated) is already done when reported; `closed` and `reordered` name a
/// change to the caller's own collection, which the bound index already follows.
#[derive(Debug)]
pub struct TabbedViewResponse<'a, R> {
    /// The view's own pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// What the body returned, `None` with no page to show.
    pub inner: Option<R>,
    /// The bound index was written with another value: a pick, or a close or a move that
    /// shifted the page showing. A pick commits at once.
    pub changed: bool,
    /// The page a click or a key made visible; the bound index already holds it.
    pub activated: Option<usize>,
    /// The page whose close button was clicked: remove it from the collection. The bound index
    /// already names the page that shows next: the one still showing, else the one that takes
    /// the closed page's slot, else the one before it.
    pub closed: Option<usize>,
    /// A chip dragged onto another slot: move the page in the collection, as
    /// [`TabMove::apply`] does.
    pub reordered: Option<TabMove>,
    /// The page whose chip a drag carries, until it drops; see
    /// [`TabStripResponse::dragging`](crate::TabStripResponse::dragging).
    pub dragging: Option<usize>,
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
    menu_style: Option<&'a MenuTheme>,
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
            widget: Widget::vstack(),
            selected,
            options,
            label,
            key: idx_key::<S>,
            closable: false,
            draggable: false,
            overflow: TabOverflow::default(),
            style: None,
            menu_style: None,
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
            menu_style: self.menu_style,
        }
    }
}

impl<'a, S, L: Fn(&S) -> &str, K: Fn(usize, &S) -> u64> TabbedView<'a, S, L, K> {
    /// Whether each chip carries a close button, reported as [`TabbedViewResponse::closed`].
    /// Default `false`: the caller removes the page.
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

    /// Per-instance override of [`Theme::tabs`](crate::Theme::tabs).
    pub fn style(mut self, s: impl Into<Option<&'a TabsTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Per-instance override of [`Theme::menu`](crate::Theme::menu) for the overflow menu.
    pub fn menu_style(mut self, s: impl Into<Option<&'a MenuTheme>>) -> Self {
        self.menu_style = s.into();
        self
    }

    /// Record the strip and the page under it; `body` runs once with the visible page's index
    /// into `options` as passed, or not at all with no pages, and what it returns comes back as
    /// [`TabbedViewResponse::inner`]. A close or a move rewrites the binding for the collection
    /// after the change, so the body still reads this frame's.
    #[track_caller]
    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, usize) -> R,
    ) -> TabbedViewResponse<'_, R> {
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
            menu_style,
        } = self;
        let mut shown = domain::idx(*selected, options.len());
        widget
            .configure()
            .default_size((Sizing::FILL, Sizing::FILL));
        let response = widget.response(ui);
        let strip_id = response.id.with("strip");
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
                    .style(t)
                    .menu_style(menu_style)
                    .show(ui, |ui, chip| chip.show_label(ui));
                StripHit {
                    activated: strip.activated,
                    closed: strip.closed,
                    reordered: strip.reordered,
                    dragging: strip.dragging,
                }
            });
            let activated = hit.activated.filter(|_| hit.closed.is_none());
            let mut bound = match hit.closed {
                Some(idx) => shown.map(|page| reindex::after_removal(page, idx, options.len())),
                None => activated,
            };
            if activated.is_some() {
                shown = activated;
            }
            if let (Some(TabMove { from, to }), Some(page)) = (hit.reordered, shown) {
                bound = Some(reindex::after_move(page, from, to));
            }
            let inner = Panel::vstack()
                .id(response.id.with("content"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| shown.map(|page| body(ui, page)))
                .inner;
            ViewPass {
                activated,
                closed: hit.closed,
                reordered: hit.reordered,
                dragging: hit.dragging,
                bound,
                inner,
            }
        });
        let changed = pass.bound.is_some_and(|bound| bound != *selected);
        if let Some(bound) = pass.bound {
            *selected = bound;
        }
        TabbedViewResponse {
            response: Response::new(ui, response),
            inner: pass.inner,
            changed,
            activated: pass.activated,
            closed: pass.closed,
            reordered: pass.reordered,
            dragging: pass.dragging,
        }
    }
}

impl<S, L, K> Configure for TabbedView<'_, S, L, K> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// What one pass over the strip did, the index the binding takes after it, and what the body
/// returned.
#[derive(Debug)]
struct ViewPass<R> {
    activated: Option<usize>,
    closed: Option<usize>,
    reordered: Option<TabMove>,
    dragging: Option<usize>,
    bound: Option<usize>,
    inner: Option<R>,
}

#[derive(Debug)]
struct StripHit {
    activated: Option<usize>,
    closed: Option<usize>,
    reordered: Option<TabMove>,
    dragging: Option<usize>,
}

const fn idx_key<S>(idx: usize, _: &S) -> u64 {
    idx as u64
}
