use palantir::{Color, Element, Frame, Grid, Sizing, Stroke, Track, Ui, VStack};

fn body() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}
fn header() -> Color {
    Color::rgb(0.85, 0.45, 0.30)
}
fn sidebar() -> Color {
    Color::rgb(0.45, 0.80, 0.55)
}
fn rail() -> Color {
    Color::rgb(0.55, 0.45, 0.80)
}

pub fn build(ui: &mut Ui) {
    VStack::new()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // 1. Classic three-column app shell:
            //    fixed sidebar | flexible content | hugging right rail.
            //    Row 0 spans all three as a header band.
            cell(ui, "shell", |ui| {
                Grid::with_id("shell-grid")
                    .cols([Track::fixed(140.0), Track::fill(), Track::hug()])
                    .rows([Track::fixed(36.0), Track::fill()])
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Frame::with_id("title")
                            .grid_cell((0, 0))
                            .grid_span((1, 3))
                            .fill(header())
                            .radius(4.0)
                            .show(ui);
                        Frame::with_id("nav")
                            .grid_cell((1, 0))
                            .fill(sidebar())
                            .radius(4.0)
                            .show(ui);
                        Frame::with_id("content")
                            .grid_cell((1, 1))
                            .fill(body())
                            .radius(4.0)
                            .show(ui);
                        // Right rail Hug → only as wide as its child.
                        Frame::with_id("rail")
                            .grid_cell((1, 2))
                            .size((Sizing::Fixed(80.0), Sizing::FILL))
                            .fill(rail())
                            .radius(4.0)
                            .show(ui);
                    });
            });

            // 2. Clamped sidebar + greedy content. The left Fill is bounded
            //    `[200, 300]` so it grows with the window only within that
            //    range; the right Fill has no clamp and absorbs every leftover
            //    pixel. Resize the window to watch the sidebar saturate.
            cell(ui, "min/max", |ui| {
                Grid::with_id("clamped")
                    .cols([
                        Track::fill_weight(1.0).min(200.0).max(300.0),
                        Track::fill_weight(2.0),
                    ])
                    .rows([Track::fill()])
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Frame::with_id("c1")
                            .grid_cell((0, 0))
                            .fill(body())
                            .radius(4.0)
                            .show(ui);
                        Frame::with_id("c2")
                            .grid_cell((0, 1))
                            .fill(rail())
                            .radius(4.0)
                            .show(ui);
                    });
            });
        });
}

fn cell(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    VStack::with_id(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(12.0)
        .gap(8.0)
        .fill(Color::rgb(0.16, 0.18, 0.24))
        .stroke(Stroke {
            width: 1.0,
            color: Color::rgb(0.30, 0.36, 0.46),
        })
        .radius(6.0)
        .show(ui, body);
}
