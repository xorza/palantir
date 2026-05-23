//! Minimal chrome demo: a red field with a rounded rect centered in
//! it — blue fill, 4 px green stroke, 30 px corners — and a black
//! rect nested inside, inset by the stroke width so its corners sit
//! concentric with the inner edge of the green border.

use palantir::{Align, Background, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, Ui};

const STROKE: f32 = 8.0;
const OUTER_CORNERS: f32 = 40.0;

pub fn build(ui: &mut Ui) {
    Panel::zstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .child_align(Align::CENTER)
        .background(Background {
            fill: Color::hex(0xff0000).into(),
            ..Default::default()
        })
        .show(ui, |ui| {
            // Blue rect with the green border. `padding = STROKE` insets
            // the child off the border ring so the black rect tucks
            // inside it rather than overlapping the stroke.
            Panel::zstack()
                .auto_id()
                .size((Sizing::Fixed(240.0), Sizing::Fixed(160.0)))
                .padding(0)
                .child_align(Align::CENTER)
                .background(Background {
                    fill: Color::hex(0x0000ff).into(),
                    stroke: Stroke::solid(Color::hex(0x00ff00), STROKE),
                    corners: Corners::all(OUTER_CORNERS),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    Frame::new()
                        .auto_id()
                        .size((Sizing::FILL, Sizing::FILL))
                        // Concentric: shrink the radius by the inset so the
                        // black corners follow the border's inner contour.
                        .background(Background {
                            fill: Color::hex(0x000000).into(),
                            corners: Corners::all(OUTER_CORNERS - STROKE - 1.0),
                            ..Default::default()
                        })
                        .show(ui);
                });
        });
}
