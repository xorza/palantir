use crate::swatch;
use palantir::Track;
use palantir::{
    Background, Color, Configure, Corners, Frame, Grid, Panel, Sizing, Text, TextStyle, Ui,
};
use std::rc::Rc;

const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump!";

/// "text" tab — basic single-text wrapping mechanics in fixed-width
/// containers. The simplest demonstrations of `Text::new(...).wrapping()`
/// and the intrinsic-min overflow rule.
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
                    Text::new("The quick brown fox jumps over the lazy dog").show(ui);
                },
            );

            section(ui, "wide", "wrapping paragraph in a 360 px panel", |ui| {
                Panel::vstack()
                    .with_id("wide-inner")
                    .size((Sizing::Fixed(360.0), Sizing::Hug))
                    .padding(8.0)
                    .show(ui, |ui| {
                        Text::new(PARAGRAPH)
                            .style(TextStyle::default().with_font_size(14.0))
                            .wrapping()
                            .show(ui);
                    });
            });

            section(
                ui,
                "narrow",
                "same text in a 140 px panel — wraps to more lines",
                |ui| {
                    Panel::vstack()
                        .with_id("narrow-inner")
                        .size((Sizing::Fixed(140.0), Sizing::Hug))
                        .padding(8.0)
                        .show(ui, |ui| {
                            Text::new(PARAGRAPH)
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .show(ui);
                        });
                },
            );

            section(
                ui,
                "overflow",
                "unbreakable word in a 40 px slot — overflows at intrinsic_min",
                |ui| {
                    Panel::vstack()
                        .with_id("overflow-inner")
                        .size((Sizing::Fixed(40.0), Sizing::Hug))
                        .padding(4.0)
                        .show(ui, |ui| {
                            Text::new("supercalifragilistic")
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .show(ui);
                        });
                },
            );
        });
}

/// "text layouts" tab — composition patterns from the intrinsic-dimensions
/// plan: Grid Auto under constraint (Step B), property grid (Step B), and
/// chat-message HStack with Fill text (Step C).
pub fn build_layouts(ui: &mut Ui) {
    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            section(
                ui,
                "two-hug-columns",
                "two Hug columns: paragraph wraps to fit, label stays natural",
                |ui| {
                    Grid::new()
                        .with_id("two-hug-inner")
                        .cols(Rc::from([Track::hug(), Track::hug()]))
                        .rows(Rc::from([Track::hug()]))
                        .gap_xy(0.0, 16.0)
                        .show(ui, |ui| {
                            Text::new(PARAGRAPH)
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((0, 0))
                                .show(ui);
                            Text::new("right column")
                                .style(TextStyle::default().with_font_size(14.0))
                                .grid_cell((0, 1))
                                .show(ui);
                        });
                },
            );

            section(
                ui,
                "property-grid",
                "property grid: Hug label column + Fill value column with wrapping",
                |ui| {
                    Grid::new()
                        .with_id("property-grid-inner")
                        .size((Sizing::FILL, Sizing::Hug))
                        .cols(Rc::from([Track::hug(), Track::fill()]))
                        .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                        .gap_xy(6.0, 16.0)
                        .show(ui, |ui| {
                            Text::new("Title:")
                                .style(TextStyle::default().with_font_size(14.0))
                                .grid_cell((0, 0))
                                .show(ui);
                            Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui);
                            Text::new("Description:")
                                .style(TextStyle::default().with_font_size(14.0))
                                .grid_cell((1, 0))
                                .show(ui);
                            Text::new(PARAGRAPH)
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((1, 1))
                                .show(ui);
                            Text::new("Tags:")
                                .style(TextStyle::default().with_font_size(14.0))
                                .grid_cell((2, 0))
                                .show(ui);
                            Text::new("layout, grid, intrinsic, wrapping, css")
                                .style(TextStyle::default().with_font_size(14.0))
                                .wrapping()
                                .grid_cell((2, 1))
                                .show(ui);
                        });
                },
            );

            section(
                ui,
                "chat-message",
                "chat: Fixed avatar + Fill wrapping message",
                |ui| {
                    Panel::vstack()
                        .with_id("chat-list")
                        .size((Sizing::FILL, Sizing::Hug))
                        .gap(8.0)
                        .show(ui, |ui| {
                            chat_row(
                                ui,
                                "alice-1",
                                swatch::A,
                                "Hey! Did you finish reading docs/intrinsics.md last night?",
                            );
                            chat_row(
                                ui,
                                "bob-1",
                                swatch::B,
                                "Yeah — the Step B/C distinction finally clicked once I saw \
                             the showcase property-grid card actually wrapping. Resizing \
                             the window confirms the message column reflows live.",
                            );
                            chat_row(ui, "alice-2", swatch::A, "Right? layout is fun.");
                        });
                },
            );
        });
}

/// Plain section: title + body. No card decoration; the surrounding
/// showcase panel already contains the demo.
fn section(ui: &mut Ui, id: &'static str, title: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .with_id(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(6.0)
        .show(ui, |ui| {
            Text::new(title)
                .with_id(("section-title", id))
                .style(TextStyle::default().with_font_size(12.0))
                .show(ui);
            body(ui);
        });
}

/// One chat row: avatar (Fixed circle) + Fill wrapping message.
fn chat_row(ui: &mut Ui, key: &'static str, avatar_color: Color, message: &'static str) {
    Panel::hstack()
        .with_id(("chat-row", key))
        .size((Sizing::FILL, Sizing::Hug))
        .gap(10.0)
        .show(ui, |ui| {
            Frame::new()
                .with_id(("avatar", key))
                .size((Sizing::Fixed(36.0), Sizing::Fixed(36.0)))
                .background(Background {
                    fill: avatar_color,
                    radius: Corners::all(18.0),
                    ..Default::default()
                })
                .show(ui);
            Text::new(message)
                .with_id(("message", key))
                .style(TextStyle::default().with_font_size(14.0))
                .size((Sizing::FILL, Sizing::Hug))
                .wrapping()
                .show(ui);
        });
}
