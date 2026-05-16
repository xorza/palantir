use crate::showcase::swatch;
use palantir::{Color, Configure, Frame, Grid, Panel, Sizing, Track, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // 1. Classic three-column app shell:
            //    fixed sidebar | flexible content | hugging right rail.
            //    Row 0 spans all three as a header band.
            cell(ui, "shell", |ui| {
                Grid::new()
                    .id_salt("shell-grid")
                    .cols([Track::fixed(140.0), Track::fill(), Track::hug()])
                    .rows([Track::fixed(36.0), Track::fill()])
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        grid_tile(ui, "title", (0, 0), Some((1, 3)), None, swatch::B);
                        grid_tile(ui, "nav", (1, 0), None, None, swatch::C);
                        grid_tile(ui, "content", (1, 1), None, None, swatch::A);
                        grid_tile(
                            ui,
                            "rail",
                            (1, 2),
                            None,
                            Some((Sizing::Fixed(80.0), Sizing::FILL)),
                            swatch::D,
                        );
                    });
            });

            // 2. Clamped sidebar + greedy content. The left Fill is bounded
            //    `[200, 300]` so it grows with the window only within that
            //    range; the right Fill has no clamp and absorbs every leftover
            //    pixel. Resize the window to watch the sidebar saturate.
            cell(ui, "min/max", |ui| {
                Grid::new()
                    .id_salt("clamped")
                    .cols([
                        Track::fill_weight(1.0).min(200.0).max(300.0),
                        Track::fill_weight(2.0),
                    ])
                    .rows([Track::fill()])
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        grid_tile(ui, "c1", (0, 0), None, None, swatch::A);
                        grid_tile(ui, "c2", (0, 1), None, None, swatch::D);
                    });
            });
        });
}

fn cell(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(12.0)
        .gap(8.0)
        .show(ui, body);
}

fn grid_tile(
    ui: &mut Ui,
    id: &'static str,
    cell: (u16, u16),
    span: Option<(u16, u16)>,
    size: Option<(Sizing, Sizing)>,
    color: Color,
) {
    let mut f = Frame::new()
        .id_salt(id)
        .grid_cell(cell)
        .background(swatch::swatch_bg(color));
    if let Some(s) = span {
        f = f.grid_span(s);
    }
    if let Some(sz) = size {
        f = f.size(sz);
    }
    f.show(ui);
}
