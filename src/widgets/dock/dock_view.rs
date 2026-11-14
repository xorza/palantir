//! The dock widget: the split walk, one strip-over-content pane per group, and the drag-docking gesture.

use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::scene::layer::Layer;
use crate::theme::dock::DockTheme;
use crate::theme::menu::MenuTheme;
use crate::theme::tabs::TabsTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use crate::widgets::dock::allowed_splits::AllowedSplits;
use crate::widgets::dock::dock_node::{DockNode, NodeIdx};
use crate::widgets::dock::dock_operation::DockOperation;
use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::dock_state::{DockState, RATIO_MIN};
use crate::widgets::dock::dock_tab::DockTab;
use crate::widgets::dock::dock_tabs::{DockTabMenu, DockTabs};
use crate::widgets::dock::pane_geometry::DropTarget;
use crate::widgets::dock::pane_geometry::PaneGeometry;
use crate::widgets::dock::split_side::SplitDirection;
use crate::widgets::dock::tab_drag::TabDrag;
use crate::widgets::dock::tab_group::TabGroup;
use crate::widgets::dock::tab_group::TabGroupId;
use crate::widgets::menu::Menu;
use crate::widgets::panel::Panel;
use crate::widgets::splitter::Splitter;
use crate::widgets::splitter::split_half::SplitHalf;
use crate::widgets::tabs::tab_item::{TabItem, TabItemBuf};
use crate::widgets::tabs::tab_key::TabKey;
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use crate::widgets::text::Text;
use crate::window::cursor_icon::CursorIcon;
use std::rc::Rc;

/// The docked pane tree: splits onto [`Splitter`]s, leaves as a [`TabStrip`] over a group-keyed content area, and the drag gesture that moves a tab.
///
/// **Two calls, not one.** The walk cannot see this frame's layout, so a tab click learned mid-walk would draw the pane it replaced. [`Self::scan`] runs ahead of it, the application applies what it emits, then this walk runs.
///
/// ```no_run
/// # use palantir::{DockOperation, DockState, DockTabs, DockView, Ui, WidgetId};
/// # #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// # enum Tab { Main }
/// # fn demo<D: DockTabs<Tab = Tab>>(ui: &mut Ui, dock: &mut DockState<Tab>, tabs: &mut D) {
/// let id = WidgetId::from_hash("app.dock");
/// let mut operations: Vec<DockOperation<Tab>> = Vec::new();
/// DockView::scan(ui, id, dock, &mut operations);
/// for operation in operations.drain(..) {
///     dock.apply(operation);
/// }
/// DockView::new(id, dock, &mut operations).min_pane(220.0).show(ui, tabs);
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
    /// The id [`Self::scan`] reads, which `show` holds the recorded id to.
    dock: WidgetId,
    widget: Widget,
    state: &'a DockState<T>,
    operations: &'a mut Vec<DockOperation<T>>,
    min_pane: f32,
    overflow: TabOverflow,
    allowed_splits: AllowedSplits,
    edge_fraction: f32,
    style: Option<&'a DockTheme>,
    tabs_style: Option<&'a TabsTheme>,
    menu_style: Option<&'a MenuTheme>,
}

/// The split wedges' reach: the inner half of a pane's content is the join zone.
const DEFAULT_EDGE_FRACTION: f32 = 0.25;

impl<'a, T: DockTab> DockView<'a, T> {
    /// A view over `state` under the id `dock`, emitting into `operations`. The widget never
    /// mutates the tree; every decision arrives as an operation. `dock` is the id
    /// [`Self::scan`] reads and every part's id derives from; [`Self::show`] refuses another, so
    /// the two calls cannot part.
    pub fn new(
        dock: WidgetId,
        state: &'a DockState<T>,
        operations: &'a mut Vec<DockOperation<T>>,
    ) -> Self {
        Self {
            dock,
            widget: Widget::zstack().id(dock),
            state,
            operations,
            min_pane: 0.0,
            overflow: TabOverflow::default(),
            allowed_splits: AllowedSplits::ALL,
            edge_fraction: DEFAULT_EDGE_FRACTION,
            style: None,
            tabs_style: None,
            menu_style: None,
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

    /// Which split directions a tab drag offers. Default [`AllowedSplits::ALL`].
    pub const fn allowed_splits(mut self, allowed: AllowedSplits) -> Self {
        self.allowed_splits = allowed;
        self
    }

    /// How far in from each edge of a pane's content a dragged tab splits rather than joins,
    /// as a fraction of that content's width or height. Default `0.25`, which leaves the
    /// inner half as the join zone; `0.0` offers no split, and `0.5` no join.
    ///
    /// # Panics
    ///
    /// Panics unless `fraction` is in `0..=0.5`.
    #[track_caller]
    pub const fn edge_fraction(mut self, fraction: f32) -> Self {
        assert!(
            fraction >= 0.0 && fraction <= 0.5,
            "edge_fraction must be in 0..=0.5",
        );
        self.edge_fraction = fraction;
        self
    }

    /// Per-instance override of [`Theme::dock`](crate::Theme::dock): the drop wash and the carried chip.
    pub fn style(mut self, s: impl Into<Option<&'a DockTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Per-instance override of [`Theme::tabs`](crate::Theme::tabs) for every pane's strip.
    pub fn tabs_style(mut self, s: impl Into<Option<&'a TabsTheme>>) -> Self {
        self.tabs_style = s.into();
        self
    }

    /// Per-instance override of [`Theme::menu`](crate::Theme::menu) for the tab menus and the
    /// strips' overflow menus.
    pub fn menu_style(mut self, s: impl Into<Option<&'a MenuTheme>>) -> Self {
        self.menu_style = s.into();
        self
    }

    /// Record the pane tree and the drag feedback over it.
    ///
    /// # Panics
    ///
    /// Panics when a `Configure` id setter (`id`, `id_salt`, `auto_id`) moved the view off the
    /// id `new` took: [`Self::scan`] reads that id, so every click, close and drag would be lost.
    #[track_caller]
    pub fn show<D: DockTabs<Tab = T>>(self, ui: &mut Ui, tabs: &mut D) -> Response {
        let app_theme = Rc::clone(ui.theme());
        let theme = self.style.unwrap_or(&app_theme.dock);
        let Self {
            dock,
            mut widget,
            state,
            operations,
            min_pane,
            overflow,
            allowed_splits,
            edge_fraction,
            style: _,
            tabs_style,
            menu_style,
        } = self;
        widget
            .configure()
            .default_size((Sizing::FILL, Sizing::FILL));
        let response = widget.response(ui);
        let id = response.id;
        assert!(
            id == dock,
            "a dock view records under the id `new` took, which `scan` reads",
        );
        let carried = DockView::drag(ui, id);
        // Classified before the walk, so the strip it lands in records its caret, and kept for
        // the scan that sees the release: the drop lands where the frame showed it.
        let drop = carried.and_then(|tab| {
            let rules = SplitRules {
                allowed: allowed_splits,
                edge_fraction,
            };
            DockView::drop_target(ui, id, state, rules)
                .filter(|target| state.accepts_move(tab, target.drop()))
        });
        if let Some(tab) = carried {
            ui.set_state(id, TabDrag { tab, target: drop });
        }
        let mut cx = DockCtx {
            dock: id,
            state,
            operations,
            tabs,
            min_pane,
            overflow,
            theme,
            tabs_style,
            menu_style,
            drop,
        };
        widget.record(ui, None, |ui| {
            cx.node(ui, DockState::<T>::ROOT, DockPath::ROOT);
            cx.tab_menu(ui);
            if let Some(tab) = carried {
                ui.request_cursor(CursorIcon::Grabbing);
                cx.drag_feedback(ui, tab);
            }
        });
        response
    }
}

impl<T: DockTab> DockView<'_, T> {
    /// Scan, apply, record, apply: the whole frame in one call, for an application with no
    /// operation queue, under the id `dock`.
    pub fn run<D: DockTabs<Tab = T>>(
        ui: &mut Ui,
        dock: WidgetId,
        state: &mut DockState<T>,
        tabs: &mut D,
    ) {
        ui.with_state::<DockOpBuf<T>, _>(dock, |ui, buf| {
            let operations = &mut buf.operations;
            operations.clear();
            DockView::scan(ui, dock, state, operations);
            for operation in operations.drain(..) {
                state.apply(operation);
            }
            DockView::new(dock, &*state, operations).show(ui, tabs);
            for operation in operations.drain(..) {
                state.apply(operation);
            }
        });
    }
}

impl<T: DockTab> DockView<'_, T> {
    /// Navigation-phase scan: focus follows a press into a pane, then chip responses (close beats activation, then drag arming), then the drag lifecycle.
    ///
    /// **Run before [`Self::show`], and apply what it emits.**
    pub fn scan(
        ui: &mut Ui,
        dock: WidgetId,
        state: &DockState<T>,
        operations: &mut Vec<DockOperation<T>>,
    ) {
        if let Some(group) = state
            .groups()
            .find(|g| g.id != state.focused() && ui.is_focus_within(g.id.pane_id(dock)))
        {
            operations.push(DockOperation::FocusPane { group: group.id });
        }
        let mut dragged = Self::drag(ui, dock);
        for group in state.groups() {
            let hits = TabStrip::scan(
                ui,
                group.id.strip_id(dock),
                group.tabs.iter().map(|&tab| TabKey::of(tab)),
            );
            let tab_at = |slot: Option<usize>| slot.map(|slot| group.tabs[slot]);
            if let Some(tab) = tab_at(hits.closed) {
                operations.push(DockOperation::CloseTab { tab });
            }
            if let Some(tab) = tab_at(hits.clicked) {
                operations.push(DockOperation::ActivateTab { tab });
            }
            if dragged.is_none()
                && let Some(tab) = tab_at(hits.drag_started)
            {
                dragged = Some(tab);
                ui.set_state(dock, TabDrag { tab, target: None });
            }
        }
        let Some(tab) = dragged else {
            return;
        };
        let Some(address) = state.find_tab(tab) else {
            Self::end_drag(ui, dock);
            return;
        };
        if ui.shortcut_pressed(Key::Escape) {
            Self::end_drag(ui, dock);
            return;
        }
        let chip = TabStrip::chip_id(address.group.strip_id(dock), TabKey::of(tab));
        if ui.response_for(chip).left.drag.stopped() {
            let shown = ui.state::<TabDrag<T>>(dock).and_then(|d| d.target);
            if let Some(target) = shown {
                operations.push(DockOperation::MoveTab {
                    tab,
                    to: target.drop(),
                });
            }
            Self::end_drag(ui, dock);
        }
    }

    fn drag(ui: &Ui, dock: WidgetId) -> Option<T> {
        ui.state::<TabDrag<T>>(dock).map(|drag| drag.tab)
    }

    /// End the drag and forget its drop.
    fn end_drag(ui: &mut Ui, dock: WidgetId) {
        ui.remove_state::<TabDrag<T>>(dock);
    }

    /// The drop the pointer indicates: the pane containing it, classified into a zone. Not a hover test, since an inert pane hovers nothing. `None` off the panes; a release there cancels, as it does over a drop [`DockState::accepts_move`] refuses.
    fn drop_target(
        ui: &mut Ui,
        dock: WidgetId,
        state: &DockState<T>,
        rules: SplitRules,
    ) -> Option<DropTarget> {
        let SplitRules {
            allowed,
            edge_fraction,
        } = rules;
        let p = ui.pointer_pos()?;
        let (group, pane) = state.groups().find_map(|g| {
            let rect = ui.response_for(g.id.pane_id(dock)).rect?;
            rect.contains(p).then_some((g, rect))
        })?;
        let strip_id = group.id.strip_id(dock);
        let strip = ui.response_for(strip_id).rect?;
        let keys = group.tabs.iter().map(|&tab| TabKey::of(tab));
        let geometry = PaneGeometry {
            group: group.id,
            pane,
            strip,
            strip_slot: TabStrip::drop_slot(ui, strip_id, keys, p.x),
            tabs: group.tabs.len(),
            can_split: state.can_split(group.id),
            allowed,
            edge_fraction,
        };
        Some(geometry.classify(p))
    }
}

/// The arranged size of a group's content area, `None` before its first layout.
fn content_size(ui: &Ui, dock: WidgetId, group: TabGroupId) -> Option<Size> {
    let size = ui.response_for(group.content_id(dock)).layout_rect?.size;
    (size.w > 0.0 && size.h > 0.0).then_some(size)
}

impl<T> Configure for DockView<'_, T> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
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

/// The chip whose menu is open, kept on the dock's own id: one menu at a time, since it is modal.
#[derive(Clone, Copy, Debug)]
struct TabMenuTarget<T> {
    tab: T,
    group: TabGroupId,
}

impl<T: DockTab> TabMenuTarget<T> {
    /// The chip the menu opened from, which keys its open state.
    fn chip(&self, dock: WidgetId) -> WidgetId {
        TabStrip::chip_id(self.group.strip_id(dock), TabKey::of(self.tab))
    }
}

/// Where a dock lets a carried tab split a pane.
#[derive(Clone, Copy, Debug)]
struct SplitRules {
    allowed: AllowedSplits,
    /// Reach of the split wedges in from each edge.
    edge_fraction: f32,
}

#[derive(Debug)]
struct DockCtx<'c, T, D> {
    /// The dock's own id, which holds the open tab menu's target.
    dock: WidgetId,
    state: &'c DockState<T>,
    operations: &'c mut Vec<DockOperation<T>>,
    tabs: &'c mut D,
    min_pane: f32,
    overflow: TabOverflow,
    /// Borrowed from the `Rc<Theme>` clone `show` holds, so the theme outlives every `Ui` reborrow.
    theme: &'c DockTheme,
    tabs_style: Option<&'c TabsTheme>,
    menu_style: Option<&'c MenuTheme>,
    /// Where the carried tab would land, from last frame's geometry.
    drop: Option<DropTarget>,
}

impl<T: DockTab, D: DockTabs<Tab = T>> DockCtx<'_, T, D> {
    fn node(&mut self, ui: &mut Ui, idx: NodeIdx, path: DockPath) {
        let state = self.state;
        match state.node(idx) {
            DockNode::Group(group) => self.group(ui, group),
            DockNode::Split(split) => {
                let split = *split;
                let mut live = split.ratio();
                let splitter = match split.direction() {
                    SplitDirection::Row => Splitter::row(&mut live),
                    SplitDirection::Col => Splitter::col(&mut live),
                };
                // The split clamp floors the drag too, so a release keeps what the drag drew.
                let hit = splitter
                    .id(path.splitter_id(self.dock))
                    .min_pane(self.min_pane)
                    .min_ratio(RATIO_MIN)
                    .show(ui, |ui, half| {
                        let child_path = match half {
                            SplitHalf::First => path.first(),
                            SplitHalf::Second => path.second(),
                        };
                        self.node(ui, split.child(half), child_path);
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
        let dock = self.dock;
        Panel::vstack()
            .id(group.id.pane_id(dock))
            .size((Sizing::FILL, Sizing::FILL))
            .focusable(true)
            .show(ui, |ui| {
                self.strip(ui, group);
                let size = content_size(ui, dock, group.id);
                let tab = group.active_tab();
                let tabs = &mut *self.tabs;
                Panel::vstack()
                    .id(group.id.content_id(dock))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| tabs.body(ui, tab, size));
            });
    }

    fn strip(&mut self, ui: &mut Ui, group: &TabGroup<T>) {
        let strip_id = group.id.strip_id(self.dock);
        let focused = self.state.focused() == group.id;
        let activated = ui.with_state::<TabItemBuf, _>(strip_id, |ui, buf| {
            buf.items.clear();
            buf.items.reserve_exact(group.tabs.len());
            for &tab in &group.tabs {
                let label = self.tabs.label(ui, tab);
                buf.items.push(TabItem {
                    key: TabKey::of(tab),
                    label,
                    closable: self.tabs.closable(tab),
                    draggable: self.tabs.draggable(tab),
                });
            }
            let hit = TabStrip::new(&buf.items)
                .id(strip_id)
                .selected(group.active)
                .focused(focused)
                .overflow(self.overflow)
                .style(self.tabs_style)
                .menu_style(self.menu_style)
                .dock_drag(self.drop.and_then(|drop| drop.caret_in(group.id)))
                .show(ui, |ui, chip| {
                    self.tabs.chip(ui, group.tabs[chip.slot], chip);
                });
            hit.unscanned
        });
        // The scan sees only chip and close-button ids; keyboard moves and popup entries have none, so they land a frame later.
        if let Some(slot) = activated
            && let Some(&tab) = group.tabs.get(slot)
        {
            self.operations.push(DockOperation::ActivateTab { tab });
        }
        // A right-click opens the tab's menu only where the app declares one, as an empty menu
        // would still eat the next click.
        for &tab in &group.tabs {
            let chip = TabStrip::chip_id(strip_id, TabKey::of(tab));
            if ui.response_for(chip).right.clicked()
                && self.tabs.has_tab_menu(tab)
                && let Some(p) = ui.pointer_pos()
            {
                let target = TabMenuTarget {
                    tab,
                    group: group.id,
                };
                ui.set_state(self.dock, target);
                Menu::open(ui, chip, p);
            }
        }
    }

    /// The open tab menu, if any: one per dock, recorded once the panes are.
    fn tab_menu(&mut self, ui: &mut Ui) {
        let Some(target) = ui.state::<TabMenuTarget<T>>(self.dock).copied() else {
            return;
        };
        let chip = target.chip(self.dock);
        // The app can remove or move the tab while its menu is open; a menu for a chip that is
        // gone would act on a tab that is not there.
        if self
            .state
            .find_tab(target.tab)
            .is_none_or(|at| at.group != target.group)
        {
            Menu::close(ui, chip);
            ui.remove_state::<TabMenuTarget<T>>(self.dock);
            return;
        }
        let operations = &mut *self.operations;
        let tabs = &mut *self.tabs;
        Menu::for_id(chip)
            .style(self.menu_style)
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                tabs.tab_menu(
                    ui,
                    DockTabMenu {
                        tab: target.tab,
                        group: target.group,
                        operations,
                    },
                );
            });
        if !Menu::is_open(ui, chip) {
            ui.remove_state::<TabMenuTarget<T>>(self.dock);
        }
    }

    /// The drag's tooltip-layer feedback: a wash over the drop region and a ghost chip; a strip drop's caret is the strip's own. Both sense nothing, so hit-testing is unaffected.
    fn drag_feedback(&mut self, ui: &mut Ui, tab: T) {
        let dock = self.dock;
        if let Some(DropTarget::Region { wash: r, .. }) = self.drop {
            let preview = self.theme.preview.clone();
            ui.layer(Layer::Tooltip)
                .fixed_at(r.min)
                .max_size(r.size)
                .show(|ui| {
                    Panel::zstack()
                        .id(dock.with("preview"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .chrome(preview)
                        .show(ui, |_| {});
                });
        }
        let Some(p) = ui.pointer_pos() else {
            return;
        };
        let label = self.tabs.label(ui, tab);
        let ghost = self.theme.ghost.chrome.clone();
        let ghost_text = self.theme.ghost.text.apply(&ui.theme().text);
        let padding = self.theme.ghost_padding;
        ui.layer(Layer::Tooltip)
            .fixed_at(p + self.theme.ghost_offset)
            .show(|ui| {
                Panel::hstack()
                    .id(dock.with("ghost"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .padding(padding)
                    .chrome(ghost)
                    .show(ui, |ui| {
                        Text::new(label)
                            .id(dock.with("ghost_label"))
                            .style(&ghost_text)
                            .show(ui);
                    });
            });
    }
}
