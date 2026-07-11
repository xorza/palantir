//! Clip modes and subtree transforms. The clip cards each contain a
//! child that overflows on all four sides via negative margins; the
//! card's clip mode decides what survives — no clip spills, rect clip
//! cuts square at the bounds, rounded clip trims to the painted corner
//! radius. The second row adds padding: children clip at the content
//! rect (deflated by padding), and the mask follows the same edge.
//! Below, `TranslateScale` transforms apply to whole subtrees —
//! descendants paint translated / scaled, stroke widths included.

use crate::showcase::support;
use crate::showcase::support::{captioned_cell, demo_cell, swatch_bg};
use aperture::{
    Background, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, TranslateScale, Ui,
};
use glam::Vec2;
use std::hash::Hash;

/// Card with a rounded background. The radius is large so the
/// difference between rect-scissor and rounded-stencil reads clearly
/// at the corners.
fn card() -> Background {
    Background {
        fill: Color::hex(0x252525).into(),
        stroke: Stroke::solid(Color::hex(0x4d5663), 1.5),
        corners: Corners::all(28.0),
        ..Default::default()
    }
}

pub fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        support::header(
            ui,
            "Clip modes against an overflowing child (top rows), and \
             TranslateScale subtree transforms (bottom).",
        );

        Panel::hstack()
            .id_salt("no-padding")
            .gap(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                clip_card(ui, "no clip — child spills", ClipMode::None, 0.0);
                clip_card(ui, "clip_rect — square cut", ClipMode::Rect, 0.0);
                clip_card(ui, "clip_rounded — follows radius", ClipMode::Rounded, 0.0);
            });

        Panel::hstack()
            .id_salt("padded")
            .gap(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                clip_card(ui, "padded, no clip", ClipMode::None, 28.0);
                clip_card(ui, "padded, clip_rect", ClipMode::Rect, 28.0);
                clip_card(ui, "padded, clip_rounded", ClipMode::Rounded, 14.0);
            });

        Panel::hstack()
            .id_salt("transform-row")
            .gap(12.0)
            .size((Sizing::FILL, Sizing::Fixed(160.0)))
            .show(ui, |ui| {
                demo_cell(ui, "transform — translate (40, 30)", |ui| {
                    Panel::zstack()
                        .auto_id()
                        .transform(TranslateScale::from_translation(Vec2::new(40.0, 30.0)))
                        .show(ui, |ui| tile(ui, "t-tile"));
                });
                demo_cell(ui, "transform — scale 1.5 (strokes too)", |ui| {
                    Panel::zstack()
                        .auto_id()
                        .transform(TranslateScale::from_scale(1.5))
                        .show(ui, |ui| tile(ui, "s-tile"));
                });
                // Outer scale, inner translate — order matters.
                demo_cell(
                    ui,
                    "transform — composed: scale 1.25 then translate",
                    |ui| {
                        Panel::zstack()
                            .id_salt("outer")
                            .transform(TranslateScale::from_scale(1.25))
                            .show(ui, |ui| {
                                Panel::zstack()
                                    .id_salt("inner")
                                    .transform(TranslateScale::from_translation(Vec2::new(
                                        20.0, 10.0,
                                    )))
                                    .show(ui, |ui| tile(ui, "c-tile"));
                            });
                    },
                );
            });
    });
}

enum ClipMode {
    None,
    Rect,
    Rounded,
}

fn clip_card(ui: &mut Ui, label: &'static str, mode: ClipMode, padding: f32) {
    captioned_cell(ui, label, |ui| {
        let mut panel = Panel::zstack()
            .id_salt((label, "card"))
            .size((Sizing::FILL, Sizing::FILL))
            .padding(padding)
            .background(card());
        panel = match mode {
            ClipMode::None => panel,
            ClipMode::Rect => panel.clip_rect(),
            ClipMode::Rounded => panel.clip_rounded(),
        };
        panel.show(ui, |ui| spiller(ui, (label, "spill")));
    });
}

/// Rectangle that overflows the card on all four sides via negative
/// margins. Sized small enough that the "no clip" overflow doesn't
/// punch out of the showcase area into the toolbar above.
fn spiller(ui: &mut Ui, id: impl Hash) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Fixed(240.0), Sizing::Fixed(280.0)))
        .margin((-24.0, -24.0, -24.0, -24.0))
        .background(Background {
            fill: support::B.into(),
            corners: Corners::all(0.0),
            ..Default::default()
        })
        .show(ui);
}

fn tile(ui: &mut Ui, id: &'static str) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Fixed(60.0), Sizing::Fixed(60.0)))
        .background(swatch_bg(support::A))
        .show(ui);
}
