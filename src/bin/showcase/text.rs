//! Text measurement and wrapping. Left column: single-text wrapping
//! mechanics in fixed-width containers — the simplest demonstrations
//! of `TextWrap::WrapWithOverflow` and the intrinsic-min overflow
//! rule. Right column: composition patterns from the
//! intrinsic-dimensions plan — Grid Auto under constraint, a property
//! grid, and a chat-message HStack with Fill wrapping text.

use crate::support;
use crate::support::section;
use aperture::{
    Background, Color, Configure, Corners, Frame, Grid, Panel, Sizing, Text, TextStyle, TextWrap,
    Track, Ui,
};

const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump!";

fn body_style() -> TextStyle {
    TextStyle::default().with_font_size(14.0)
}

pub(crate) fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        support::header(
            ui,
            "Text wrapping mechanics (left) and intrinsic-dimension composition \
             patterns (right). Resize the window — the right column reflows live.",
        );
        Panel::hstack()
            .auto_id()
            .gap(24.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::vstack()
                    .id_salt("col-l")
                    .gap(16.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, wrapping);
                Panel::vstack()
                    .id_salt("col-r")
                    .gap(16.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, compositions);
            });
    });
}

fn wrapping(ui: &mut Ui) {
    section(
        ui,
        "single",
        "single-line label, hugs natural width",
        |ui| {
            Text::new("The quick brown fox jumps over the lazy dog")
                .auto_id()
                .show(ui);
        },
    );

    section(ui, "wide", "wrapping paragraph in a 360 px panel", |ui| {
        wrap_panel(ui, "wide-inner", 360.0, PARAGRAPH);
    });

    section(
        ui,
        "narrow",
        "same text in a 140 px panel — wraps to more lines",
        |ui| {
            wrap_panel(ui, "narrow-inner", 140.0, PARAGRAPH);
        },
    );

    section(
        ui,
        "overflow",
        "unbreakable word in a 40 px slot — overflows at intrinsic_min",
        |ui| {
            wrap_panel(ui, "overflow-inner", 40.0, "supercalifragilistic");
        },
    );
}

fn wrap_panel(ui: &mut Ui, id: &'static str, width: f32, text: &'static str) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::fixed(width), Sizing::HUG))
        .padding(8.0)
        .background(support::panel_bg())
        .show(ui, |ui| {
            Text::new(text)
                .auto_id()
                .style(body_style())
                .text_wrap(TextWrap::WrapWithOverflow)
                .show(ui);
        });
}

fn compositions(ui: &mut Ui) {
    section(
        ui,
        "two-hug-columns",
        "two Hug columns: paragraph wraps to fit, label stays natural",
        |ui| {
            Grid::new()
                .id_salt("two-hug-inner")
                .cols([Track::hug(), Track::hug()])
                .rows([Track::hug()])
                .gap_xy(0.0, 16.0)
                .show(ui, |ui| {
                    Text::new(PARAGRAPH)
                        .auto_id()
                        .style(body_style())
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .grid_cell((0, 0))
                        .show(ui);
                    Text::new("right column")
                        .auto_id()
                        .style(body_style())
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
                .id_salt("property-grid-inner")
                .size((Sizing::FILL, Sizing::HUG))
                .cols([Track::hug(), Track::fill()])
                .rows([Track::hug(), Track::hug(), Track::hug()])
                .gap_xy(6.0, 16.0)
                .show(ui, |ui| {
                    let rows = [
                        (
                            "Title:",
                            "Lorem Ipsum is simply dummy text of the printing industry.",
                        ),
                        ("Description:", PARAGRAPH),
                        ("Tags:", "layout, grid, intrinsic, wrapping, css"),
                    ];
                    for (i, (label, value)) in rows.into_iter().enumerate() {
                        let r = i as u16;
                        Text::new(label)
                            .id_salt(("prop-label", i))
                            .style(body_style())
                            .grid_cell((r, 0))
                            .show(ui);
                        Text::new(value)
                            .id_salt(("prop-value", i))
                            .style(body_style())
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .grid_cell((r, 1))
                            .show(ui);
                    }
                });
        },
    );

    section(
        ui,
        "chat-message",
        "chat: Fixed avatar + Fill wrapping message",
        |ui| {
            Panel::vstack()
                .id_salt("chat-list")
                .size((Sizing::FILL, Sizing::HUG))
                .gap(8.0)
                .show(ui, |ui| {
                    chat_row(
                        ui,
                        "alice-1",
                        support::A,
                        "Hey! Did you finish reading src/layout/intrinsic.md last night?",
                    );
                    chat_row(
                        ui,
                        "bob-1",
                        support::B,
                        "Yeah — the Step B/C distinction finally clicked once I saw \
                         the showcase property-grid card actually wrapping. Resizing \
                         the window confirms the message column reflows live.",
                    );
                    chat_row(ui, "alice-2", support::A, "Right? layout is fun.");
                });
        },
    );
}

/// One chat row: avatar (Fixed circle) + Fill wrapping message.
fn chat_row(ui: &mut Ui, key: &'static str, avatar_color: Color, message: &'static str) {
    Panel::hstack()
        .id_salt(("chat-row", key))
        .size((Sizing::FILL, Sizing::HUG))
        .gap(10.0)
        .show(ui, |ui| {
            Frame::new()
                .id_salt(("avatar", key))
                .size((Sizing::fixed(36.0), Sizing::fixed(36.0)))
                .background(Background {
                    fill: avatar_color.into(),
                    corners: Corners::all(18.0),
                    ..Default::default()
                })
                .show(ui);
            Text::new(message)
                .id_salt(("message", key))
                .style(body_style())
                .size((Sizing::FILL, Sizing::HUG))
                .text_wrap(TextWrap::WrapWithOverflow)
                .show(ui);
        });
}
