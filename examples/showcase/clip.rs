use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, Ui};

pub fn build(ui: &mut Ui) {
    Panel::hstack()
        .gap(16.0)
        .clip(false)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Left: clipped — child rect spills via negative margin, but the
            // scissor on the panel cuts it at the panel border.
            Panel::zstack()
                .with_id("clipped")
                .size((Sizing::FILL, Sizing::FILL))
                .clip(true)
                .background(Background {
                    fill: Color::rgb(0.16, 0.20, 0.28),
                    stroke: Some(Stroke {
                        width: 1.5,
                        color: Color::rgb(0.30, 0.36, 0.46),
                    }),
                    radius: Corners::all(8.0),
                })
                .show(ui, |ui| {
                    spiller(ui, "spilled-clipped");
                });

            // Right: same content, no clip — the spilling rect leaks past the panel.
            Panel::zstack()
                .with_id("unclipped")
                .size((Sizing::FILL, Sizing::FILL))
                .clip(false)
                .background(Background {
                    fill: Color::rgb(0.16, 0.20, 0.28),
                    stroke: Some(Stroke {
                        width: 1.5,
                        color: Color::rgb(0.30, 0.36, 0.46),
                    }),
                    radius: Corners::all(8.0),
                })
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
            fill: Color::rgb(0.85, 0.45, 0.30),
            radius: Corners::all(6.0),
            ..Default::default()
        })
        .show(ui);
}
