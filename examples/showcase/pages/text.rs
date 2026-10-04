//! Text measurement and wrapping. The left column is single-text
//! wrapping mechanics in fixed-width containers — the simplest
//! demonstrations of `TextWrap::WrapWithOverflow` and the intrinsic-min
//! overflow rule. The right column is composition: Grid Auto under
//! constraint, a property grid, and a chat row whose Fill message column
//! reflows live.

use crate::support;
use crate::support::{Column, api, body_style, columns, note, section, well_bg};
use palantir::{
    Background, Block, Configure, Corners, Grid, Panel, RgbaF32, Sizing, Text, TextWrap, Track, Ui,
};

const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump!";

pub(crate) fn build(ui: &mut Ui) {
    columns(ui, |ui, column| match column {
        Column::Left => wrapping(ui),
        Column::Right => compositions(ui),
    });
}

fn wrapping(ui: &mut Ui) {
    section(ui, "Single line", &[api!(type Text)], |ui| {
        note(ui, "With no width to fit, text hugs its natural width.");
        Text::new("The quick brown fox jumps over the lazy dog")
            .style(&body_style())
            .show(ui);
    });

    section(ui, "Wrapping", &[api!(Text::text_wrap)], |ui| {
        note(ui, "The same paragraph at 360 and 140 px.");
        wrap_panel(ui, "wide-inner", 360.0, PARAGRAPH);
        wrap_panel(ui, "narrow-inner", 140.0, PARAGRAPH);
    });

    section(ui, "Overflow", &[api!(type TextWrap)], |ui| {
        note(
            ui,
            "Wrapping breaks only between words: a word wider than its 40 px slot \
                 spills out whole rather than breaking.",
        );
        wrap_panel(ui, "overflow-inner", 40.0, "supercalifragilistic");
    });
}

fn wrap_panel(ui: &mut Ui, id: &'static str, width: f32, text: &'static str) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::fixed(width), Sizing::HUG))
        .padding(8.0)
        .background(well_bg())
        .show(ui, |ui| {
            Text::new(text)
                .style(&body_style())
                .text_wrap(TextWrap::WrapWithOverflow)
                .show(ui);
        });
}

fn compositions(ui: &mut Ui) {
    section(ui, "Two Hug columns", &[api!(Track::HUG)], |ui| {
        note(
            ui,
            "Under a width limit the paragraph wraps to fit, and the label beside it keeps \
                 its natural width.",
        );
        Grid::new()
            .cols([Track::HUG, Track::HUG])
            .rows([Track::HUG])
            .line_gap(0.0)
            .gap(16.0)
            .show(ui, |ui| {
                Text::new(PARAGRAPH)
                    .style(&body_style())
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .grid_cell((0, 0))
                    .show(ui);
                Text::new("right column")
                    .style(&body_style())
                    .grid_cell((0, 1))
                    .show(ui);
            });
    });

    section(ui, "Property grid", &[api!(Track::FILL)], |ui| {
        note(ui, "A Hug label column and a Fill value column that wraps.");
        Grid::new()
            .size((Sizing::FILL, Sizing::HUG))
            .cols([Track::HUG, Track::FILL])
            .rows([Track::HUG, Track::HUG, Track::HUG])
            .line_gap(6.0)
            .gap(16.0)
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
                        .id_salt((i, 0))
                        .style(&body_style())
                        .grid_cell((r, 0))
                        .show(ui);
                    Text::new(value)
                        .id_salt((i, 1))
                        .style(&body_style())
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .grid_cell((r, 1))
                        .show(ui);
                }
            });
    });

    section(ui, "Chat", &[], |ui| {
        note(
            ui,
            "A Fixed avatar and a Fill message. Resize the window and the messages wrap \
                 again.",
        );
        Panel::vstack()
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
                    "Yeah — it clicked once I saw the property grid above actually wrap. \
                         Resize the window and this column flows again, live.",
                );
                chat_row(ui, "alice-2", support::A, "Right? layout is fun.");
            });
    });
}

/// One chat row: avatar (Fixed circle) + Fill wrapping message.
fn chat_row(ui: &mut Ui, key: &'static str, avatar: RgbaF32, message: &'static str) {
    Panel::hstack()
        .id_salt(("chat-row", key))
        .size((Sizing::FILL, Sizing::HUG))
        .gap(10.0)
        .show(ui, |ui| {
            Block::new()
                .id_salt(("avatar", key))
                .size((Sizing::fixed(36.0), Sizing::fixed(36.0)))
                .background(Background::rounded(avatar, Corners::all(18.0)))
                .show(ui);
            Text::new(message)
                .id_salt(("message", key))
                .style(&body_style())
                .size((Sizing::FILL, Sizing::HUG))
                .text_wrap(TextWrap::WrapWithOverflow)
                .show(ui);
        });
}
