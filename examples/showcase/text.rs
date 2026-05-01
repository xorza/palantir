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
            // Step B in action: two `Hug` columns sharing a constrained
            // surface. The paragraph column shrinks to its intrinsic min
            // floor + slack, the right-column label keeps its natural
            // width, and the paragraph wraps cleanly inside its resolved
            // column.
            section(
                ui,
                "two-hug-columns",
                "two Hug columns: paragraph wraps to fit, label stays natural",
                |ui| {
                    Grid::with_id("two-hug-inner")
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

            // Property-grid pattern: Hug label column + Fill value column
            // with wrapping text. The label hugs to its natural width;
            // the value column gets the rest of the surface and wraps the
            // paragraph inside it. The motivating use case behind Step B.
            //
            // The Grid is `Sizing::FILL × Sizing::Hug` so it spans the
            // section's full width — same gotcha as `HStack { Fill }` with
            // a Hug parent: a Fill *column* needs the *grid* to be Fill
            // (or Fixed) on that axis, otherwise leftover is zero and
            // the column collapses. Same rule CSS Grid follows for
            // `display: grid; width: auto; grid-template-columns: 1fr`.
            section(
                ui,
                "property-grid",
                "property grid: Hug label column + Fill value column with wrapping",
                |ui| {
                    Grid::with_id("property-grid-inner")
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                        .gap_xy(6.0, 16.0)
                        .show(ui, |ui| {
                            Text::new("Title:").size_px(14.0).grid_cell((0, 0)).show(ui);
                            Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui);
                            Text::new("Description:")
                                .size_px(14.0)
                                .grid_cell((1, 0))
                                .show(ui);
                            Text::new(PARAGRAPH)
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((1, 1))
                                .show(ui);
                            Text::new("Tags:").size_px(14.0).grid_cell((2, 0)).show(ui);
                            Text::new("layout, grid, intrinsic, wrapping, css")
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((2, 1))
                                .show(ui);
                        });
                },
            );

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
