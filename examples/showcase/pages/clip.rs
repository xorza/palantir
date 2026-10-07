//! Clip modes and subtree transforms. Each card holds a child overflowing on all four sides via negative margins; the clip mode decides what survives (none spills, rect cuts square, rounded trims to the corner radius), and padding moves the boundary to the content rect. `TranslateScale` applies to whole subtrees, stroke widths included.

use crate::support;
use crate::support::{api, captioned_cell, demo_cell, note, section, tiles};
use palantir::{
    Align, Background, Block, Configure, Corners, Panel, RgbaF32, Sizing, Stroke, TranslateScale,
    Ui, Vec2,
};

const CARD: f32 = 200.0;
/// How far the child overhangs the card on every side; the cell adds it to all four, so "no clip" spills into empty space, not the neighbouring tile.
const SPILL: f32 = 18.0;
const CELL: f32 = CARD + 2.0 * SPILL;

pub(crate) fn build(ui: &mut Ui) {
    section(
        ui,
        "Clip modes",
        &[api!(Panel::clip_rect), api!(Panel::clip_rounded)],
        |ui| {
            note(
                ui,
                "The same child, overflowing the card by 18 px on every side, under each mode.",
            );
            tiles(ui, |ui| {
                clip_card(ui, "None — the child spills", Mode::None, 0.0);
                clip_card(ui, "Rect — square cut at the bounds", Mode::Rect, 0.0);
                clip_card(ui, "Rounded — follows the radius", Mode::Rounded, 0.0);
            });
        },
    );

    section(ui, "Clip and padding", &[], |ui| {
        note(
            ui,
            "Padding moves the clip boundary in, to the content rect, and the rounded mask \
                 follows the same edge.",
        );
        tiles(ui, |ui| {
            clip_card(ui, "padded, no clip", Mode::None, 28.0);
            clip_card(ui, "padded, Rect", Mode::Rect, 28.0);
            clip_card(ui, "padded, Rounded", Mode::Rounded, 14.0);
        });
    });

    section(
        ui,
        "Subtree transform",
        &[api!(Panel::transform), api!(TranslateScale::from_scale)],
        |ui| {
            note(
                ui,
                "A TranslateScale on a container moves and scales everything under it, stroke \
                 widths included. Nested transforms compose.",
            );
            tiles(ui, |ui| {
                demo_cell(ui, "translate (30, 24)", |ui| {
                    Panel::zstack()
                        .transform(TranslateScale::from_translation(Vec2::new(30.0, 24.0)))
                        .show(ui, |ui| tile(ui));
                });
                demo_cell(ui, "scale 1.5 — strokes scale too", |ui| {
                    Panel::zstack()
                        .transform(TranslateScale::from_scale(1.5))
                        .show(ui, |ui| tile(ui));
                });
                demo_cell(ui, "composed — scale 1.25, then translate", |ui| {
                    Panel::zstack()
                        .transform(TranslateScale::from_scale(1.25))
                        .show(ui, |ui| {
                            Panel::zstack()
                                .transform(TranslateScale::from_translation(Vec2::new(20.0, 10.0)))
                                .show(ui, |ui| tile(ui));
                        });
                });
            });
        },
    );
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    None,
    Rect,
    Rounded,
}

/// Card with a large corner radius so the rect scissor and rounded stencil differ clearly.
fn card_bg() -> Background {
    Background::rounded(support::WELL, Corners::all(28.0))
        .with_border(Stroke::new(RgbaF32::hex(0x4d5663), 1.5))
}

#[track_caller]
fn clip_card(ui: &mut Ui, label: &'static str, mode: Mode, padding: f32) {
    captioned_cell(ui, label, CELL, CELL, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .child_align(Align::CENTER)
            .show(ui, |ui| {
                let mut panel = Panel::zstack()
                    .size((Sizing::fixed(CARD), Sizing::fixed(CARD)))
                    .padding(padding)
                    .background(card_bg());
                panel = match mode {
                    Mode::None => panel,
                    Mode::Rect => panel.clip_rect(),
                    Mode::Rounded => panel.clip_rounded(),
                };
                panel.show(ui, spiller);
            });
    });
}

/// Rectangle overflowing the card on all sides: the negative margin grows its slot past the content rect and `Fill` takes it all, so the overhang stays [`SPILL`] padded or not.
fn spiller(ui: &mut Ui) {
    Block::new()
        .size((Sizing::FILL, Sizing::FILL))
        .margin((-SPILL, -SPILL, -SPILL, -SPILL))
        // Translucent so the card edge stays visible beneath; unclipped would otherwise be a solid block.
        .background(Background::fill(support::B.with_alpha(0.8)))
        .show(ui);
}

#[track_caller]
fn tile(ui: &mut Ui) {
    Block::new()
        .auto_id()
        .size((Sizing::fixed(56.0), Sizing::fixed(56.0)))
        .background(support::swatch_bg(support::A))
        .show(ui);
}
