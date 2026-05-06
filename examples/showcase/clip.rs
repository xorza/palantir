use crate::swatch;
use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, Surface, Ui};

/// Visible panel boundary needed for this demo: the whole point is to
/// see whether a child rect spills past or gets clipped at the panel
/// edge — without a stroke, the boundary is invisible.
fn bounded_panel() -> Background {
    Background {
        fill: Color::hex(0x252525),
        stroke: Some(Stroke {
            width: 1.5,
            color: Color::hex(0x363636),
        }),
        radius: Corners::all(8.0),
    }
}

pub fn build(ui: &mut Ui) {
    Panel::hstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Left: clipped — child rect spills via negative margin, but the
            // scissor on the panel cuts it at the panel border.
            Panel::zstack()
                .with_id("clipped")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Surface::clipped(bounded_panel()))
                .show(ui, |ui| {
                    spiller(ui, "spilled-clipped");
                });

            // Right: same content, no clip — the spilling rect leaks past the panel.
            Panel::zstack()
                .with_id("unclipped")
                .size((Sizing::FILL, Sizing::FILL))
                .background(bounded_panel())
                .show(ui, |ui| {
                    spiller(ui, "spilled-unclipped");
                });
        });
}

fn spiller(ui: &mut Ui, id: &'static str) {
    Frame::new()
        .with_id(id)
        .size((Sizing::Fixed(220.0), Sizing::Fixed(80.0)))
        .margin((-40.0, -30.0, 0.0, 0.0))
        .background(Background {
            fill: swatch::B,
            radius: Corners::all(6.0),
            ..Default::default()
        })
        .show(ui);
}
