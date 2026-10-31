//! Scroll viewports in splitter panes. The outer horizontal splitter holds a vertical list; its
//! right half splits vertically into a horizontal strip and a two-axis grid of buttons that zooms
//! about the pointer, `Scroll::both().zoomable()`, its clicks hit-tested through the zoom.

use crate::support;
use crate::support::Kit;
use palantir::glam::Vec2;
use palantir::{
    Button, Chrome, Configure, Corners, DragValue, Metrics, Panel, Scroll, Sizing, SplitHalf,
    Splitter, Stroke, Text, Ui, WidgetId, fmt,
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
    /// The list item "Reveal" brings to the top.
    reveal: i64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            h: 0.45,
            v: 0.5,
            clicked: None,
            reveal: 20,
        }
    }
}

/// The vertical list's id, which "Reveal" scrolls.
fn list_id() -> WidgetId {
    WidgetId::from_hash("showcase::scroll::list")
}

/// The id of the vertical list's item `n`, whose laid-out rect "Reveal" reads.
fn item_id(n: u32) -> WidgetId {
    list_id().with(n)
}

/// Scroll the list so item `n` sits at its top, from where the list and the item last laid out.
fn reveal(ui: &mut Ui, n: i64) {
    let n = u32::try_from(n).expect("the field's range holds an item's index");
    let rect = |ui: &Ui, id| {
        ui.response_for(id)
            .layout_rect
            .expect("the list is on the page, so it and its items laid out last frame")
    };
    let top = rect(ui, item_id(n)).min.y - rect(ui, list_id()).min.y;
    Scroll::set_offset(ui, list_id(), Vec2::new(0.0, top));
}

/// The zoomable pane's id, which the zoom readout and Reset view address.
fn grid_id() -> WidgetId {
    WidgetId::from_hash("showcase::scroll::grid")
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
                    .id(list_id())
                    .size((Sizing::FILL, Sizing::FILL))
                    .gap(M.space.xs)
                    .show(ui, |ui| {
                        for i in 0..ITEMS {
                            Item {
                                id: item_id(i),
                                number: i,
                                size: (Sizing::FILL, Sizing::fixed(28.0)),
                            }
                            .show(ui, k);
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
                                        Item {
                                            id: WidgetId::from_hash(("showcase::scroll::strip", i)),
                                            number: i,
                                            size: (Sizing::fixed(60.0), Sizing::FILL),
                                        }
                                        .show(ui, k);
                                    }
                                });
                        }),
                        SplitHalf::Second => pane(ui, k, "two-axis, zoomable", |ui| {
                            Scroll::both()
                                .id(grid_id())
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
        DragValue::new(&mut s.reveal)
            .range(0.0..=f64::from(ITEMS - 1))
            .size((Sizing::fixed(64.0), Sizing::HUG))
            .show(ui);
        if Button::new().label("Reveal").show(ui).clicked() {
            reveal(ui, s.reveal);
        }
        let zoom = fmt!(ui, "{:.2}×", Scroll::zoom(ui, grid_id()));
        support::readout(ui, k, "zoom", zoom);
        if Button::new().label("Reset view").show(ui).clicked() {
            Scroll::set_zoom(ui, grid_id(), 1.0);
            Scroll::set_offset(ui, grid_id(), Vec2::ZERO);
        }
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
#[derive(Debug)]
struct Item {
    id: WidgetId,
    number: u32,
    size: (Sizing, Sizing),
}

impl Item {
    fn show(self, ui: &mut Ui, k: Kit) {
        let number = self.number;
        Panel::hstack()
            .id(self.id)
            .size(self.size)
            .padding((M.space.md, M.space.xs))
            .chrome(
                Chrome::rounded(k.t.raised, Corners::all(M.radius.control))
                    .with_border(Stroke::new(k.t.hairline, M.stroke.hairline)),
            )
            .show(ui, |ui| {
                Text::new(fmt!(ui, "{number:02}"))
                    .style(&k.t.body())
                    .show(ui);
            });
    }
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

#[cfg(test)]
mod tests {
    use palantir::glam::UVec2;
    use palantir::{ColorScheme, Harness, Metrics, Palette, Scroll, Ui};

    use crate::pages::scroll::{build, list_id};
    use crate::support::Kits;

    /// Reveal brings item 20, the field's start, to the list's top: each item before it takes its
    /// 28 px and the xs gap of 4 px, so the offset is 20 × (28 + 4) = 640.
    #[test]
    fn reveal_brings_the_item_to_the_top() {
        assert_eq!(Metrics::DEFAULT.space.xs, 4.0);
        let k = Kits::new().kit(ColorScheme::Dark, &Palette::DARK);
        let mut record = |ui: &mut Ui| build(ui, k);
        let mut h = Harness::builder(UVec2::new(1000, 480)).mono().build();
        h.frame(&mut record);
        assert_eq!(Scroll::offset(h.ui(), list_id()).y, 0.0);
        h.click("Reveal");
        h.frame(&mut record);
        assert_eq!(Scroll::offset(h.ui(), list_id()).y, 640.0);
    }
}
