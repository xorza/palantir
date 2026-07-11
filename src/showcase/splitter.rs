use aperture::{Background, Color, Configure, Panel, Sizing, Splitter, Text, Ui, WidgetId};

struct State {
    h: f32,
    v: f32,
}

impl Default for State {
    fn default() -> Self {
        Self { h: 0.45, v: 0.6 }
    }
}

pub fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::splitter::state");
    let s = ui.state_mut::<State>(state_id);
    let mut h = s.h;
    let mut v = s.v;

    Panel::vstack()
        .auto_id()
        .gap(8.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("Splitter — drag the bars, double-click to recenter")
                .id_salt(("sp", "title"))
                .show(ui);
            Splitter::horizontal(&mut h)
                .id_salt(("sp", "h"))
                .min_pane(80.0)
                .show(
                    ui,
                    |ui| {
                        Splitter::vertical(&mut v).id_salt(("sp", "v")).show(
                            ui,
                            |ui| pane(ui, "top-left", Color::hex(0x2b3440)),
                            |ui| pane(ui, "bottom-left", Color::hex(0x34404e)),
                        );
                    },
                    |ui| pane(ui, "right", Color::hex(0x3d3346)),
                );
            let readout = ui.fmt(format_args!("h = {h:.2}   v = {v:.2}"));
            Text::new(readout).id_salt(("sp", "readout")).show(ui);
        });

    let s = ui.state_mut::<State>(state_id);
    s.h = h;
    s.v = v;
}

fn pane(ui: &mut Ui, label: &'static str, fill: Color) {
    Panel::zstack()
        .id_salt(("sp", label))
        .size((Sizing::FILL, Sizing::FILL))
        .padding(10.0)
        .background(Background::fill(fill))
        .show(ui, |ui| {
            Text::new(label).id_salt(("sp", label, "t")).show(ui);
        });
}
