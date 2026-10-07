//! Scroll viewports in splitter panes. The outer horizontal splitter holds a vertical scroll list; its right half splits vertically into a horizontal strip and a two-axis grid.

use crate::support;
use crate::support::{on_swatch_style, readout, swatch_bg, well_bg};
use palantir::{
    Configure, Panel, RgbaF32, Scroll, Sizing, SplitHalf, Splitter, Text, Ui, WidgetId, fmt,
};

#[derive(Debug)]
struct State {
    h: f32,
    v: f32,
}

impl Default for State {
    fn default() -> Self {
        Self { h: 0.45, v: 0.5 }
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::scroll::state");
    ui.with_state::<State, _>(state_id, split_panes);
}

fn split_panes(ui: &mut Ui, s: &mut State) {
    Splitter::row(&mut s.h)
        .min_pane(120.0)
        .show(ui, |ui, half| match half {
            SplitHalf::First => pane(ui, "vertical", |ui| {
                Scroll::vertical()
                    .size((Sizing::FILL, Sizing::FILL))
                    .gap(4.0)
                    .show(ui, |ui| {
                        for i in 0..40 {
                            row(ui, i);
                        }
                    });
            }),
            SplitHalf::Second => {
                Splitter::column(&mut s.v)
                    .min_pane(100.0)
                    .show(ui, |ui, half| match half {
                        SplitHalf::First => pane(ui, "horizontal", |ui| {
                            Scroll::horizontal()
                                .size((Sizing::FILL, Sizing::FILL))
                                .gap(4.0)
                                .show(ui, |ui| {
                                    for i in 0..40 {
                                        col(ui, i);
                                    }
                                });
                        }),
                        SplitHalf::Second => pane(ui, "two-axis", |ui| {
                            Scroll::both()
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui, grid);
                        }),
                    });
            }
        });

    let line = fmt!(ui, "row {:.2}  column {:.2}", s.h, s.v);
    readout(ui, "split ratios", line);
}

#[track_caller]
fn pane(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .gap(6.0)
        .background(well_bg())
        .show(ui, |ui| {
            Text::new(label).style(&support::caption_style()).show(ui);
            body(ui);
        });
}

fn row(ui: &mut Ui, i: u32) {
    Panel::hstack()
        .id_salt(i)
        .size((Sizing::FILL, Sizing::fixed(28.0)))
        .padding((10.0, 6.0))
        .background(swatch_bg(ramp(i)))
        .show(ui, |ui| {
            let label = fmt!(ui, "row {i:02}");
            Text::new(label).style(&on_swatch_style()).show(ui);
        });
}

fn col(ui: &mut Ui, i: u32) {
    Panel::vstack()
        .id_salt(i)
        .size((Sizing::fixed(60.0), Sizing::FILL))
        .padding((6.0, 10.0))
        .background(swatch_bg(ramp(i)))
        .show(ui, |ui| {
            let label = fmt!(ui, "col {i:02}");
            Text::new(label).style(&on_swatch_style()).show(ui);
        });
}

fn grid(ui: &mut Ui) {
    // One Hug-sized child holding a 12×16 grid of nested stacks; both-axes Scroll measures with INF, so the stacks size to content and overflow both sides.
    Panel::vstack().gap(4.0).show(ui, |ui| {
        for r in 0..16u32 {
            Panel::hstack().id_salt(r).gap(4.0).show(ui, |ui| {
                for c in 0..12u32 {
                    Panel::hstack()
                        .id_salt(c)
                        .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                        .padding((6.0, 4.0))
                        .background(swatch_bg(ramp(r * 12 + c)))
                        .show(ui, |ui| {
                            let label = fmt!(ui, "{r},{c}");
                            Text::new(label)
                                .style(&on_swatch_style().with_font_size(11.0))
                                .show(ui);
                        });
                }
            });
        }
    });
}

/// Teal → purple → orange sweep so panning shows progress; these colours are demo content, not theme, in the swatch palette's hues.
fn ramp(i: u32) -> RgbaF32 {
    let t = (i % 40) as f32 / 40.0;
    let (from, to, u) = if t < 0.5 {
        (support::A, support::D, t * 2.0)
    } else {
        (support::D, support::B, (t - 0.5) * 2.0)
    };
    support::mix(from, to, u)
}
