use crate::swatch;
use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, Ui};

/// Card with a rounded background. Used in three configurations below
/// (no clip, scissor clip, rounded stencil clip) to demonstrate how
/// each interacts with overflowing children.
fn card() -> Background {
    Background {
        fill: Color::hex(0x252525),
        stroke: Stroke {
            width: 1.5,
            color: Color::hex(0x4d5663),
        },
        radius: Corners::all(28.0),
    }
}

pub fn build(ui: &mut Ui) {
    Panel::hstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // No clip: child spills past the rounded corners.
            Panel::zstack()
                .id_salt("no_clip")
                .size((Sizing::FILL, Sizing::FILL))
                .background(card())
                .show(ui, |ui| spiller(ui, ("spill", "no_clip")));

            // Scissor clip: child cut at the rect bounding box, square
            // corners visible where the rounded paint thins out.
            Panel::zstack()
                .id_salt("scissor")
                .size((Sizing::FILL, Sizing::FILL))
                .background(card())
                .clip_rect()
                .show(ui, |ui| spiller(ui, ("spill", "scissor")));

            // Rounded stencil clip: child trimmed to the painted
            // corner radius.
            Panel::zstack()
                .id_salt("rounded")
                .size((Sizing::FILL, Sizing::FILL))
                .background(card())
                .clip_rounded()
                .show(ui, |ui| spiller(ui, ("spill", "rounded")));
        });
}

/// Wide rectangle that overflows the card on three sides via negative
/// margins. The card behavior decides what survives:
/// no clip → spills everywhere; scissor → square cut at the rect;
/// rounded → respects the corner radius.
fn spiller(ui: &mut Ui, id: impl std::hash::Hash) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Fixed(360.0), Sizing::Fixed(420.0)))
        .margin((-60.0, -60.0, -60.0, -60.0))
        .background(Background {
            fill: swatch::B,
            radius: Corners::all(0.0),
            ..Default::default()
        })
        .show(ui);
}
