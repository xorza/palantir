//! The two tab widgets side by side: a page view bound to an index, and
//! the strip on its own with badges, close buttons and overflow.

use crate::support;
use crate::support::{body_style, note_style, well_bg};
use palantir::{
    Configure, Panel, Sizing, TabBadge, TabItem, TabOverflow, TabStrip, TabStripResponse,
    TabbedView, Text, Ui, WidgetId, fmt,
};

const PAGES: [&str; 3] = ["Colour", "Geometry", "Metadata"];

const MANY: [&str; 9] = [
    "curves", "levels", "sharpen", "denoise", "rotate", "crop", "vignette", "grain", "export",
];

#[derive(Debug)]
struct State {
    page: usize,
    picked: usize,
    overflowing: usize,
    /// Chips the strip demo has closed, so a close reads as a real
    /// removal rather than a flash.
    open: Vec<u64>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            page: 0,
            picked: 1,
            overflowing: 0,
            open: (0..4).collect(),
        }
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::tabs::state");
    ui.with_state::<State, _>(state_id, |ui, s| {
        support::section(ui, "TABBED VIEW", |ui| {
            support::note(
                ui,
                "A strip over a content area, bound to a &mut usize — the same shape as \
                 ComboBox. Click a chip, or focus the strip and travel with the arrow keys, \
                 Home / End and Ctrl+Tab.",
            );
            Panel::vstack()
                .id_salt("view-well")
                .size((Sizing::FILL, Sizing::fixed(180.0)))
                .padding(10.0)
                .background(well_bg())
                .show(ui, |ui| {
                    TabbedView::new(&mut s.page, &PAGES)
                        .id_salt("pages")
                        .closable(false)
                        .show(ui, |ui, page| {
                            Panel::vstack()
                                .id_salt("page-body")
                                .size((Sizing::FILL, Sizing::FILL))
                                .padding(14.0)
                                .gap(6.0)
                                .show(ui, |ui| {
                                    Text::new(PAGES[page])
                                        .id_salt("page-title")
                                        .style(&body_style())
                                        .show(ui);
                                    let line = fmt!(ui, "page index {page}");
                                    Text::new(line)
                                        .id_salt("page-note")
                                        .style(&note_style())
                                        .show(ui);
                                });
                        });
                });
        });

        support::section(ui, "THE STRIP ALONE", |ui| {
            support::note(
                ui,
                "TabStrip draws chips and nothing else. The first chip carries a status dot \
                 whose box is reserved on every frame, so inking it never shifts its \
                 neighbours.",
            );
            strip_demo(ui, s);
        });

        support::section(ui, "OVERFLOW", |ui| {
            support::note(
                ui,
                "More chips than room. They pan under the wheel, and the trailing button \
                 lists every tab so one that scrolled out is still reachable.",
            );
            Panel::vstack()
                .id_salt("overflow-well")
                .size((Sizing::fixed(320.0), Sizing::HUG))
                .padding(10.0)
                .background(well_bg())
                .show(ui, |ui| {
                    let items: Vec<TabItem> = MANY
                        .iter()
                        .enumerate()
                        .map(|(i, label)| TabItem {
                            closable: false,
                            ..TabItem::new(i as u64, ui.intern(*label))
                        })
                        .collect();
                    let hit = TabStrip::new(&items)
                        .id_salt("overflow-strip")
                        .selected(s.overflowing)
                        .overflow(TabOverflow::Menu)
                        .show(ui);
                    if let Some(i) = hit.clicked.or(hit.keyed) {
                        s.overflowing = i;
                    }
                });
            let readout = fmt!(ui, "showing: {}", MANY[s.overflowing]);
            Text::new(readout)
                .id_salt("overflow-readout")
                .style(&note_style())
                .show(ui);
        });
    });
}

fn strip_demo(ui: &mut Ui, s: &mut State) {
    Panel::vstack()
        .id_salt("strip-well")
        .size((Sizing::FILL, Sizing::HUG))
        .padding(10.0)
        .gap(8.0)
        .background(well_bg())
        .show(ui, |ui| {
            let items: Vec<TabItem> = s
                .open
                .iter()
                // Every chip reserves the dot's box; the even ones ink
                // it. Both states side by side is the point — the
                // reserved box is what keeps an inked chip the width of
                // an idle one.
                .map(|&key| TabItem {
                    badge: if key % 2 == 0 {
                        TabBadge::On
                    } else {
                        TabBadge::Idle
                    },
                    ..TabItem::new(key, fmt!(ui, "layer {key}"))
                })
                .collect();
            let TabStripResponse {
                clicked,
                keyed,
                closed,
                ..
            } = TabStrip::new(&items)
                .id_salt("bare-strip")
                .selected(s.picked)
                .show(ui);
            let clicked = clicked.or(keyed);
            if let Some(slot) = closed
                && s.open.len() > 1
            {
                s.open.remove(slot);
                s.picked = s.picked.min(s.open.len() - 1);
            } else if let Some(slot) = clicked {
                s.picked = slot;
            }
        });
    let readout = fmt!(
        ui,
        "{} chips open, chip {} selected",
        s.open.len(),
        s.picked
    );
    Text::new(readout)
        .id_salt("strip-readout")
        .style(&note_style())
        .show(ui);
}
