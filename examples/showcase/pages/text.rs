//! Text measurement and wrapping. The left column is single-text wrapping in a box the knobs size (`TextWrap::WrapWithOverflow` and the intrinsic-min overflow rule); the right is composition: Grid Auto under constraint, a property grid, and a chat row whose Fill message column reflows live.

use crate::scaffold::api;
use crate::support;
use crate::support::{Col, Kit, Knob, Section};
use palantir::{
    Block, Chrome, Configure, Corners, Grid, Metrics, Panel, RgbaF32, Sizing, Text, TextWrap,
    Track, Ui, WidgetId,
};

const M: Metrics = Metrics::DEFAULT;

const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump!";

/// The wrapping demo's knobs.
#[derive(Debug)]
struct State {
    width: f64,
    font_size: f64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            width: 240.0,
            font_size: f64::from(M.text.body),
        }
    }
}

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    let state_id = WidgetId::from_hash("showcase::text::state");
    ui.with_state::<State, _>(state_id, |ui, s| {
        support::cols(ui, |ui, col| match col {
            Col::Left => wrapping(ui, k, s),
            Col::Right => compositions(ui, k),
        });
    });
}

fn wrapping(ui: &mut Ui, k: Kit, s: &mut State) {
    Section::new("Single line")
        .apis(&[api!(type Text)])
        .show(ui, k, |ui| {
            support::note(ui, k, "With no width to fit, text hugs its natural width.");
            Text::new("The quick brown fox jumps over the lazy dog")
                .style(&k.t.body())
                .show(ui);
        });

    Section::new("Wrapping")
        .apis(&[api!(Text::text_wrap)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A paragraph in a box of the width and at the size the knobs set: it wraps \
                 again between words as either moves.",
            );
            Knob {
                label: "width",
                range: 60.0..=480.0,
                step: 10.0,
                unit: " px",
            }
            .show(ui, k, &mut s.width);
            Knob {
                label: "font size",
                range: 10.0..=24.0,
                step: 1.0,
                unit: " px",
            }
            .show(ui, k, &mut s.font_size);
            WrapPanel {
                id: "live",
                width: s.width as f32,
                font_size: s.font_size as f32,
                text: PARAGRAPH,
            }
            .show(ui, k);
        });

    Section::new("Overflow")
        .apis(&[api!(type TextWrap)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Wrapping breaks only between words: a word wider than its 40 px slot \
                 spills out whole rather than breaking.",
            );
            WrapPanel {
                id: "overflow-inner",
                width: 40.0,
                font_size: M.text.body,
                text: "supercalifragilistic",
            }
            .show(ui, k);
        });
}

/// `text` at `font_size` wrapping in a well `width` wide.
#[derive(Debug)]
struct WrapPanel {
    id: &'static str,
    width: f32,
    font_size: f32,
    text: &'static str,
}

impl WrapPanel {
    fn show(self, ui: &mut Ui, k: Kit) {
        Panel::vstack()
            .id_salt(self.id)
            .size((Sizing::fixed(self.width), Sizing::HUG))
            .padding(M.space.sm)
            .chrome(k.t.card())
            .show(ui, |ui| {
                Text::new(self.text)
                    .style(&k.t.body().with_font_size(self.font_size))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
            });
    }
}

fn compositions(ui: &mut Ui, k: Kit) {
    Section::new("Two Hug columns")
        .apis(&[api!(Track::HUG)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
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
                        .style(&k.t.body())
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .grid_cell((0, 0))
                        .show(ui);
                    Text::new("right column")
                        .style(&k.t.body())
                        .grid_cell((0, 1))
                        .show(ui);
                });
        });

    Section::new("Property grid")
        .apis(&[api!(Track::FILL)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A Hug label column and a Fill value column that wraps.",
            );
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
                            .style(&k.t.body())
                            .grid_cell((r, 0))
                            .show(ui);
                        Text::new(value)
                            .id_salt((i, 1))
                            .style(&k.t.body())
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .grid_cell((r, 1))
                            .show(ui);
                    }
                });
        });

    Section::new("Chat").show(ui, k, |ui| {
        support::note(
            ui,
            k,
            "A Fixed avatar and a Fill message. Resize the window and the messages wrap \
                 again.",
        );
        Panel::vstack()
            .size((Sizing::FILL, Sizing::HUG))
            .gap(8.0)
            .show(ui, |ui| {
                ChatRow {
                    key: "alice-1",
                    avatar: k.swatch.teal,
                    message: "Hey! Did you finish reading src/layout/intrinsic.md last night?",
                }
                .show(ui, k);
                ChatRow {
                    key: "bob-1",
                    avatar: k.swatch.orange,
                    message: "Yeah — it clicked once I saw the property grid above actually wrap. \
                         Resize the window and this column flows again, live.",
                }
                .show(ui, k);
                ChatRow {
                    key: "alice-2",
                    avatar: k.swatch.teal,
                    message: "Right? layout is fun.",
                }
                .show(ui, k);
            });
    });
}

/// One chat row: avatar (Fixed circle) + Fill wrapping message.
#[derive(Debug)]
struct ChatRow {
    key: &'static str,
    avatar: RgbaF32,
    message: &'static str,
}

impl ChatRow {
    fn show(self, ui: &mut Ui, k: Kit) {
        let Self {
            key,
            avatar,
            message,
        } = self;
        Panel::hstack()
            .id_salt(("chat-row", key))
            .size((Sizing::FILL, Sizing::HUG))
            .gap(10.0)
            .show(ui, |ui| {
                Block::new()
                    .id_salt(("avatar", key))
                    .size((Sizing::fixed(36.0), Sizing::fixed(36.0)))
                    .chrome(Chrome::rounded(avatar, Corners::all(18.0)))
                    .show(ui);
                Text::new(message)
                    .id_salt(("message", key))
                    .style(&k.t.body())
                    .size((Sizing::FILL, Sizing::HUG))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
            });
    }
}
