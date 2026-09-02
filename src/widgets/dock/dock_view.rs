//! The dock widget: the split walk, one strip-over-content pane per
//! group, and the drag-docking gesture.

use crate::layout::types::sizing::Sizing;
use crate::primitives::approx;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::dock::dock_node::{DockNode, DockSplit, NodeIdx};
use crate::widgets::dock::dock_op::DockOp;
use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::dock_state::DockState;
use crate::widgets::dock::dock_tab::DockTab;
use crate::widgets::dock::dock_tabs::{DockTabMenu, DockTabs};
use crate::widgets::dock::split_side::SplitDir;
use crate::widgets::dock::tab_group::TabGroup;
use crate::widgets::panel::Panel;
use crate::widgets::response::Response;
use crate::widgets::splitter::{SplitHalf, Splitter};
use crate::widgets::tabs::tab_item::{TabItem, TabItemBuf};
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use crate::widgets::text::Text;
use crate::widgets::theme::dock::DockTheme;
use crate::window::cursor_icon::CursorIcon;
use std::rc::Rc;

/// The docked pane tree: splits onto [`Splitter`]s, leaves as a
/// [`TabStrip`] over a group-keyed content area, and the drag gesture
/// that moves a tab between them.
///
/// **Two calls, not one.** Palantir's record pass cannot see this
/// frame's layout, so a widget that learned of a tab click mid-record
/// would draw the pane the click replaced.
/// [`DockState::scan`] runs a phase earlier, the application applies
/// what it emits, and only then does this walk run — so a switch draws
/// on the frame it lands.
///
/// ```no_run
/// # use palantir::{DockOp, DockState, DockTabs, DockView, Ui};
/// # #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// # enum Tab { Main }
/// # fn demo<D: DockTabs<Tab = Tab>>(ui: &mut Ui, dock: &mut DockState<Tab>, tabs: &mut D) {
/// let mut ops: Vec<DockOp<Tab>> = Vec::new();
/// dock.scan(ui, &mut ops);
/// for op in ops.drain(..) {
///     dock.apply(op);
/// }
/// DockView::new(dock, &mut ops).min_pane(220.0).show(ui, tabs);
/// for op in ops.drain(..) {
///     dock.apply(op);
/// }
/// # }
/// ```
///
/// [`Self::run`] does all of that in one call, for an application with
/// no queue of its own to route the ops through.
#[derive(Debug)]
pub struct DockView<'a, T> {
    node: Node,
    state: &'a DockState<T>,
    ops: &'a mut Vec<DockOp<T>>,
    min_pane: f32,
    overflow: TabOverflow,
    style: Option<&'a DockTheme>,
}

impl<'a, T: DockTab> DockView<'a, T> {
    /// A view over `state`, emitting into `ops`.
    ///
    /// The widget never mutates the tree. Everything it decides arrives
    /// as an op, so an application can route dock changes through the
    /// same queue as its own edits, keep them out of undo, and validate
    /// before a save.
    #[track_caller]
    pub fn new(state: &'a DockState<T>, ops: &'a mut Vec<DockOp<T>>) -> Self {
        Self {
            node: Node::zstack()
                .id(state.dock_id())
                .size((Sizing::FILL, Sizing::FILL)),
            state,
            ops,
            min_pane: 0.0,
            overflow: TabOverflow::default(),
            style: None,
        }
    }

    /// Floor either pane's extent on the split axis while a divider is
    /// dragged. Default `0.0`.
    pub fn min_pane(mut self, px: f32) -> Self {
        self.min_pane = px.max(0.0);
        self
    }

    /// What each pane's strip does with chips that do not fit. Default
    /// [`TabOverflow::Scroll`].
    pub fn overflow(mut self, overflow: TabOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    style_setter!(
        'a,
        DockTheme,
        dock,
        "The dividers read [`crate::Theme::splitter`] and every pane's \
         strip [`crate::Theme::tabs`], so this bundle covers the drag \
         feedback alone.",
    );

    /// Record the pane tree, and the drag feedback over it.
    pub fn show<'u, D: DockTabs<Tab = T>>(self, ui: &'u mut Ui, tabs: &mut D) -> Response<'u> {
        let app_theme = Rc::clone(ui.theme());
        let theme = self.slot(&app_theme);
        let Self {
            node,
            state,
            ops,
            min_pane,
            overflow,
            style: _,
        } = self;
        let widget = ui.widget(node);
        let id = widget.id();
        let response = widget.response(ui);
        let mut cx = DockCtx {
            state,
            ops,
            tabs,
            min_pane,
            overflow,
            theme,
        };
        widget.record(ui, None, |ui| {
            cx.node(ui, DockState::<T>::ROOT, DockPath::ROOT);
            if let Some(tab) = state.drag(ui) {
                ui.set_cursor(CursorIcon::Grabbing);
                cx.drag_feedback(ui, tab);
            }
        });
        Response::eager(id, ui, response)
    }
}

impl<T: DockTab> DockView<'_, T> {
    /// Scan, apply, record, apply — the whole frame in one call, for an
    /// application with no op queue of its own.
    ///
    /// The two-call surface exists so dock ops can travel through an
    /// application's own pipeline beside its other edits. An application
    /// with no such pipeline pays one line here instead.
    pub fn run<D: DockTabs<Tab = T>>(ui: &mut Ui, state: &mut DockState<T>, tabs: &mut D) {
        let id = state.dock_id();
        let mut ops = ui
            .try_state_mut::<DockOpBuf<T>>(id)
            .map(|buf| std::mem::take(&mut buf.ops))
            .unwrap_or_default();
        ops.clear();
        state.scan(ui, &mut ops);
        for op in ops.drain(..) {
            state.apply(op);
        }
        DockView::new(&*state, &mut ops).show(ui, tabs);
        for op in ops.drain(..) {
            state.apply(op);
        }
        ui.state_mut::<DockOpBuf<T>>(id).ops = ops;
    }
}

impl_configure!(<T> DockView<'_, T>);

/// The scratch [`DockView::run`] keeps between frames, so an application
/// that never spells the op vocabulary still allocates once rather than
/// once a frame.
#[derive(Debug)]
struct DockOpBuf<T> {
    ops: Vec<DockOp<T>>,
}

/// Hand-written rather than derived: a derive would demand `T: Default`,
/// and a tab key is an application enum with no meaningful default.
impl<T> Default for DockOpBuf<T> {
    fn default() -> Self {
        Self { ops: Vec::new() }
    }
}

/// What the recursive walk carries — one value rather than six
/// parameters, so the recursion keeps its arity.
#[derive(Debug)]
struct DockCtx<'c, T, D> {
    state: &'c DockState<T>,
    ops: &'c mut Vec<DockOp<T>>,
    tabs: &'c mut D,
    min_pane: f32,
    overflow: TabOverflow,
    /// Borrowed from the `Rc<Theme>` clone `show` holds for its whole
    /// body, so the theme outlives every reborrow of the `Ui` inside it.
    theme: &'c DockTheme,
}

impl<T: DockTab, D: DockTabs<Tab = T>> DockCtx<'_, T, D> {
    /// One node: a split onto a [`Splitter`], a leaf onto a pane.
    fn node(&mut self, ui: &mut Ui, idx: NodeIdx, path: DockPath) {
        let state = self.state;
        match state.node(idx) {
            DockNode::Group(group) => self.group(ui, group),
            DockNode::Split(split) => {
                let DockSplit {
                    dir,
                    ratio,
                    first,
                    second,
                } = *split;
                let mut live = ratio;
                let splitter = match dir {
                    SplitDir::Row => Splitter::horizontal(&mut live),
                    SplitDir::Column => Splitter::vertical(&mut live),
                };
                splitter
                    .id(state.splitter_id(path))
                    .min_pane(self.min_pane)
                    .show(ui, |ui, half| {
                        let (child, child_path) = match half {
                            SplitHalf::First => (first, path.first()),
                            SplitHalf::Second => (second, path.second()),
                        };
                        self.node(ui, child, child_path);
                    });
                // The widget wrote the divider drag into `live`; the
                // tree itself only changes through the recorded op.
                // Approximate rather than exact, because a re-derived
                // ratio carries last-bit noise an `!=` would emit on
                // every frame.
                if !approx::approx_zero(live - ratio) {
                    self.ops.push(DockOp::SetRatio {
                        split: path,
                        ratio: live,
                    });
                }
            }
        }
    }

    /// One pane: the group's tab strip over its active tab's view.
    fn group(&mut self, ui: &mut Ui, group: &TabGroup<T>) {
        let state = self.state;
        Panel::vstack()
            .id(state.pane_id(group.id))
            .size((Sizing::FILL, Sizing::FILL))
            // Focusable so a press anywhere in the pane that misses
            // every inner focusable lands here — which is what the
            // scan's focus query reads.
            .focusable(true)
            .show(ui, |ui| {
                self.strip(ui, group);
                // Last frame's arrangement, as every measurement during
                // a record is — but of the *group's* content area, which
                // outlives the tab in it. That is what lets a view first
                // recording on this pass still be handed a size.
                let size = state.content_size(ui, group.id);
                let tab = group.active_tab();
                let tabs = &mut *self.tabs;
                Panel::vstack()
                    .id(state.content_id(group.id))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| tabs.content(ui, tab, size));
            });
    }

    /// One pane's strip, and the per-chip menu behind it.
    fn strip(&mut self, ui: &mut Ui, group: &TabGroup<T>) {
        let strip_id = self.state.strip_id(group.id);
        let focused = self.state.focused() == group.id;
        let keyed = ui.with_state::<TabItemBuf, _>(strip_id, |ui, buf| {
            buf.items.clear();
            buf.items.reserve_exact(group.tabs.len());
            for &tab in &group.tabs {
                let label = self.tabs.title(ui, tab);
                buf.items.push(TabItem {
                    key: DockState::<T>::tab_key(tab),
                    label,
                    closable: self.tabs.closable(tab),
                    draggable: self.tabs.draggable(tab),
                    badge: self.tabs.badge(tab),
                    icon: self.tabs.icon(tab),
                });
            }
            TabStrip::new(&buf.items)
                .id(strip_id)
                .selected(group.active)
                .focused(focused)
                .overflow(self.overflow)
                .show(ui)
                .keyed
        });
        // Only the keyboard activation. A pointer click on a chip was
        // already turned into an op by the scan, a phase earlier, and
        // pushing it again here would put the same op in the queue
        // twice.
        //
        // So a keyboard move lands one frame after the press, where a
        // click lands on its own frame. That is inherent rather than a
        // shortcut: the strip resolves an arrow against its own input
        // scope, which only exists while it is recording, so there is
        // nothing for the earlier phase to read.
        if let Some(slot) = keyed
            && let Some(&tab) = group.tabs.get(slot)
        {
            self.ops.push(DockOp::ActivateTab { tab });
        }
        for &tab in &group.tabs {
            let key = DockState::<T>::tab_key(tab);
            let menu_id = strip_id.with(("menu", key));
            if ui
                .response_for(TabStrip::chip_id(strip_id, key))
                .right
                .clicked()
                && let Some(p) = ui.pointer_pos()
            {
                ContextMenu::open(ui, menu_id, p);
            }
            let ops = &mut *self.ops;
            let tabs = &mut *self.tabs;
            ContextMenu::for_id(menu_id)
                .size((Sizing::HUG, Sizing::HUG))
                .show(ui, |ui, close| {
                    tabs.tab_menu(
                        ui,
                        DockTabMenu {
                            tab,
                            group: group.id,
                            ops,
                            close,
                        },
                    );
                });
        }
    }

    /// The drag's tooltip-layer feedback: a wash over the region the
    /// drop would occupy — the whole pane for a join, half for a split,
    /// a caret between two chips for a strip insert — and a small ghost
    /// chip trailing the pointer.
    ///
    /// Both sense nothing, so the overlay never intercepts the drag's
    /// own hit-testing.
    fn drag_feedback(&mut self, ui: &mut Ui, tab: T) {
        let state = self.state;
        let dock = state.dock_id();
        if let Some(target) = state.drop_target(ui) {
            let r = target.highlight;
            let preview = Background::rounded(
                self.theme.preview_fill,
                Corners::all(self.theme.preview_corner),
            )
            .with_stroke(self.theme.preview_stroke);
            ui.layer(Layer::Tooltip)
                .at(r.min)
                .max_size(r.size)
                .show(|ui| {
                    Panel::zstack()
                        .id(dock.with("preview"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(preview)
                        .show(ui, |_| {});
                });
        }
        let Some(p) = ui.pointer_pos() else {
            return;
        };
        let label = self.tabs.title(ui, tab);
        let ghost = self.theme.ghost.background.clone();
        let ghost_text = self.theme.ghost.text.unwrap_or(ui.theme().text);
        let padding = self.theme.ghost_padding;
        ui.layer(Layer::Tooltip)
            .at(p + self.theme.ghost_offset)
            .show(|ui| {
                Panel::hstack()
                    .id(dock.with("ghost"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .padding(padding)
                    .background(ghost)
                    .show(ui, |ui| {
                        Text::new(label)
                            .id(dock.with("ghost_label"))
                            .style(&ghost_text)
                            .show(ui);
                    });
            });
    }
}
