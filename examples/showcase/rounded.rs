use crate::swatch;
use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, Surface, Ui};

/// Card with a rounded background. Used in three configurations below
/// (no clip, scissor clip, rounded stencil clip) to demonstrate how
/// each interacts with overflowing children.
fn card() -> Background {
    Background {
        fill: Color::hex(0x252525),
        stroke: Stroke {
            width: 1.5,
            color: Color::hex(0x4d5663),
        }),
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
            labeled_card(ui, "no_clip", card().into(), "no clip");

            // Scissor clip: child cut at the rect bounding box, square
            // corners visible where the rounded paint thins out.
            labeled_card(ui, "scissor", Surface::clip_rect_with_bg(card()), "scissor");

            // Rounded stencil clip: child trimmed to the painted
            // corner radius.
            labeled_card(
                ui,
                "rounded",
                Surface::clip_rounded_with_bg(card()),
                "rounded",
            );
        });
}

fn labeled_card(ui: &mut Ui, id: &'static str, surface: Surface, _label: &str) {
    Panel::zstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .background(surface)
        .show(ui, |ui| {
            spiller(ui, ("spill", id));
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
