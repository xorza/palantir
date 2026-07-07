use aperture::{Configure, Panel, Sizing, Text, ToggleSwitch, Ui, WidgetId};

#[derive(Default)]
struct State {
    wifi: bool,
    bluetooth: bool,
    airplane: bool,
}

pub fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::switch::state");
    Panel::vstack()
        .auto_id()
        .gap(12.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("ToggleSwitch — knob slides + track cross-fades on toggle")
                .id_salt(("sw", "title"))
                .show(ui);

            let s = ui.state_mut::<State>(state_id);
            let mut wifi = s.wifi;
            let mut bluetooth = s.bluetooth;
            let mut airplane = s.airplane;

            ToggleSwitch::new(&mut wifi)
                .id_salt(("sw", "wifi"))
                .label("Wi-Fi")
                .show(ui);
            ToggleSwitch::new(&mut bluetooth)
                .id_salt(("sw", "bt"))
                .label("Bluetooth")
                .show(ui);
            ToggleSwitch::new(&mut airplane)
                .id_salt(("sw", "air"))
                .label("Airplane mode")
                .show(ui);
            // Disabled switch, seeded on: the flip is gated so it holds
            // its value every frame without needing persisted state.
            let mut locked_on = true;
            ToggleSwitch::new(&mut locked_on)
                .id_salt(("sw", "locked"))
                .label("disabled (stays on)")
                .disabled(true)
                .show(ui);

            let s = ui.state_mut::<State>(state_id);
            s.wifi = wifi;
            s.bluetooth = bluetooth;
            s.airplane = airplane;

            let summary = ui.fmt(format_args!(
                "wifi={wifi}  bluetooth={bluetooth}  airplane={airplane}",
            ));
            Text::new(summary).id_salt(("sw", "summary")).show(ui);
        });
}
