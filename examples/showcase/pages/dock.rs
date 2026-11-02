//! A live dock: drag a chip to another pane's edge to split, into its strip to join, onto a divider to resize.

use crate::support;
use crate::support::Kit;
use palantir::{
    Block, Button, Chrome, ColorPicker, Configure, Corners, DockDrop, DockOperation, DockState,
    DockTabMenu, DockTabs, DockView, InternedStr, MenuItem, Metrics, Panel, RgbaF32, Size, Sizing,
    SplitSide, TabChip, Text, Ui, WidgetId, fmt,
};

const M: Metrics = Metrics::DEFAULT;

/// The showcase's tab key: a small `Copy` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Canvas,
    Layers,
    History,
    Console,
}

const OPENABLE: [Tab; 3] = [Tab::Layers, Tab::History, Tab::Console];

impl Tab {
    const fn label(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::Layers => "layers",
            Self::History => "history",
            Self::Console => "console",
        }
    }

    const fn blurb(self) -> &'static str {
        match self {
            Self::Canvas => "The pinned tab, showing the color Layers picks. It refuses to close.",
            Self::Layers => {
                "Pick the canvas color here; drag this chip onto a pane's edge to split."
            }
            Self::History => "The last operation a gesture asked of the arrangement.",
            Self::Console => "Right-click a chip for the split menu.",
        }
    }
}

#[derive(Debug)]
struct State {
    dock: DockState<Tab>,
    /// The frame's operation sink, reused so the page allocates once.
    operations: Vec<DockOperation<Tab>>,
    /// The last operation a gesture asked of the arrangement, for the log.
    last: Option<DockOperation<Tab>>,
    /// The canvas's color, which the layers pane picks.
    color: RgbaF32,
}

impl State {
    /// Run the frame's operations on the arrangement, keeping the last for the log.
    fn apply_operations(&mut self) {
        for operation in self.operations.drain(..) {
            self.last = Some(operation);
            self.dock.apply(operation);
        }
    }
}

impl Default for State {
    fn default() -> Self {
        let mut dock = DockState::new("showcase.dock", Tab::Canvas);
        let primary = dock.primary().id();
        for tab in OPENABLE {
            dock.apply(DockOperation::OpenTab { tab });
        }
        dock.apply(DockOperation::MoveTab {
            tab: Tab::Console,
            to: DockDrop::Split {
                group: primary,
                side: SplitSide::Bottom,
            },
        });
        dock.apply(DockOperation::ActivateTab { tab: Tab::Canvas });
        Self {
            dock,
            operations: Vec::new(),
            last: None,
            color: RgbaF32::hex(0x3b82f6),
        }
    }
}

/// What the panes show: the canvas's color, which the layers pane picks, and the last operation.
#[derive(Debug)]
struct Panes<'a> {
    k: Kit,
    color: &'a mut RgbaF32,
    last: Option<DockOperation<Tab>>,
}

impl DockTabs for Panes<'_> {
    type Tab = Tab;

    fn title(&mut self, ui: &mut Ui, tab: Tab) -> InternedStr {
        ui.intern(tab.label())
    }

    fn content(&mut self, ui: &mut Ui, tab: Tab, size: Option<Size>) {
        let k = self.k;
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(M.space.md)
            .gap(M.space.sm)
            .chrome(k.t.card())
            .show(ui, |ui| {
                Text::new(tab.label()).style(&k.t.body()).show(ui);
                Text::new(tab.blurb()).style(&k.t.caption()).show(ui);
                match tab {
                    Tab::Canvas => {
                        Block::new()
                            .size((Sizing::FILL, Sizing::FILL))
                            .chrome(Chrome::rounded(*self.color, Corners::all(M.radius.card)))
                            .show(ui);
                    }
                    Tab::Layers => {
                        ColorPicker::new(self.color).show(ui);
                    }
                    Tab::History => {
                        let last = describe(ui, self.last);
                        Text::new(last).style(&k.t.mono(k.t.ink)).show(ui);
                    }
                    Tab::Console => {}
                }
                let measured = match size {
                    Some(s) => fmt!(ui, "content area {:.0} x {:.0}", s.w, s.h),
                    None => ui.intern("content area not laid out yet"),
                };
                Text::new(measured).style(&k.t.caption()).show(ui);
            });
    }

    fn closable(&mut self, tab: Tab) -> bool {
        tab != Tab::Canvas
    }

    fn tab_content(&mut self, ui: &mut Ui, tab: Tab, chip: &TabChip) {
        chip.show_label(ui);
        if tab == Tab::Canvas {
            Block::new()
                .id(chip.id.with("dot"))
                .size((Sizing::fixed(6.0), Sizing::fixed(6.0)))
                .chrome(Chrome::rounded(chip.text.color, Corners::all(3.0)))
                .show(ui);
        }
    }

    fn has_tab_menu(&mut self, _tab: Tab) -> bool {
        true
    }

    fn tab_menu(&mut self, ui: &mut Ui, menu: DockTabMenu<'_, Tab>) {
        let right = MenuItem::new("Split right").show(ui).clicked();
        let down = MenuItem::new("Split down").show(ui).clicked();
        let side = if down {
            Some(SplitSide::Bottom)
        } else {
            right.then_some(SplitSide::Right)
        };
        if let Some(side) = side {
            menu.operations.push(DockOperation::MoveTab {
                tab: menu.tab,
                to: DockDrop::Split {
                    group: menu.group,
                    side,
                },
            });
        }
    }
}

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    let state_id = WidgetId::from_hash("showcase::dock::state");
    ui.with_state::<State, _>(state_id, |ui, s| {
        reopen_row(ui, k, s);
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                // The scan settles the arrangement before the record walks it, so a click draws on the frame it lands.
                s.operations.clear();
                DockView::scan(ui, &s.dock, &mut s.operations);
                s.apply_operations();
                let mut panes = Panes {
                    k,
                    color: &mut s.color,
                    last: s.last,
                };
                DockView::new(&s.dock, &mut s.operations)
                    .min_pane(140.0)
                    .show(ui, &mut panes);
                s.apply_operations();
            });
    });
}

/// Re-open closed tabs so the demo can't be emptied down to the pinned pane.
fn reopen_row(ui: &mut Ui, k: Kit, s: &mut State) {
    let all_open = OPENABLE.iter().all(|&t| s.dock.find_tab(t).is_some());
    let line = if all_open {
        "every tab is open — drag a chip onto a pane edge to split it"
    } else {
        "closed tabs reopen in the focused pane:"
    };
    support::row(ui, |ui| {
        Text::new(line).style(&k.t.caption()).show(ui);
        for tab in OPENABLE {
            if s.dock.find_tab(tab).is_some() {
                continue;
            }
            if Button::new()
                .id_salt(tab.label())
                .label(tab.label())
                .show(ui)
                .clicked()
            {
                s.dock.apply(DockOperation::OpenTab { tab });
            }
        }
        if Button::new()
            .id_salt("reset")
            .label("Reset layout")
            .show(ui)
            .clicked()
        {
            *s = State::default();
        }
    });
}

/// `operation` in words, for the log.
fn describe(ui: &mut Ui, operation: Option<DockOperation<Tab>>) -> InternedStr {
    match operation {
        None => ui.intern("—"),
        Some(DockOperation::ActivateTab { tab }) => fmt!(ui, "activated {}", tab.label()),
        Some(DockOperation::OpenTab { tab }) => fmt!(ui, "opened {}", tab.label()),
        Some(DockOperation::CloseTab { tab }) => fmt!(ui, "closed {}", tab.label()),
        Some(DockOperation::MoveTab { tab, .. }) => fmt!(ui, "moved {}", tab.label()),
        Some(DockOperation::SetRatio { ratio, .. }) => fmt!(ui, "resized a split to {ratio:.2}"),
        Some(_) => ui.intern("focused a pane"),
    }
}
