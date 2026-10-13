//! The two tab widgets: a page view bound to an index, and the strip on
//! its own with badges, close buttons and overflow.

use crate::support;
use crate::support::{Section, api};
use palantir::{
    Configure, Panel, Sizing, TabBadge, TabItem, TabOverflow, TabStrip, TabbedView, Text, Ui,
    WidgetId, fmt,
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
    /// Chips the strip demo has closed, so a close reads as a real removal.
    open: Vec<u64>,
    /// The frame's chips, rebuilt every frame since labels are interned per frame, into this buffer to keep its capacity.
    items: Vec<TabItem>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            page: 0,
            picked: 1,
            overflowing: 0,
            open: (0..4).collect(),
            items: Vec::with_capacity(MANY.len()),
        }
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::tabs::state");
    ui.with_state::<State, _>(state_id, |ui, s| {
        Section::new("Tabbed view")
            .apis(&[api!(type TabbedView)])
            .show(ui, |ui| {
                support::note(
                    ui,
                    "A strip over a content area, bound to a &mut usize as a combo box is. Click a \
                 chip, or focus the strip and move with the arrow keys, Home and End.",
                );
                Panel::vstack()
                    .size((Sizing::FILL, Sizing::fixed(180.0)))
                    .padding(10.0)
                    .background(support::well_bg())
                    .show(ui, |ui| {
                        TabbedView::new(&mut s.page, &PAGES).closable(false).show(
                            ui,
                            |ui, page| {
                                Panel::vstack()
                                    .size((Sizing::FILL, Sizing::FILL))
                                    .padding(14.0)
                                    .gap(6.0)
                                    .show(ui, |ui| {
                                        Text::new(PAGES[page])
                                            .style(&support::body_style())
                                            .show(ui);
                                        let line = fmt!(ui, "page index {page}");
                                        Text::new(line).style(&support::note_style()).show(ui);
                                    });
                            },
                        );
                    });
            });

        Section::new("The strip alone")
            .apis(&[api!(TabStrip::new), api!(TabBadge::Idle)])
            .show(ui, |ui| {
                support::note(
                    ui,
                    "TabStrip draws chips and nothing else. Every chip reserves the status \
                     dot's box and the even ones ink it, so a dot never shifts a neighbour. \
                     Close a chip with its × button.",
                );
                strip_demo(ui, s);
            });

        Section::new("Overflow")
            .apis(&[api!(TabOverflow::Menu)])
            .show(ui, |ui| {
                support::note(
                    ui,
                    "More chips than room. They pan under the wheel, and the trailing button lists \
                 every tab so one that scrolled out is still reachable.",
                );
                Panel::vstack()
                    .size((Sizing::fixed(320.0), Sizing::HUG))
                    .padding(10.0)
                    .background(support::well_bg())
                    .show(ui, |ui| {
                        s.items.clear();
                        for (i, label) in MANY.iter().enumerate() {
                            s.items.push(TabItem {
                                closable: false,
                                ..TabItem::new(i as u64, ui.intern(*label))
                            });
                        }
                        let hit = TabStrip::new(&s.items)
                            .selected(s.overflowing)
                            .overflow(TabOverflow::Menu)
                            .show(ui);
                        if let Some(i) = hit.activated() {
                            s.overflowing = i;
                        }
                    });
                support::readout(ui, "showing", MANY[s.overflowing]);
            });
    });
}

fn strip_demo(ui: &mut Ui, s: &mut State) {
    support::well(ui, |ui| {
        s.items.clear();
        for &key in &s.open {
            let badge = if key % 2 == 0 {
                TabBadge::On
            } else {
                TabBadge::Idle
            };
            s.items.push(TabItem {
                badge,
                ..TabItem::new(key, fmt!(ui, "layer {key}"))
            });
        }
        let hit = TabStrip::new(&s.items).selected(s.picked).show(ui);
        if let Some(slot) = hit.closed
            && s.open.len() > 1
        {
            s.open.remove(slot);
            s.picked = s.picked.min(s.open.len() - 1);
        } else if let Some(slot) = hit.activated() {
            s.picked = slot;
        }
    });
    let line = fmt!(ui, "{} open, slot {} selected", s.open.len(), s.picked);
    support::readout(ui, "chips", line);
}
