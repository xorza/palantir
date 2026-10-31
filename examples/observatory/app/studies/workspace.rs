//! The workspace study: the other studies' views as dock panes under a menu bar and over a status
//! bar, the views moved into panes unchanged.

use std::mem;

use palantir::{
    Align, Chrome, ColorScheme, Configure, DockDrop, DockOperation, DockState, DockTabs, DockView,
    InternedStr, Key, MenuBar, MenuItem, Panel, SchemePreference, Scroll, Shortcut, Size, Sizing,
    SplitSide, Text, Ui, VAlign, WidgetId, fmt,
};

use crate::app::model::Observatory;
use crate::app::model::appearance::{Appearance, Preset};
use crate::app::scaffold::{StudyHeader, Tokens, api};
use crate::app::studies::sky_chart::{self, ChartRequest};
use crate::app::studies::{Study, catalog, details};
use crate::app::toggle_eyepiece;

pub(crate) fn header(ui: &mut Ui, t: Tokens) {
    StudyHeader {
        title: "Workspace",
        blurb: "The chart, the catalog, the details and the log as dock panes: the same view \
                functions the other studies call, moved into panes unchanged. Drag a tab onto a \
                pane's edge to split it; F8 opens the eyepiece, a second window on the same model.",
        keys: &["drag tab", "drag divider", "right-click tab", "F8"],
        apis: &[
            api!(type palantir::DockView),
            api!(type palantir::DockState),
            api!(MenuBar::new),
            api!(Ui::open_window),
        ],
        showcase: &["dock", "tabs", "overlays", "state & windows"],
        source: "examples/observatory/app/studies/workspace.rs",
    }
    .show(ui, t);
}

/// A pane of the workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Pane {
    Chart,
    Catalog,
    Details,
    Log,
}

impl Pane {
    const fn label(self) -> &'static str {
        match self {
            Self::Chart => "chart",
            Self::Catalog => "catalog",
            Self::Details => "details",
            Self::Log => "log",
        }
    }
}

/// The dock's arrangement and the operations a frame queues for it.
#[derive(Debug)]
struct State {
    dock: DockState<Pane>,
    operations: Vec<DockOperation<Pane>>,
}

impl Default for State {
    /// The chart pinned on the left, the details and the log tabbed on its right, the catalog
    /// under it.
    fn default() -> Self {
        let mut dock = DockState::new("observatory.workspace", Pane::Chart);
        let primary = dock.primary().id();
        for tab in [Pane::Details, Pane::Log, Pane::Catalog] {
            dock.apply(DockOperation::OpenTab { tab });
        }
        dock.apply(DockOperation::MoveTab {
            tab: Pane::Details,
            to: DockDrop::Split {
                group: primary,
                side: SplitSide::Right,
            },
        });
        if let Some(details) = dock.find_tab(Pane::Details) {
            dock.apply(DockOperation::MoveTab {
                tab: Pane::Log,
                to: DockDrop::Into {
                    group: details.group,
                    idx: 1,
                },
            });
        }
        dock.apply(DockOperation::MoveTab {
            tab: Pane::Catalog,
            to: DockDrop::Split {
                group: primary,
                side: SplitSide::Bottom,
            },
        });
        dock.apply(DockOperation::ActivateTab { tab: Pane::Chart });
        dock.apply(DockOperation::ActivateTab { tab: Pane::Details });
        Self {
            dock,
            operations: Vec::new(),
        }
    }
}

/// The panes' content, over the model.
#[derive(Debug)]
struct Panes<'a> {
    model: &'a mut Observatory,
    t: Tokens,
    switch: Option<Study>,
}

impl DockTabs for Panes<'_> {
    type Tab = Pane;

    fn title(&mut self, ui: &mut Ui, tab: Pane) -> InternedStr {
        ui.intern(tab.label())
    }

    fn content(&mut self, ui: &mut Ui, tab: Pane, _size: Option<Size>) {
        Panel::vstack()
            .id_salt(("pane", tab.label()))
            .size((Sizing::FILL, Sizing::FILL))
            .padding(8.0)
            .gap(8.0)
            .show(ui, |ui| match tab {
                Pane::Chart => {
                    if sky_chart::chart(ui, self.model, "pane") == Some(ChartRequest::ShowInCatalog)
                    {
                        self.switch = Some(Study::Catalog);
                    }
                }
                Pane::Catalog => catalog::show(ui, self.model, self.t),
                Pane::Details => details(ui, self.model, self.t),
                Pane::Log => log(ui, self.model, self.t),
            });
    }

    fn closable(&mut self, _tab: Pane) -> bool {
        false
    }
}

/// The log, newest last, scrolled to the end.
fn log(ui: &mut Ui, model: &Observatory, t: Tokens) {
    Scroll::vertical()
        .id_salt("log")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::vstack()
                .id_salt("lines")
                .size((Sizing::FILL, Sizing::HUG))
                .gap(2.0)
                .show(ui, |ui| {
                    if model.log.lines().len() == 0 {
                        Text::new("Nothing has happened yet.")
                            .style(&t.dim())
                            .show(ui);
                    }
                    for line in model.log.lines() {
                        Text::new(fmt!(ui, "{:>4}  {}", line.step, line.text))
                            .id_salt(("line", line.step))
                            .style(&t.mono(t.ink_dim))
                            .show(ui);
                    }
                });
        });
}

pub(crate) fn show(ui: &mut Ui, model: &mut Observatory, t: Tokens) -> Option<Study> {
    menu_bar(ui, model);
    let state_id = WidgetId::from_hash("observatory::workspace::state");
    // An `Option`, as `with_state` leaves a default in the row while the body runs, and the
    // dock's default allocates.
    let switch = ui.with_state::<Option<State>, _>(state_id, |ui, s| {
        let s = s.get_or_insert_with(State::default);
        let mut panes = Panes {
            model: &mut *model,
            t,
            switch: None,
        };
        Panel::vstack()
            .id_salt("dock")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                // The scan settles the arrangement before the record walks it, so a drag lands
                // on the frame it ends.
                s.operations.clear();
                DockView::scan(ui, &s.dock, &mut s.operations);
                for operation in s.operations.drain(..) {
                    s.dock.apply(operation);
                }
                DockView::new(&s.dock, &mut s.operations)
                    .min_pane(160.0)
                    .show(ui, &mut panes);
                for operation in s.operations.drain(..) {
                    s.dock.apply(operation);
                }
            });
        panes.switch
    });
    status_bar(ui, model, t);
    switch
}

/// File, View and Window, each a command the rest of the example also has.
fn menu_bar(ui: &mut Ui, model: &mut Observatory) {
    let preference = ui.scheme_preference();
    let mut picked = None;
    let mut night = None;
    let mut reset = false;
    let mut eyepiece = false;
    MenuBar::new().id_salt("menu").show(ui, |bar| {
        bar.menu("File", |ui, close| {
            reset = MenuItem::new("Reset session").show(ui, close).clicked();
        });
        bar.menu("View", |ui, close| {
            for (value, label) in [
                (SchemePreference::System, "System scheme"),
                (SchemePreference::Light, "Light scheme"),
                (SchemePreference::Dark, "Dark scheme"),
            ] {
                let mark = if value == preference {
                    "\u{25cf} "
                } else {
                    "   "
                };
                let label = fmt!(ui, "{mark}{label}");
                if MenuItem::new(label)
                    .id_salt(value as u8)
                    .show(ui, close)
                    .clicked()
                {
                    picked = Some(value);
                }
            }
            let on = model.appearance.preset() == Preset::NightVision;
            let label = if on {
                "Leave night vision"
            } else {
                "Night vision"
            };
            if MenuItem::new(label).show(ui, close).clicked() {
                night = Some(!on);
            }
        });
        bar.menu("Window", |ui, close| {
            eyepiece = MenuItem::new("Eyepiece")
                .shortcut_hint(Shortcut::key(Key::F8))
                .show(ui, close)
                .clicked();
        });
    });
    if let Some(preference) = picked {
        ui.set_scheme_preference(preference);
    }
    match night {
        Some(true) => {
            model.appearance.set_preset(Preset::NightVision);
            ui.set_scheme_preference(SchemePreference::Dark);
        }
        Some(false) => model.appearance.set_preset(Preset::Stock),
        None => {}
    }
    // The session starts over; the look is not the session's, and a fresh one would restart
    // its edit generation under a number a window has already applied.
    if reset {
        let look = mem::replace(&mut model.appearance, Appearance::new());
        *model = Observatory::new();
        model.appearance = look;
    }
    if eyepiece {
        toggle_eyepiece(ui);
    }
}

/// The selection, the plan's size and the shown scheme.
fn status_bar(ui: &mut Ui, model: &Observatory, t: Tokens) {
    Panel::hstack()
        .id_salt("status")
        .size((Sizing::FILL, Sizing::HUG))
        .padding((8.0, 4.0))
        .gap(24.0)
        .child_align(Align::v(VAlign::Center))
        .chrome(Chrome::fill(t.surround))
        .show(ui, |ui| {
            let selection = match model.selection {
                Some(id) => fmt!(ui, "{}", model.catalog.get(id).name),
                None => ui.intern("nothing selected"),
            };
            let planned = model.plan.entries().len();
            let scheme = match ui.color_scheme() {
                ColorScheme::Light => "light",
                ColorScheme::Dark => "dark",
            };
            for (salt, text) in [
                ("selection", selection),
                ("plan", fmt!(ui, "{planned} planned")),
                ("scheme", fmt!(ui, "{scheme} scheme")),
            ] {
                Text::new(text).id_salt(salt).style(&t.caption()).show(ui);
            }
        });
}
