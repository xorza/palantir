//! Clip modes and subtree transforms. Each clip card holds a child that
//! overflows on all four sides via negative margins; the card's clip
//! mode decides what survives — no clip spills, rect clip cuts square at
//! the bounds, rounded clip trims to the painted corner radius. Adding
//! padding moves the boundary: children clip at the content rect and the
//! mask follows the same edge.
//!
//! `TranslateScale` applies to whole subtrees — descendants paint
//! translated and scaled, stroke widths included.

use crate::support;
use crate::support::{captioned_cell, demo_cell, section, tiles};
use glam::Vec2;
use palantir::{
    Align, Background, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, TranslateScale, Ui,
};
use std::hash::Hash;

const CARD: f32 = 200.0;
/// How far the child overhangs the card on every side. The cell is the
/// card plus this on all four sides, so "no clip" spills into empty
/// space instead of over the neighbouring tile.
const SPILL: f32 = 18.0;
const CELL: f32 = CARD + 2.0 * SPILL;

pub(crate) fn build(ui: &mut Ui) {
    section(
        ui,
        "clip modes",
        "clip modes — the same overflowing child under each mode",
        |ui| {
            tiles(ui, "clip-tiles", |ui| {
                clip_card(ui, "None — the child spills", Mode::None, 0.0);
                clip_card(ui, "Rect — square cut at the bounds", Mode::Rect, 0.0);
                clip_card(ui, "Rounded — follows the radius", Mode::Rounded, 0.0);
            });
        },
    );

    section(
        ui,
        "clip & padding",
        "clip & padding — padding moves the boundary inward to the content rect",
        |ui| {
            tiles(ui, "padded-tiles", |ui| {
                clip_card(ui, "padded, no clip", Mode::None, 28.0);
                clip_card(ui, "padded, Rect", Mode::Rect, 28.0);
                clip_card(ui, "padded, Rounded", Mode::Rounded, 14.0);
            });
        },
    );

    section(
        ui,
        "subtree transform",
        "subtree transform — TranslateScale on a container moves everything \
         beneath it",
        |ui| {
            tiles(ui, "transform-tiles", |ui| {
                demo_cell(ui, "translate (30, 24)", |ui| {
                    Panel::zstack()
                        .id_salt("t-outer")
                        .transform(TranslateScale::from_translation(Vec2::new(30.0, 24.0)))
                        .show(ui, |ui| tile(ui, "t-tile"));
                });
                demo_cell(ui, "scale 1.5 — strokes scale too", |ui| {
                    Panel::zstack()
                        .id_salt("s-outer")
                        .transform(TranslateScale::from_scale(1.5))
                        .show(ui, |ui| tile(ui, "s-tile"));
                });
                demo_cell(ui, "composed — scale 1.25, then translate", |ui| {
                    Panel::zstack()
                        .id_salt("c-outer")
                        .transform(TranslateScale::from_scale(1.25))
                        .show(ui, |ui| {
                            Panel::zstack()
                                .id_salt("c-inner")
                                .transform(TranslateScale::from_translation(Vec2::new(20.0, 10.0)))
                                .show(ui, |ui| tile(ui, "c-tile"));
                        });
                });
            });
        },
    );
}

enum Mode {
    None,
    Rect,
    Rounded,
}

/// Card with a large corner radius, so the difference between the
/// rect scissor and the rounded stencil reads clearly at the corners.
fn card_bg() -> Background {
    Background::rounded(support::WELL, Corners::all(28.0))
        .with_stroke(Stroke::solid(Color::hex(0x4d5663), 1.5))
}

fn clip_card(ui: &mut Ui, label: &'static str, mode: Mode, padding: f32) {
    captioned_cell(ui, label, CELL, CELL, |ui| {
        Panel::zstack()
            .id_salt((label, "bleed"))
            .size((Sizing::FILL, Sizing::FILL))
            .child_align(Align::CENTER)
            .show(ui, |ui| {
                let mut panel = Panel::zstack()
                    .id_salt((label, "card"))
                    .size((Sizing::fixed(CARD), Sizing::fixed(CARD)))
                    .padding(padding)
                    .background(card_bg());
                panel = match mode {
                    Mode::None => panel,
                    Mode::Rect => panel.clip_rect(),
                    Mode::Rounded => panel.clip_rounded(),
                };
                panel.show(ui, |ui| spiller(ui, (label, "spill")));
            });
    });
}

/// Rectangle that overflows the card on all four sides. The negative
/// margin grows its slot past the content rect and `Fill` takes all of
/// it, so the overhang stays exactly [`SPILL`] whether or not the card
/// is padded.
fn spiller(ui: &mut Ui, id: impl Hash) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .margin((-SPILL, -SPILL, -SPILL, -SPILL))
        // Translucent so the card's own edge stays visible underneath —
        // otherwise the unclipped case is a solid block with nothing to
        // read the overhang against.
        .background(Background::fill(support::B.with_alpha(0.8)))
        .show(ui);
}

fn tile(ui: &mut Ui, id: &'static str) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::fixed(56.0), Sizing::fixed(56.0)))
        .background(support::swatch_bg(support::A))
        .show(ui);
}
