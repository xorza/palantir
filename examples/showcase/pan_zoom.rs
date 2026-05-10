use palantir::{Background, Color, Configure, Corners, Panel, Scroll, Sizing, Text, TextStyle, Ui};

/// `Scroll::both().with_zoom()` over a dense grid. Bare wheel pans;
/// `Ctrl/Cmd + wheel` zooms about the cursor; pinch zooms
/// unconditionally. Pin the cursor to a cell and scroll-zoom — the
/// cell stays under the cursor.
pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new(
                "Pan + zoom — wheel pans, Ctrl/Cmd + wheel zooms about the cursor, \
                 pinch zooms on touchpad. The point under the cursor stays fixed.",
            )
            .auto_id()
            .style(TextStyle::default().with_font_size(13.0))
            .show(ui);

            Scroll::both()
                .auto_id()
                .with_zoom()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::vstack().id_salt("pz-grid").gap(4.0).show(ui, |ui| {
                        for r in 0..24u32 {
                            Panel::hstack()
                                .id_salt(("pz-row", r))
                                .gap(4.0)
                                .show(ui, |ui| {
                                    for c in 0..24u32 {
                                        cell(ui, r, c);
                                    }
                                });
                        }
                    });
                });
        });
}

fn cell(ui: &mut Ui, r: u32, c: u32) {
    Panel::hstack()
        .id_salt(("pz-cell", r, c))
        .size((Sizing::Fixed(56.0), Sizing::Fixed(40.0)))
        .padding((6.0, 4.0))
        .background(Background {
            fill: cell_color(r, c),
            radius: Corners::all(3.0),
            ..Default::default()
        })
        .show(ui, |ui| {
            Text::new(cell_label(r, c))
                .id_salt(("pz-cell-label", r, c))
                .style(
                    TextStyle::default()
                        .with_font_size(11.0)
                        .with_color(Color::hex(0x1a1a1a)),
                )
                .show(ui);
        });
}

fn cell_color(r: u32, c: u32) -> Color {
    let tr = r as f32 / 24.0;
    let tc = c as f32 / 24.0;
    Color::rgb(
        0.30 + 0.55 * tc,
        0.55 - 0.25 * (tr - 0.5).abs(),
        0.85 - 0.55 * tr,
    )
}

fn cell_label(r: u32, c: u32) -> &'static str {
    const LABELS: [&str; 24 * 24] = build_labels();
    LABELS[(r * 24 + c) as usize]
}

const fn build_labels() -> [&'static str; 24 * 24] {
    // The label set was generated to keep the showcase readable; rather
    // than ship a 600-line literal, we settle for a compact "grid"
    // marker per cell. The visual interest is the colored cells, not
    // the text.
    [".."; 24 * 24]
}
