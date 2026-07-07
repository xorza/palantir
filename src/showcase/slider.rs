use aperture::{Configure, DragValue, Panel, Separator, Sizing, Slider, Text, Ui, WidgetId};

struct State {
    volume: f32,
    zoom: f32,
    angle: f64,
    count: i64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            volume: 0.6,
            zoom: 50.0,
            angle: 45.0,
            count: 8,
        }
    }
}

pub fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::slider::state");
    Panel::vstack()
        .auto_id()
        .gap(12.0)
        .padding(16.0)
        .size((Sizing::Fixed(340.0), Sizing::FILL))
        .show(ui, |ui| {
            Text::new("Slider — drag or click the rail")
                .id_salt(("sl", "t1"))
                .show(ui);

            let s = ui.state_mut::<State>(state_id);
            let mut volume = s.volume;
            let mut zoom = s.zoom;
            let mut angle = s.angle;
            let mut count = s.count;

            Slider::new(&mut volume, 0.0..=1.0)
                .id_salt(("sl", "vol"))
                .show(ui);
            let vt = ui.fmt(format_args!("volume = {volume:.2}"));
            Text::new(vt).id_salt(("sl", "volt")).show(ui);

            Slider::new(&mut zoom, 0.0..=100.0)
                .step(5.0)
                .id_salt(("sl", "zoom"))
                .show(ui);
            let zt = ui.fmt(format_args!("zoom = {zoom:.0}%  (snaps to 5)"));
            Text::new(zt).id_salt(("sl", "zoomt")).show(ui);

            Separator::horizontal().id_salt(("sl", "sep")).show(ui);
            Text::new("DragValue — drag to scrub, click to type")
                .id_salt(("sl", "t2"))
                .show(ui);
            Panel::hstack()
                .id_salt(("sl", "drow"))
                .gap(12.0)
                .show(ui, |ui| {
                    DragValue::new(&mut angle)
                        .editable(true)
                        .speed(0.5)
                        .decimals(1)
                        .suffix("°")
                        .size((Sizing::Fixed(96.0), Sizing::Hug))
                        .id_salt(("dv", "angle"))
                        .show(ui);
                    DragValue::new(&mut count)
                        .editable(true)
                        .speed(0.25)
                        .range(0.0..=100.0)
                        .decimals(0)
                        .size((Sizing::Fixed(96.0), Sizing::Hug))
                        .id_salt(("dv", "count"))
                        .show(ui);
                });

            let s = ui.state_mut::<State>(state_id);
            s.volume = volume;
            s.zoom = zoom;
            s.angle = angle;
            s.count = count;
        });
}
