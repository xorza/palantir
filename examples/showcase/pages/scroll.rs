//! Scroll viewports in splitter panes. The outer horizontal splitter holds a vertical list; its
//! right half splits vertically into a horizontal strip and a two-axis grid of buttons that zooms
//! about the pointer, `Scroll::both().zoomable()`, its clicks hit-tested through the zoom.

use crate::support;
use crate::support::Kit;
use palantir::{
    Button, Chrome, Configure, Corners, Metrics, Panel, Scroll, Sizing, SplitHalf, Splitter,
    Stroke, Text, Ui, WidgetId, fmt,
};

const M: Metrics = Metrics::DEFAULT;
/// Items in the vertical list and the horizontal strip, enough to overflow either pane.
const ITEMS: u32 = 40;
/// The zoomable grid's side, in buttons.
const SIDE: u32 = 24;

#[derive(Debug)]
struct State {
    h: f32,
    v: f32,
    clicked: Option<Cell>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            h: 0.45,
            v: 0.5,
            clicked: None,
        }
    }
}

/// A button's place in the zoomable grid.
#[derive(Clone, Copy, Debug)]
struct Cell {
    row: u32,
    col: u32,
}

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    let state_id = WidgetId::from_hash("showcase::scroll::state");
    ui.with_state::<State, _>(state_id, |ui, s| split_panes(ui, k, s));
}

fn split_panes(ui: &mut Ui, k: Kit, s: &mut State) {
    let mut clicked = None;
    Splitter::row(&mut s.h)
        .min_pane(120.0)
        .show(ui, |ui, half| match half {
            SplitHalf::First => pane(ui, k, "vertical", |ui| {
                Scroll::vertical()
                    .size((Sizing::FILL, Sizing::FILL))
                    .gap(M.space.xs)
                    .show(ui, |ui| {
                        for i in 0..ITEMS {
                            item(ui, k, i, (Sizing::FILL, Sizing::fixed(28.0)));
                        }
                    });
            }),
            SplitHalf::Second => {
                Splitter::col(&mut s.v)
                    .min_pane(100.0)
                    .show(ui, |ui, half| match half {
                        SplitHalf::First => pane(ui, k, "horizontal", |ui| {
                            Scroll::horizontal()
                                .size((Sizing::FILL, Sizing::FILL))
                                .gap(M.space.xs)
                                .show(ui, |ui| {
                                    for i in 0..ITEMS {
                                        item(ui, k, i, (Sizing::fixed(60.0), Sizing::FILL));
                                    }
                                });
                        }),
                        SplitHalf::Second => pane(ui, k, "two-axis, zoomable", |ui| {
                            Scroll::both()
                                .zoomable()
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui, |ui| grid(ui, &mut clicked));
                        }),
                    });
            }
        });
    if clicked.is_some() {
        s.clicked = clicked;
    }

    support::row(ui, |ui| {
        let ratios = fmt!(ui, "row {:.2}  column {:.2}", s.h, s.v);
        support::readout(ui, k, "split ratios", ratios);
        let click = match s.clicked {
            Some(Cell { row, col }) => fmt!(ui, "row {row}, column {col}"),
            None => ui.intern("— click a button to check hit-testing through the zoom"),
        };
        support::readout(ui, k, "last click", click);
    });
}

#[track_caller]
fn pane(ui: &mut Ui, k: Kit, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .padding(M.space.sm)
        .gap(M.space.xs)
        .chrome(k.t.card())
        .show(ui, |ui| {
            Text::new(label).style(&k.t.caption()).show(ui);
            body(ui);
        });
}

/// One numbered item of the list or the strip, on the raised surface; the hairline keeps it apart
/// from a light ground, which the raised white matches.
fn item(ui: &mut Ui, k: Kit, i: u32, size: (Sizing, Sizing)) {
    Panel::hstack()
        .id_salt(i)
        .size(size)
        .padding((M.space.md, M.space.xs))
        .chrome(
            Chrome::rounded(k.t.raised, Corners::all(M.radius.control))
                .with_border(Stroke::new(k.t.hairline, M.stroke.hairline)),
        )
        .show(ui, |ui| {
            Text::new(fmt!(ui, "{i:02}")).style(&k.t.body()).show(ui);
        });
}

/// A grid of stock buttons, wider and taller than the pane, so it pans on both axes and a zoom
/// shows the hit test following the scale.
fn grid(ui: &mut Ui, clicked: &mut Option<Cell>) {
    Panel::vstack().gap(M.space.xs).show(ui, |ui| {
        for row in 0..SIDE {
            Panel::hstack().id_salt(row).gap(M.space.xs).show(ui, |ui| {
                for col in 0..SIDE {
                    // Formatted into the record arena, not a `String`: this runs for every button
                    // every frame.
                    let label = fmt!(ui, "{row},{col}");
                    if Button::new()
                        .id_salt(col)
                        .label(label)
                        .size((Sizing::fixed(56.0), Sizing::fixed(32.0)))
                        .show(ui)
                        .clicked()
                    {
                        *clicked = Some(Cell { row, col });
                    }
                }
            });
        }
    });
}
