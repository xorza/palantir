//! The dock widget: the split walk, one strip-over-content pane per
//! group, and the drag-docking gesture.

use crate::input::keyboard::key::Key;
use crate::input::shortcut::Shortcut;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::dock::dock_node::{DockNode, NodeIdx};
use crate::widgets::dock::dock_op::DockOp;
use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::dock_state::DockState;
use crate::widgets::dock::dock_tab::DockTab;
use crate::widgets::dock::dock_tabs::{DockTabMenu, DockTabs};
use crate::widgets::dock::pane_geometry::DropTarget;
use crate::widgets::dock::pane_geometry::PaneGeometry;
use crate::widgets::dock::split_side::SplitDir;
use crate::widgets::dock::tab_drag::TabDrag;
use crate::widgets::dock::tab_group::TabGroup;
use crate::widgets::dock::tab_group::TabGroupId;
use crate::widgets::panel::Panel;
use crate::widgets::splitter::Splitter;
use crate::widgets::splitter::split_half::SplitHalf;
use crate::widgets::tabs::tab_item::{TabItem, TabItemBuf};
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use crate::widgets::text::Text;
use crate::widgets::theme::dock::DockTheme;
use crate::window::cursor_icon::CursorIcon;
use std::mem;
use std::rc::Rc;

/// The docked pane tree: splits onto [`Splitter`]s, leaves as a
/// [`TabStrip`] over a group-keyed content area, and the drag gesture
/// that moves a tab between them.
///
/// **Two calls, not one.** Palantir's record pass cannot see this
/// frame's layout, so a widget that learned of a tab click mid-record
/// would draw the pane the click replaced.
/// [`Self::scan`] runs a phase earlier, the application applies
/// what it emits, and only then does this walk run — so a switch draws
/// on the frame it lands.
///
/// ```no_run
/// # use palantir::{DockOp, DockState, DockTabs, DockView, Ui};
/// # #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// # enum Tab { Main }
/// # fn demo<D: DockTabs<Tab = Tab>>(ui: &mut Ui, dock: &mut DockState<Tab>, tabs: &mut D) {
/// let mut ops: Vec<DockOp<Tab>> = Vec::new();
/// DockView::scan(dock, ui, &mut ops);
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
#[must_use = "a widget records nothing until `show`"]
pub struct DockView<'a, T> {
    widget: Widget,
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
    /// same queue as its own edits and keep them out of undo.
    #[track_caller]
    pub fn new(state: &'a DockState<T>, ops: &'a mut Vec<DockOp<T>>) -> Self {
        Self {
            widget: Widget::zstack()
                .id(Self::dock_id(state))
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
    pub const fn min_pane(mut self, px: f32) -> Self {
        self.min_pane = px.max(0.0);
        self
    }

    /// What each pane's strip does with chips that do not fit. Default
    /// [`TabOverflow::Scroll`].
    pub const fn overflow(mut self, overflow: TabOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `dock`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    ///
    /// The dividers read [`crate::Theme::splitter`] and every pane's strip
    /// [`crate::Theme::tabs`], so this bundle covers the drag feedback
    /// alone.
    pub fn style(mut self, s: impl Into<Option<&'a DockTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the pane tree, and the drag feedback over it.
    pub fn show<'u, D: DockTabs<Tab = T>>(self, ui: &'u mut Ui, tabs: &mut D) -> Response<'u> {
        let app_theme = Rc::clone(ui.theme());
        let theme = self.style.unwrap_or(&app_theme.dock);
        let Self {
            mut widget,
            state,
            ops,
            min_pane,
            overflow,
            style: _,
        } = self;
        let id = widget.resolve(ui);
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
            if let Some(tab) = DockView::drag(state, ui) {
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
        let id = DockView::dock_id(state);
        let mut ops = ui
            .state_mut::<DockOpBuf<T>>(id)
            .map(|buf| mem::take(&mut buf.ops))
            .unwrap_or_default();
        ops.clear();
        DockView::scan(state, ui, &mut ops);
        for op in ops.drain(..) {
            state.apply(op);
        }
        DockView::new(&*state, &mut ops).show(ui, tabs);
        for op in ops.drain(..) {
            state.apply(op);
        }
        ui.state_or_default::<DockOpBuf<T>>(id).ops = ops;
    }
}

/// The view facts a dock is addressed by — its ids, the scan that runs a
/// phase before the record, and the content size a tab is handed — as
/// associated functions over the [`DockState`] they read, which stays pure
/// data.
impl<T: DockTab> DockView<'_, T> {
    /// The id everything else this dock records derives from.
    pub fn dock_id(state: &DockState<T>) -> WidgetId {
        WidgetId::from_hash(("palantir.dock", state.seed()))
    }

    /// A group's pane container — strip row and content together. The
    /// rect the drop classification keys off.
    pub fn pane_id(state: &DockState<T>, group: TabGroupId) -> WidgetId {
        Self::dock_id(state).with(("pane", group))
    }

    /// A group's *content* area — the space below the strip that the
    /// active tab's view fills.
    ///
    /// Keyed by the group rather than by the tab it happens to be
    /// showing, which is the whole point: switching tabs leaves this
    /// widget in place, so a view can be handed its arranged size on the
    /// very pass it first records.
    pub fn content_id(state: &DockState<T>, group: TabGroupId) -> WidgetId {
        Self::dock_id(state).with(("content", group))
    }

    /// A group's tab strip.
    pub fn strip_id(state: &DockState<T>, group: TabGroupId) -> WidgetId {
        Self::dock_id(state).with(("strip", group))
    }

    /// The splitter at a tree path.
    pub fn splitter_id(state: &DockState<T>, path: DockPath) -> WidgetId {
        Self::dock_id(state).with(("splitter", path))
    }

    fn drag_id(state: &DockState<T>) -> WidgetId {
        Self::dock_id(state).with("drag")
    }

    /// The chip key a tab is drawn under — the one derivation, so the
    /// strip and a caller polling last frame's responses ask the same
    /// question.
    pub fn tab_key(tab: T) -> u64 {
        WidgetId::from_hash(tab).0
    }

    /// Navigation-phase scan: focus follows a press into a pane, then
    /// one pass over every strip's last-frame chip responses — close
    /// clicks (which win over activation), activation clicks, and the
    /// drag arming — then the in-flight drag's lifecycle.
    ///
    /// **Run this before the record, and apply what it emits.** Palantir
    /// cannot see this frame's layout during a record, so a widget that
    /// learned of a tab click mid-record would draw the pane the click
    /// replaced. Scanning a phase earlier settles the new arrangement
    /// first, so a switch — or a committed drop — draws on the frame it
    /// lands rather than the one after.
    pub fn scan(state: &DockState<T>, ui: &mut Ui, ops: &mut Vec<DockOp<T>>) {
        // Ahead of the chip pass: a read-only focus query that only ever
        // moves `focused`, so it composes with an activation from the
        // same scan rather than racing it.
        if let Some(group) = state
            .groups()
            .find(|g| g.id != state.focused() && ui.focus_within(Self::pane_id(state, g.id)))
        {
            ops.push(DockOp::FocusPane { group: group.id });
        }
        let mut dragged = Self::drag(state, ui);
        for group in state.groups() {
            let strip = Self::strip_id(state, group.id);
            for &tab in &group.tabs {
                let key = Self::tab_key(tab);
                if ui
                    .response_for(TabStrip::close_id(strip, key))
                    .left
                    .clicked()
                {
                    ops.push(DockOp::CloseTab { tab });
                    continue;
                }
                let chip = ui.response_for(TabStrip::chip_id(strip, key));
                if chip.clicked() {
                    ops.push(DockOp::ActivateTab { tab });
                }
                if dragged.is_none() && chip.left.drag.started() {
                    dragged = Some(tab);
                    Self::set_drag(state, ui, Some(tab));
                }
            }
        }
        let Some(tab) = dragged else {
            return;
        };
        let Some(address) = state.find_tab(tab) else {
            Self::set_drag(state, ui, None);
            return;
        };
        if ui.key_pressed(Shortcut::key(Key::Escape)) {
            Self::set_drag(state, ui, None);
            return;
        }
        // The release edge fires on the chip that caught the press.
        let chip = TabStrip::chip_id(Self::strip_id(state, address.group), Self::tab_key(tab));
        if ui.response_for(chip).left.drag.stopped() {
            if let Some(target) = Self::drop_target(state, ui) {
                ops.push(DockOp::MoveTab {
                    tab,
                    to: target.drop,
                });
            }
            Self::set_drag(state, ui, None);
        }
    }

    /// The tab a pointer is currently carrying, if any.
    fn drag(state: &DockState<T>, ui: &Ui) -> Option<T> {
        ui.state::<TabDrag<T>>(Self::drag_id(state))
            .and_then(|d| d.tab)
    }

    fn set_drag(state: &DockState<T>, ui: &mut Ui, tab: Option<T>) {
        ui.state_or_default::<TabDrag<T>>(Self::drag_id(state)).tab = tab;
    }

    /// The drop the pointer currently indicates: the pane whose rect
    /// contains it, classified into a zone.
    ///
    /// Panes tile the dock without overlapping, so plain containment
    /// against last frame's rects is exact. Deliberately *not* a hover
    /// test: the hover resolves only to sensed widgets, and a pane's
    /// content can be entirely inert — the pointer over it hovers
    /// nothing, and the drop would go dark. `None` over a divider, the
    /// chrome around the dock, or off-window; a release there cancels.
    fn drop_target(state: &DockState<T>, ui: &mut Ui) -> Option<DropTarget> {
        let p = ui.pointer_pos()?;
        let (edge_fraction, caret_width) = {
            let dock = &ui.theme().dock;
            (dock.edge_fraction, dock.caret_width)
        };
        let (group, pane) = state.groups().find_map(|g| {
            let rect = ui.response_for(Self::pane_id(state, g.id)).rect?;
            rect.contains(p).then_some((g, rect))
        })?;
        let strip_id = Self::strip_id(state, group.id);
        let strip = ui.response_for(strip_id).rect?;
        let can_split = state.can_split(group.id);
        let allowed = state.allowed_splits();
        let dock_id = Self::dock_id(state);
        ui.with_state::<ChipRects, _>(dock_id, |ui, buf| {
            buf.rects.clear();
            // An upper bound, not a count — a tab that recorded no rect
            // drops out — so `reserve`, and a no-op from the drag's
            // second frame on.
            buf.rects.reserve(group.tabs.len());
            buf.rects.extend(group.tabs.iter().filter_map(|&tab| {
                ui.response_for(TabStrip::chip_id(strip_id, Self::tab_key(tab)))
                    .rect
            }));
            Some(
                PaneGeometry {
                    group: group.id,
                    pane,
                    strip,
                    chips: &buf.rects,
                    can_split,
                    allowed,
                    edge_fraction,
                    caret_width,
                }
                .classify(p),
            )
        })
    }

    /// The arranged size of a group's content area, `None` before its
    /// first layout — the one frame in a group's life where a view has
    /// to size itself.
    pub fn content_size(state: &DockState<T>, ui: &Ui, group: TabGroupId) -> Option<Size> {
        let size = ui
            .response_for(Self::content_id(state, group))
            .layout_rect?
            .size;
        (size.w > 0.0 && size.h > 0.0).then_some(size)
    }
}

impl<T> Configure for DockView<'_, T> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

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

/// The chip rects one drop classification reads.
///
/// Kept on the dock's own state row rather than rebuilt per frame: a
/// held drag asks for them on every pointer move.
#[derive(Debug, Default)]
struct ChipRects {
    rects: Vec<Rect>,
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
                let (dir, ratio, first, second) =
                    (split.dir(), split.ratio(), split.first(), split.second());
                let mut live = ratio;
                let splitter = match dir {
                    SplitDir::Row => Splitter::row(&mut live),
                    SplitDir::Column => Splitter::column(&mut live),
                };
                let hit = splitter
                    .id(DockView::splitter_id(state, path))
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
                if hit.changed {
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
            .id(DockView::pane_id(state, group.id))
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
                let size = DockView::content_size(state, ui, group.id);
                let tab = group.active_tab();
                let tabs = &mut *self.tabs;
                Panel::vstack()
                    .id(DockView::content_id(state, group.id))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| tabs.content(ui, tab, size));
            });
    }

    /// One pane's strip, and the per-chip menu behind it.
    fn strip(&mut self, ui: &mut Ui, group: &TabGroup<T>) {
        let strip_id = DockView::strip_id(self.state, group.id);
        let focused = self.state.focused() == group.id;
        let activated = ui.with_state::<TabItemBuf, _>(strip_id, |ui, buf| {
            buf.items.clear();
            buf.items.reserve_exact(group.tabs.len());
            for &tab in &group.tabs {
                let label = self.tabs.title(ui, tab);
                buf.items.push(TabItem {
                    key: DockView::tab_key(tab),
                    label,
                    closable: self.tabs.closable(tab),
                    draggable: self.tabs.draggable(tab),
                    badge: self.tabs.badge(tab),
                    icon: self.tabs.icon(tab),
                });
            }
            let hit = TabStrip::new(&buf.items)
                .id(strip_id)
                .selected(group.active)
                .focused(focused)
                .overflow(self.overflow)
                .show(ui);
            hit.keyed.or(hit.menu_picked)
        });
        // Everything the scan a phase earlier could not see. A pointer
        // click on a chip was already turned into an op there, and
        // pushing it again here would put the same op in the queue
        // twice — but that scan reads chip and close-button ids, and
        // neither a keyboard move nor a popup entry has one.
        //
        // So a keyboard move lands one frame after the press, where a
        // click lands on its own frame. That is inherent rather than a
        // shortcut: the strip resolves an arrow against its own input
        // scope, which only exists while it is recording, so there is
        // nothing for the earlier phase to read.
        if let Some(slot) = activated
            && let Some(&tab) = group.tabs.get(slot)
        {
            self.ops.push(DockOp::ActivateTab { tab });
        }
        for &tab in &group.tabs {
            let key = DockView::tab_key(tab);
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
        let dock = DockView::dock_id(state);
        if let Some(target) = DockView::drop_target(state, ui) {
            let r = target.highlight;
            let preview = Background::rounded(
                self.theme.preview_fill,
                Corners::all(self.theme.preview_corner),
            )
            .with_border(self.theme.preview_stroke);
            ui.layer(Layer::Tooltip)
                .fixed_at(r.min)
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
        let ghost_text = self.theme.ghost.text.apply(&ui.theme().text);
        let padding = self.theme.ghost_padding;
        ui.layer(Layer::Tooltip)
            .fixed_at(p + self.theme.ghost_offset)
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
