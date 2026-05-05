use palantir::{Color, Configure, Panel, Scroll, Sizing, Stroke, Styled, Text, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .gap(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new(
                "Scroll — hover a card and pan with the wheel / two-finger scroll. \
                 Cards are vertical · horizontal · two-axis.",
            )
            .size_px(13.0)
            .color(Color::rgb(0.78, 0.82, 0.92))
            .show(ui);

            Panel::hstack()
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    card(ui, "v-card", "vertical", |ui| {
                        Scroll::vertical()
                            .with_id("rows-scroll")
                            .size((Sizing::FILL, Sizing::FILL))
                            .gap(4.0)
                            .show(ui, |ui| {
                                for i in 0..40 {
                                    row(ui, "v", i);
                                }
                            });
                    });

                    card(ui, "h-card", "horizontal", |ui| {
                        Scroll::horizontal()
                            .with_id("cols-scroll")
                            .size((Sizing::FILL, Sizing::FILL))
                            .gap(4.0)
                            .show(ui, |ui| {
                                for i in 0..40 {
                                    col(ui, i);
                                }
                            });
                    });

                    card(ui, "xy-card", "two-axis", |ui| {
                        Scroll::both()
                            .with_id("grid-scroll")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                grid(ui);
                            });
                    });
                });
        });
}

fn card(ui: &mut Ui, key: &'static str, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .with_id(key)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .gap(6.0)
        .fill(Color::rgb(0.16, 0.20, 0.28))
        .stroke(Stroke {
            width: 1.5,
            color: Color::rgb(0.30, 0.36, 0.46),
        })
        .radius(8.0)
        .show(ui, |ui| {
            Text::new(label)
                .with_id((key, "title"))
                .size_px(12.0)
                .color(Color::rgb(0.78, 0.82, 0.92))
                .show(ui);
            body(ui);
        });
}

fn row(ui: &mut Ui, ns: &'static str, i: u32) {
    let (r, g, b) = swatch(i);
    Panel::hstack()
        .with_id((ns, "scroll-row", i))
        .size((Sizing::FILL, Sizing::Fixed(28.0)))
        .padding((10.0, 6.0))
        .fill(Color::rgb(r, g, b))
        .radius(4.0)
        .show(ui, |ui| {
            Text::new(label_for(i))
                .with_id((ns, "scroll-row-label", i))
                .size_px(13.0)
                .color(Color::rgb(0.10, 0.10, 0.14))
                .show(ui);
        });
}

fn col(ui: &mut Ui, i: u32) {
    let (r, g, b) = swatch(i);
    Panel::vstack()
        .with_id(("h", "scroll-col", i))
        .size((Sizing::Fixed(60.0), Sizing::FILL))
        .padding((6.0, 10.0))
        .fill(Color::rgb(r, g, b))
        .radius(4.0)
        .show(ui, |ui| {
            Text::new(label_for(i))
                .with_id(("h", "scroll-col-label", i))
                .size_px(13.0)
                .color(Color::rgb(0.10, 0.10, 0.14))
                .show(ui);
        });
}

fn grid(ui: &mut Ui) {
    // Single Hug-sized child holding a 12×16 colored grid via nested
    // VStack/HStack. Both-axes Scroll measures with INF on both axes,
    // so the inner stacks size to natural content and overflow the
    // viewport on both sides.
    Panel::vstack().with_id("xy-grid").gap(4.0).show(ui, |ui| {
        for r in 0..16u32 {
            Panel::hstack()
                .with_id(("xy-row", r))
                .gap(4.0)
                .show(ui, |ui| {
                    for c in 0..12u32 {
                        let (rr, gg, bb) = swatch(r * 12 + c);
                        Panel::hstack()
                            .with_id(("xy-cell", r, c))
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                            .padding((6.0, 4.0))
                            .fill(Color::rgb(rr, gg, bb))
                            .radius(4.0)
                            .show(ui, |ui| {
                                Text::new(cell_label(r, c))
                                    .with_id(("xy-cell-label", r, c))
                                    .size_px(11.0)
                                    .color(Color::rgb(0.10, 0.10, 0.14))
                                    .show(ui);
                            });
                    }
                });
        }
    });
}

fn swatch(i: u32) -> (f32, f32, f32) {
    let t = (i % 40) as f32 / 40.0;
    (
        0.30 + 0.50 * t,
        0.55 - 0.20 * (t - 0.5).abs(),
        0.85 - 0.55 * t,
    )
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

fn cell_label(r: u32, c: u32) -> &'static str {
    const LABELS: [&str; 192] = [
        "0,0", "0,1", "0,2", "0,3", "0,4", "0,5", "0,6", "0,7", "0,8", "0,9", "0,10", "0,11",
        "1,0", "1,1", "1,2", "1,3", "1,4", "1,5", "1,6", "1,7", "1,8", "1,9", "1,10", "1,11",
        "2,0", "2,1", "2,2", "2,3", "2,4", "2,5", "2,6", "2,7", "2,8", "2,9", "2,10", "2,11",
        "3,0", "3,1", "3,2", "3,3", "3,4", "3,5", "3,6", "3,7", "3,8", "3,9", "3,10", "3,11",
        "4,0", "4,1", "4,2", "4,3", "4,4", "4,5", "4,6", "4,7", "4,8", "4,9", "4,10", "4,11",
        "5,0", "5,1", "5,2", "5,3", "5,4", "5,5", "5,6", "5,7", "5,8", "5,9", "5,10", "5,11",
        "6,0", "6,1", "6,2", "6,3", "6,4", "6,5", "6,6", "6,7", "6,8", "6,9", "6,10", "6,11",
        "7,0", "7,1", "7,2", "7,3", "7,4", "7,5", "7,6", "7,7", "7,8", "7,9", "7,10", "7,11",
        "8,0", "8,1", "8,2", "8,3", "8,4", "8,5", "8,6", "8,7", "8,8", "8,9", "8,10", "8,11",
        "9,0", "9,1", "9,2", "9,3", "9,4", "9,5", "9,6", "9,7", "9,8", "9,9", "9,10", "9,11",
        "10,0", "10,1", "10,2", "10,3", "10,4", "10,5", "10,6", "10,7", "10,8", "10,9", "10,10",
        "10,11", "11,0", "11,1", "11,2", "11,3", "11,4", "11,5", "11,6", "11,7", "11,8", "11,9",
        "11,10", "11,11", "12,0", "12,1", "12,2", "12,3", "12,4", "12,5", "12,6", "12,7", "12,8",
        "12,9", "12,10", "12,11", "13,0", "13,1", "13,2", "13,3", "13,4", "13,5", "13,6", "13,7",
        "13,8", "13,9", "13,10", "13,11", "14,0", "14,1", "14,2", "14,3", "14,4", "14,5", "14,6",
        "14,7", "14,8", "14,9", "14,10", "14,11", "15,0", "15,1", "15,2", "15,3", "15,4", "15,5",
        "15,6", "15,7", "15,8", "15,9", "15,10", "15,11",
    ];
    LABELS[(r * 12 + c) as usize]
}
