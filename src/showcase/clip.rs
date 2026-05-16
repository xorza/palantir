use super::app_state::AppState;
use crate::showcase::swatch;
use palantir::{Background, Color, Configure, Corners, Frame, Panel, Shadow, Sizing, Stroke, Ui};

/// Card with a rounded background. Used in three configurations below
/// (no clip, scissor clip, rounded stencil clip) to demonstrate how
/// each clip mode interacts with overflowing children. The radius is
/// large so the difference between rect-scissor and rounded-stencil
/// reads clearly at the corners.
fn card() -> Background {
    Background {
        fill: Color::hex(0x252525).into(),
        stroke: Stroke::solid(Color::hex(0x4d5663), 1.5),
        radius: Corners::all(28.0),
        shadow: Shadow::NONE,
    }
}

pub fn build(ui: &mut Ui<AppState>) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Row 1: no padding on the clipping panel.
            Panel::hstack()
                .id_salt("no-padding")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // No clip: child spills past both the rect bounds
                    // and the rounded corners.
                    Panel::zstack()
                        .id_salt("none")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(card())
                        .show(ui, |ui| spiller(ui, ("spill", "none")));

                    // Scissor clip: child cut at the rect bounding
                    // box; square corners visible where the rounded
                    // paint thins out.
                    Panel::zstack()
                        .id_salt("rect")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(card())
                        .clip_rect()
                        .show(ui, |ui| spiller(ui, ("spill", "rect")));

                    // Rounded stencil clip: child trimmed to the
                    // painted corner radius.
                    Panel::zstack()
                        .id_salt("rounded")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(card())
                        .clip_rounded()
                        .show(ui, |ui| spiller(ui, ("spill", "rounded")));
                });

            // Row 2: same clip modes, but the panel has padding.
            // Children clip at the content rect (deflated by padding)
            // — the spiller's negative margin is measured from inside
            // the padding, and the clip mask follows the same edge.
            Panel::hstack()
                .id_salt("padded")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id_salt("none-pad")
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(28.0)
                        .background(card())
                        .show(ui, |ui| spiller(ui, ("spill", "none-pad")));

                    Panel::zstack()
                        .id_salt("rect-pad")
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(28.0)
                        .background(card())
                        .clip_rect()
                        .show(ui, |ui| spiller(ui, ("spill", "rect-pad")));

                    Panel::zstack()
                        .id_salt("rounded-pad")
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(14.0)
                        .background(card())
                        .clip_rounded()
                        .show(ui, |ui| spiller(ui, ("spill", "rounded-pad")));
                });
        });
}

/// Rectangle that overflows the card on all four sides via negative
/// margins. The card's clip mode decides what survives:
/// no clip → spills everywhere; rect → square cut at the rect;
/// rounded → respects the corner radius. Sized small enough that the
/// "no clip" overflow doesn't punch out of the showcase area into
/// the toolbar above.
fn spiller<T>(ui: &mut Ui<T>, id: impl std::hash::Hash) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Fixed(240.0), Sizing::Fixed(280.0)))
        .margin((-24.0, -24.0, -24.0, -24.0))
        .background(Background {
            fill: swatch::B.into(),
            radius: Corners::all(0.0),
            ..Default::default()
        })
        .show(ui);
}
