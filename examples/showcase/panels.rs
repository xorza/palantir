use palantir::{Canvas, Color, Element, Frame, HStack, Sizing, Stroke, Styled, Ui, VStack, ZStack};

fn tile() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}

pub fn build(ui: &mut Ui) {
    HStack::new()
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            cell(ui, "HStack", |ui| {
                HStack::new().gap(6.0).show(ui, |ui| {
                    swatch(ui, "h-a", 40.0, 40.0, tile());
                    swatch(ui, "h-b", 40.0, 40.0, tile());
                    swatch(ui, "h-c", 40.0, 40.0, tile());
                });
            });
            cell(ui, "VStack", |ui| {
                VStack::new().gap(6.0).show(ui, |ui| {
                    swatch(ui, "v-a", 60.0, 24.0, tile());
                    swatch(ui, "v-b", 60.0, 24.0, tile());
                    swatch(ui, "v-c", 60.0, 24.0, tile());
                });
            });
            cell(ui, "ZStack", |ui| {
                ZStack::new().show(ui, |ui| {
                    swatch(ui, "z-back", 80.0, 80.0, Color::rgb(0.25, 0.30, 0.50));
                    swatch(ui, "z-front", 50.0, 50.0, Color::rgb(0.85, 0.45, 0.30));
                });
            });
            cell(ui, "Canvas", |ui| {
                Canvas::new()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Frame::with_id("p1")
                            .position((10.0, 10.0))
                            .size(40.0)
                            .fill(tile())
                            .show(ui);
                        Frame::with_id("p2")
                            .position((60.0, 30.0))
                            .size(40.0)
                            .fill(Color::rgb(0.85, 0.45, 0.30))
                            .show(ui);
                        Frame::with_id("p3")
                            .position((30.0, 70.0))
                            .size(40.0)
                            .fill(Color::rgb(0.45, 0.80, 0.55))
                            .show(ui);
                    });
            });
        });
}

fn swatch(ui: &mut Ui, id: &'static str, w: f32, h: f32, c: Color) {
    Frame::with_id(id)
        .size((Sizing::Fixed(w), Sizing::Fixed(h)))
        .fill(c)
        .radius(4.0)
        .show(ui);
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
