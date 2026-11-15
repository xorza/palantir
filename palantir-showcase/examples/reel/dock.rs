//! Slide 7's dock: three tabs in one pane, which a drag splits.

use palantir::{
    Configure, DockOperation, DockState, DockTabs, InternedStr, Panel, RgbaF32, Size, SizeSpec,
    Text, Ui,
};

/// The dock's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Tab {
    Scene,
    Layers,
    Console,
}

impl Tab {
    const fn label(self) -> &'static str {
        match self {
            Self::Scene => "Scene",
            Self::Layers => "Layers",
            Self::Console => "Console",
        }
    }
}

/// The arrangement the slide opens on: every tab in one pane, the scene shown.
pub(crate) fn state() -> DockState<Tab> {
    let mut dock = DockState::new(Tab::Scene);
    for tab in [Tab::Layers, Tab::Console] {
        dock.apply(DockOperation::OpenTab { tab });
    }
    dock.apply(DockOperation::ActivateTab { tab: Tab::Scene });
    dock
}

/// What the panes show: each tab's name, large.
#[derive(Debug)]
pub(crate) struct Panes;

impl DockTabs for Panes {
    type Tab = Tab;

    fn label(&mut self, ui: &mut Ui, tab: Tab) -> InternedStr {
        ui.intern(tab.label())
    }

    fn body(&mut self, ui: &mut Ui, tab: Tab, _size: Option<Size>) {
        Panel::vstack()
            .size(SizeSpec::FILL)
            .padding(24.0)
            .show(ui, |ui| {
                Text::new(tab.label())
                    .font_size(36.0)
                    .color(RgbaF32::WHITE.with_alpha(0.5))
                    .show(ui);
            });
    }

    fn closable(&mut self, _tab: Tab) -> bool {
        false
    }
}
