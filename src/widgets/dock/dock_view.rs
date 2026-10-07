//! The dock widget: the split walk, one strip-over-content pane per group, and the drag-docking gesture.

use crate::input::keyboard::key::Key;
use crate::input::shortcut::Shortcut;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::background::Background;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::dock::dock_node::{DockNode, NodeIndex};
use crate::widgets::dock::dock_operation::DockOperation;
use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::dock_state::DockState;
use crate::widgets::dock::dock_tab::DockTab;
use crate::widgets::dock::dock_tabs::{DockTabMenu, DockTabs};
use crate::widgets::dock::pane_geometry::DropTarget;
use crate::widgets::dock::pane_geometry::PaneGeometry;
use crate::widgets::dock::split_side::SplitDirection;
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
use std::rc::Rc;

/// The docked pane tree: splits onto [`Splitter`]s, leaves as a [`TabStrip`] over a group-keyed content area, and the drag gesture that moves a tab.
///
/// **Two calls, not one.** Record cannot see this frame's layout, so a tab click learned mid-record would draw the pane it replaced. [`Self::scan`] runs a phase earlier, the application applies what it emits, then this walk runs.
///
/// ```no_run
/// # use palantir::{DockOperation, DockState, DockTabs, DockView, Ui};
/// # #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// # enum Tab { Main }
/// # fn demo<D: DockTabs<Tab = Tab>>(ui: &mut Ui, dock: &mut DockState<Tab>, tabs: &mut D) {
/// let mut operations: Vec<DockOperation<Tab>> = Vec::new();
/// DockView::scan(ui, dock, &mut operations);
/// for operation in operations.drain(..) {
///     dock.apply(operation);
/// }
/// DockView::new(dock, &mut operations).min_pane(220.0).show(ui, tabs);
/// for operation in operations.drain(..) {
///     dock.apply(operation);
/// }
/// # }
/// ```
///
/// [`Self::run`] does all of that in one call, for an application with no operation queue.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct DockView<'a, T> {
    widget: Widget,
    state: &'a DockState<T>,
    operations: &'a mut Vec<DockOperation<T>>,
    min_pane: f32,
    overflow: TabOverflow,
    style: Option<&'a DockTheme>,
}

impl<'a, T: DockTab> DockView<'a, T> {
    /// A view over `state`, emitting into `operations`. The widget never mutates the tree; every decision arrives as an operation.
    #[track_caller]
    pub fn new(state: &'a DockState<T>, operations: &'a mut Vec<DockOperation<T>>) -> Self {
        Self {
            widget: Widget::zstack()
                .id(Self::dock_id(state))
                .size((Sizing::FILL, Sizing::FILL)),
            state,
            operations,
            min_pane: 0.0,
            overflow: TabOverflow::default(),
            style: None,
        }
    }

    /// Floor either pane's extent on the split axis at `px`, a *length*, while a divider is dragged. Default `0.0`.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn min_pane(mut self, px: f32) -> Self {
        self.min_pane = domain::length(px);
        self
    }

    /// What each pane's strip does with chips that do not fit. Default [`TabOverflow::Scroll`].
    pub const fn overflow(mut self, overflow: TabOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `dock`: drag feedback only.
    pub fn style(mut self, s: impl Into<Option<&'a DockTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the pane tree and the drag feedback over it.
    pub fn show<'u, D: DockTabs<Tab = T>>(self, ui: &'u mut Ui, tabs: &mut D) -> Response<'u> {
        let app_theme = Rc::clone(ui.theme());
        let theme = self.style.unwrap_or(&app_theme.dock);
        let Self {
            mut widget,
            state,
            operations,
            min_pane,
            overflow,
            style: _,
        } = self;
        let id = widget.resolve(ui);
        let response = widget.response(ui);
        let mut cx = DockCtx {
            state,
            operations,
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
        Response::new(id, ui, response)
    }
}

impl<T: DockTab> DockView<'_, T> {
    /// Scan, apply, record, apply: the whole frame in one call, for an application with no operation queue.
    pub fn run<D: DockTabs<Tab = T>>(ui: &mut Ui, state: &mut DockState<T>, tabs: &mut D) {
        let id = DockView::dock_id(state);
        ui.with_state::<DockOpBuf<T>, _>(id, |ui, buf| {
            let operations = &mut buf.operations;
            operations.clear();
            DockView::scan(ui, state, operations);
            for operation in operations.drain(..) {
                state.apply(operation);
            }
            DockView::new(&*state, operations).show(ui, tabs);
            for operation in operations.drain(..) {
                state.apply(operation);
            }
        });
    }
}

impl<T: DockTab> DockView<'_, T> {
    /// The id everything else this dock records derives from.
    pub fn dock_id(state: &DockState<T>) -> WidgetId {
        WidgetId::from_hash(("palantir.dock", state.seed()))
    }

    /// A group's pane container (strip and content); the rect drops key off.
    pub fn pane_id(state: &DockState<T>, group: TabGroupId) -> WidgetId {
        Self::dock_id(state).with(("pane", group))
    }

    /// A group's content area, below the strip. Keyed by group, not tab, so a view is handed its size on the pass it first records.
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

    /// The chip key a tab is drawn under; shared by the strip and callers polling responses.
    pub fn tab_key(tab: T) -> u64 {
        WidgetId::from_hash(tab).0
    }

    /// Navigation-phase scan: focus follows a press into a pane, then chip responses (close beats activation, then drag arming), then the drag lifecycle.
    ///
    /// **Run before the record, and apply what it emits.**
    pub fn scan(ui: &mut Ui, state: &DockState<T>, operations: &mut Vec<DockOperation<T>>) {
        if let Some(group) = state
            .groups()
            .find(|g| g.id != state.focused() && ui.is_focus_within(Self::pane_id(state, g.id)))
        {
            operations.push(DockOperation::FocusPane { group: group.id });
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
                    operations.push(DockOperation::CloseTab { tab });
                    continue;
                }
                let chip = ui.response_for(TabStrip::chip_id(strip, key));
                if chip.clicked() {
                    operations.push(DockOperation::ActivateTab { tab });
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
        let chip = TabStrip::chip_id(Self::strip_id(state, address.group), Self::tab_key(tab));
        if ui.response_for(chip).left.drag.stopped() {
            if let Some(target) = Self::drop_target(state, ui) {
                operations.push(DockOperation::MoveTab {
                    tab,
                    to: target.drop,
                });
            }
            Self::set_drag(state, ui, None);
        }
    }

    fn drag(state: &DockState<T>, ui: &Ui) -> Option<T> {
        ui.state::<TabDrag<T>>(Self::dock_id(state))
            .and_then(|d| d.tab)
    }

    fn set_drag(state: &DockState<T>, ui: &mut Ui, tab: Option<T>) {
        ui.with_state::<TabDrag<T>, _>(Self::dock_id(state), |_, s| s.tab = tab);
    }

    /// The drop the pointer indicates: the pane containing it, classified into a zone. Not a hover test, since an inert pane hovers nothing. `None` off the panes; a release there cancels.
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

    /// The arranged size of a group's content area, `None` before its first layout.
    pub fn content_size(ui: &Ui, state: &DockState<T>, group: TabGroupId) -> Option<Size> {
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

/// Scratch [`DockView::run`] keeps between frames, so it allocates once rather than per frame.
#[derive(Debug)]
struct DockOpBuf<T> {
    operations: Vec<DockOperation<T>>,
}

impl<T> Default for DockOpBuf<T> {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
struct ChipRects {
    rects: Vec<Rect>,
}

#[derive(Debug)]
struct DockCtx<'c, T, D> {
    state: &'c DockState<T>,
    operations: &'c mut Vec<DockOperation<T>>,
    tabs: &'c mut D,
    min_pane: f32,
    overflow: TabOverflow,
    /// Borrowed from the `Rc<Theme>` clone `show` holds, so the theme outlives every `Ui` reborrow.
    theme: &'c DockTheme,
}

impl<T: DockTab, D: DockTabs<Tab = T>> DockCtx<'_, T, D> {
    fn node(&mut self, ui: &mut Ui, idx: NodeIndex, path: DockPath) {
        let state = self.state;
        match state.node(idx) {
            DockNode::Group(group) => self.group(ui, group),
            DockNode::Split(split) => {
                let (dir, ratio, first, second) = (
                    split.direction(),
                    split.ratio(),
                    split.first(),
                    split.second(),
                );
                let mut live = ratio;
                let splitter = match dir {
                    SplitDirection::Row => Splitter::row(&mut live),
                    SplitDirection::Column => Splitter::column(&mut live),
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
                if hit.changed {
                    self.operations.push(DockOperation::SetRatio {
                        split: path,
                        ratio: live,
                    });
                }
            }
        }
    }

    fn group(&mut self, ui: &mut Ui, group: &TabGroup<T>) {
        let state = self.state;
        Panel::vstack()
            .id(DockView::pane_id(state, group.id))
            .size((Sizing::FILL, Sizing::FILL))
            .focusable(true)
            .show(ui, |ui| {
                self.strip(ui, group);
                let size = DockView::content_size(ui, state, group.id);
                let tab = group.active_tab();
                let tabs = &mut *self.tabs;
                Panel::vstack()
                    .id(DockView::content_id(state, group.id))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| tabs.content(ui, tab, size));
            });
    }

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
        // The scan sees only chip and close-button ids; keyboard moves and popup entries have none, so they land a frame later.
        if let Some(slot) = activated
            && let Some(&tab) = group.tabs.get(slot)
        {
            self.operations.push(DockOperation::ActivateTab { tab });
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
            let operations = &mut *self.operations;
            let tabs = &mut *self.tabs;
            ContextMenu::for_id(menu_id)
                .size((Sizing::HUG, Sizing::HUG))
                .show(ui, |ui, close| {
                    tabs.tab_menu(
                        ui,
                        DockTabMenu {
                            tab,
                            group: group.id,
                            operations,
                            close,
                        },
                    );
                });
        }
    }

    /// The drag's tooltip-layer feedback: a wash over the drop region and a ghost chip. Both sense nothing, so hit-testing is unaffected.
    fn drag_feedback(&mut self, ui: &mut Ui, tab: T) {
        let state = self.state;
        let dock = DockView::dock_id(state);
        if let Some(target) = DockView::drop_target(state, ui) {
            let r = target.highlight;
            let preview = Background::rounded(
                self.theme.preview_fill,
                Corners::all(self.theme.preview_radius),
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
