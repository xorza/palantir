use palantir::primitives::Track;
use palantir::{Color, Configure, Grid, Panel, Sizing, Stroke, Styled, Text, Ui};
use std::rc::Rc;

const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump!";

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            section(
                ui,
                "single",
                "single-line label, hugs natural width",
                |ui| {
                    Text::new("The quick brown fox jumps over the lazy dog")
                        .size_px(16.0)
                        .show(ui);
                },
            );

            section(ui, "wide", "wrapping paragraph in a 360 px panel", |ui| {
                Panel::vstack_with_id("wide-inner")
                    .size((Sizing::Fixed(360.0), Sizing::Hug))
                    .padding(8.0)
                    .show(ui, |ui| {
                        Text::new(PARAGRAPH).size_px(14.0).wrapping().show(ui);
                    });
            });

            section(
                ui,
                "narrow",
                "same text in a 140 px panel — wraps to more lines",
                |ui| {
                    Panel::vstack_with_id("narrow-inner")
                        .size((Sizing::Fixed(140.0), Sizing::Hug))
                        .padding(8.0)
                        .show(ui, |ui| {
                            Text::new(PARAGRAPH).size_px(14.0).wrapping().show(ui);
                        });
                },
            );

            section(
                ui,
                "overflow",
                "unbreakable word in a 40 px slot — overflows at intrinsic_min",
                |ui| {
                    Panel::vstack_with_id("overflow-inner")
                        .size((Sizing::Fixed(40.0), Sizing::Hug))
                        .padding(4.0)
                        .show(ui, |ui| {
                            Text::new("supercalifragilistic")
                                .size_px(14.0)
                                .wrapping()
                                .show(ui);
                        });
                },
            );

            // Known gap. Two `Auto` (Hug) grid columns with a wrapping paragraph in
            // column 0. Layout passes `available_w = INFINITY` to the Hug column's
            // children (the WPF unresolved-track trick), so the paragraph never sees
            // a finite width and shapes at its full natural width — overflowing the
            // surface. Fix requires Option B (intrinsic-dimensions pre-pass) — see
            // `docs/text.md`. The page header below labels it so it's not mistaken
            // for working behavior.
            section(
                ui,
                "gap-grid",
                "BUG (Option B gap): wrapping text in a Grid `Auto` column overflows",
                |ui| {
                    Grid::with_id("gap-grid-inner")
                        .cols(Rc::from([Track::hug(), Track::hug()]))
                        .rows(Rc::from([Track::hug()]))
                        .gap_xy(0.0, 16.0)
                        .show(ui, |ui| {
                            Text::new(PARAGRAPH)
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 0))
                                .show(ui);
                            Text::new("right column")
                                .size_px(14.0)
                                .grid_cell((0, 1))
                                .show(ui);
                        });
                },
            );
        });
}

/// Card-style wrapper: a labeled rule above a body. Makes each demo visually
/// distinct so the working/broken cases are easy to compare side by side.
///
/// The title's `Text` gets an explicit id derived from `id` because
/// `#[track_caller]` doesn't propagate through closure bodies — without
/// the explicit id, every section's title would resolve to the same call
/// site inside `section()` and collide.
fn section(ui: &mut Ui, id: &'static str, title: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack_with_id(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(6.0)
        .padding(8.0)
        .fill(Color::rgb(0.16, 0.18, 0.22))
        .stroke(Stroke {
            width: 1.0,
            color: Color::rgb(0.30, 0.34, 0.42),
        })
        .radius(4.0)
        .show(ui, |ui| {
            Text::with_id(("section-title", id), title)
                .size_px(12.0)
                .color(Color::rgb(0.70, 0.74, 0.82))
                .show(ui);
            body(ui);
        });
}
