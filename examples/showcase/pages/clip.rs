//! Clip modes and subtree transforms. Each card holds a child overflowing on all four sides via negative margins; the clip mode decides what survives (none spills, rect cuts square, rounded trims to the corner radius), and padding moves the boundary to the content rect. `TranslateScale` applies to whole subtrees, stroke widths included.

use crate::scaffold::api;
use crate::support;
use crate::support::{DemoCell, Kit, Section, Surface};
use palantir::glam::Vec2;
use palantir::{
    Align, Block, Chrome, Configure, Corners, Panel, Sizing, Stroke, TranslateScale, Ui,
};

const CARD: f32 = 200.0;
/// How far the child overhangs the card on every side; the cell adds it to all four, so "no clip" spills into empty space, not the neighboring tile.
const SPILL: f32 = 18.0;
const CELL: f32 = CARD + 2.0 * SPILL;

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    Section::new("Clip modes")
        .apis(&[api!(Panel::clip_rect), api!(Panel::clip_rounded)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The same child, overflowing the card by 18 px on every side, under each mode.",
            );
            support::tiles(ui, |ui| {
                for (label, mode) in [
                    ("None — the child spills", Mode::None),
                    ("Rect — square cut at the bounds", Mode::Rect),
                    ("Rounded — follows the radius", Mode::Rounded),
                ] {
                    ClipCard {
                        label,
                        mode,
                        padding: 0.0,
                    }
                    .show(ui, k);
                }
            });
        });

    Section::new("Clip and padding").show(ui, k, |ui| {
        support::note(
            ui,
            k,
            "Padding moves the clip boundary in, to the content rect, and the rounded mask \
                 follows the same edge.",
        );
        support::tiles(ui, |ui| {
            for card in [
                ClipCard {
                    label: "padded, no clip",
                    mode: Mode::None,
                    padding: 28.0,
                },
                ClipCard {
                    label: "padded, Rect",
                    mode: Mode::Rect,
                    padding: 28.0,
                },
                ClipCard {
                    label: "padded, Rounded",
                    mode: Mode::Rounded,
                    padding: 14.0,
                },
            ] {
                card.show(ui, k);
            }
        });
    });

    Section::new("Subtree transform")
        .apis(&[api!(Panel::transform), api!(TranslateScale::from_scale)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A TranslateScale on a container moves and scales everything under it, stroke \
                 widths included. Nested transforms compose.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("translate (30, 24)").show(ui, k, |ui| {
                    Panel::zstack()
                        .transform(TranslateScale::from_translation(Vec2::new(30.0, 24.0)))
                        .show(ui, |ui| tile(ui, k));
                });
                DemoCell::new("scale 1.5 — strokes scale too").show(ui, k, |ui| {
                    Panel::zstack()
                        .transform(TranslateScale::from_scale(1.5))
                        .show(ui, |ui| tile(ui, k));
                });
                DemoCell::new("composed — scale 1.25, then translate").show(ui, k, |ui| {
                    Panel::zstack()
                        .transform(TranslateScale::from_scale(1.25))
                        .show(ui, |ui| {
                            Panel::zstack()
                                .transform(TranslateScale::from_translation(Vec2::new(20.0, 10.0)))
                                .show(ui, |ui| tile(ui, k));
                        });
                });
            });
        });
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    None,
    Rect,
    Rounded,
}

/// Card with a large corner radius so the rect scissor and rounded stencil differ clearly.
fn card_chrome(k: Kit) -> Chrome {
    Chrome::rounded(k.t.ground, Corners::all(28.0)).with_border(Stroke::new(k.t.border, 1.5))
}

/// A card under one clip mode, its child overflowing on every side.
#[derive(Debug)]
struct ClipCard {
    label: &'static str,
    mode: Mode,
    padding: f32,
}

impl ClipCard {
    fn show(self, ui: &mut Ui, k: Kit) {
        let Self {
            label,
            mode,
            padding,
        } = self;
        DemoCell::new(label)
            .size(CELL, CELL)
            .surface(Surface::Bare)
            .show(ui, k, |ui| {
                Panel::zstack()
                    .size((Sizing::FILL, Sizing::FILL))
                    .child_align(Align::CENTER)
                    .show(ui, |ui| {
                        let mut panel = Panel::zstack()
                            .size((Sizing::fixed(CARD), Sizing::fixed(CARD)))
                            .padding(padding)
                            .chrome(card_chrome(k));
                        panel = match mode {
                            Mode::None => panel,
                            Mode::Rect => panel.clip_rect(),
                            Mode::Rounded => panel.clip_rounded(),
                        };
                        panel.show(ui, |ui| spiller(ui, k));
                    });
            });
    }
}

/// Rectangle overflowing the card on all sides: the negative margin grows its slot past the content rect and `Fill` takes it all, so the overhang stays [`SPILL`] padded or not.
fn spiller(ui: &mut Ui, k: Kit) {
    Block::new()
        .size((Sizing::FILL, Sizing::FILL))
        .margin((-SPILL, -SPILL, -SPILL, -SPILL))
        // Translucent so the card edge stays visible beneath; unclipped would otherwise be a solid block.
        .chrome(Chrome::fill(k.swatch.orange.with_alpha(0.8)))
        .show(ui);
}

#[track_caller]
fn tile(ui: &mut Ui, k: Kit) {
    Block::new()
        .auto_id()
        .size((Sizing::fixed(56.0), Sizing::fixed(56.0)))
        .chrome(support::swatch_chrome(k.swatch.teal))
        .show(ui);
}
