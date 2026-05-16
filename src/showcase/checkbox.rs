use super::app_state::AppState;
use palantir::{Checkbox, Configure, Panel, Sizing, Text, Ui, WidgetId};

#[derive(Default)]
struct State {
    accept: bool,
    notify: bool,
    coffee: bool,
    disabled_on: bool,
}

pub fn build(ui: &mut Ui<AppState>) {
    let state_id = WidgetId::from_hash("showcase::checkbox::state");
    Panel::vstack()
        .auto_id()
        .gap(12.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("Checkbox").id_salt(("cb", "title")).show(ui);

            let s = ui.state_mut::<State>(state_id);
            let mut accept = s.accept;
            let mut notify = s.notify;
            let mut coffee = s.coffee;
            let mut disabled_on = s.disabled_on;

            Checkbox::new(&mut accept)
                .id_salt(("cb", "accept"))
                .label("I accept the terms")
                .show(ui);
            Checkbox::new(&mut notify)
                .id_salt(("cb", "notify"))
                .label("Email me updates")
                .show(ui);
            Checkbox::new(&mut coffee)
                .id_salt(("cb", "coffee"))
                .label("Coffee, not tea")
                .show(ui);
            Checkbox::new(&mut disabled_on)
                .id_salt(("cb", "disabled"))
                .label("disabled — click does nothing")
                .disabled(true)
                .show(ui);

            let s = ui.state_mut::<State>(state_id);
            s.accept = accept;
            s.notify = notify;
            s.coffee = coffee;
            s.disabled_on = disabled_on;

            let summary = ui.fmt(format_args!(
                "accept={accept}  notify={notify}  coffee={coffee}",
            ));
            Text::new(summary).id_salt(("cb", "summary")).show(ui);
        });
}
