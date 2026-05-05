use palantir::{Color, Configure, Panel, Scroll, Sizing, Stroke, Styled, Text, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .gap(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new(
                "Scroll (vertical) — hover the card and pan with the wheel / two-finger scroll.",
            )
            .size_px(13.0)
            .color(Color::rgb(0.78, 0.82, 0.92))
            .show(ui);

            Panel::vstack()
                .with_id("scroll-card")
                .size((Sizing::FILL, Sizing::FILL))
                .padding(8.0)
                .fill(Color::rgb(0.16, 0.20, 0.28))
                .stroke(Stroke {
                    width: 1.5,
                    color: Color::rgb(0.30, 0.36, 0.46),
                })
                .radius(8.0)
                .show(ui, |ui| {
                    Scroll::vertical()
                        .with_id("rows-scroll")
                        .size((Sizing::FILL, Sizing::FILL))
                        .gap(4.0)
                        .show(ui, |ui| {
                            for i in 0..40 {
                                row(ui, i);
                            }
                        });
                });
        });
}

fn row(ui: &mut Ui, i: u32) {
    // HSV-ish hue sweep so the rows are visually distinct as they pan.
    let t = i as f32 / 40.0;
    let r = 0.30 + 0.50 * t;
    let g = 0.55 - 0.20 * (t - 0.5).abs();
    let b = 0.85 - 0.55 * t;

    Panel::hstack()
        .with_id(("scroll-row", i))
        .size((Sizing::FILL, Sizing::Fixed(28.0)))
        .padding((10.0, 6.0))
        .fill(Color::rgb(r, g, b))
        .radius(4.0)
        .show(ui, |ui| {
            Text::new(label_for(i))
                .with_id(("scroll-row-label", i))
                .size_px(13.0)
                .color(Color::rgb(0.10, 0.10, 0.14))
                .show(ui);
        });
}

fn label_for(i: u32) -> &'static str {
    const LABELS: [&str; 40] = [
        "row 00", "row 01", "row 02", "row 03", "row 04", "row 05", "row 06", "row 07", "row 08",
        "row 09", "row 10", "row 11", "row 12", "row 13", "row 14", "row 15", "row 16", "row 17",
        "row 18", "row 19", "row 20", "row 21", "row 22", "row 23", "row 24", "row 25", "row 26",
        "row 27", "row 28", "row 29", "row 30", "row 31", "row 32", "row 33", "row 34", "row 35",
        "row 36", "row 37", "row 38", "row 39",
    ];
    LABELS[i as usize]
}
