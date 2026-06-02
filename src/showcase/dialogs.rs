use palantir::{Button, ComboBox, Configure, Modal, Panel, Separator, Sizing, Text, Ui, WidgetId};

#[derive(Default)]
struct State {
    fruit: usize,
    modal_open: bool,
}

pub fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::dialogs::state");
    let options = ["Apple", "Banana", "Cherry", "Durian", "Elderberry"];

    Panel::vstack()
        .auto_id()
        .gap(12.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("ComboBox — click to open the dropdown")
                .id_salt(("dlg", "ct"))
                .show(ui);

            let mut fruit = ui.state_mut::<State>(state_id).fruit;
            ComboBox::new(&mut fruit, &options)
                .size((Sizing::Fixed(180.0), Sizing::Hug))
                .id_salt(("dlg", "combo"))
                .show(ui);
            ui.state_mut::<State>(state_id).fruit = fruit;
            let chosen = ui.fmt(format_args!("selected: {}", options[fruit]));
            Text::new(chosen).id_salt(("dlg", "chosen")).show(ui);

            Separator::horizontal().id_salt(("dlg", "sep")).show(ui);

            Text::new("Modal — dims the background, Esc or backdrop closes")
                .id_salt(("dlg", "mt"))
                .show(ui);
            if Button::new()
                .id_salt(("dlg", "open"))
                .label("Open dialog")
                .show(ui)
                .clicked()
            {
                ui.state_mut::<State>(state_id).modal_open = true;
            }
        });

    if ui.state_mut::<State>(state_id).modal_open {
        let resp = Modal::new().id_salt(("dlg", "modal")).show(ui, |ui| {
            Panel::vstack()
                .id_salt(("dlg", "mbody"))
                .gap(16.0)
                .show(ui, |ui| {
                    Text::new("Delete all the things?")
                        .id_salt(("dlg", "mtitle"))
                        .show(ui);
                    Panel::hstack()
                        .id_salt(("dlg", "mrow"))
                        .gap(8.0)
                        .show(ui, |ui| {
                            if Button::new()
                                .id_salt(("dlg", "cancel"))
                                .label("Cancel")
                                .show(ui)
                                .clicked()
                            {
                                ui.state_mut::<State>(state_id).modal_open = false;
                            }
                            if Button::new()
                                .id_salt(("dlg", "ok"))
                                .label("Delete")
                                .show(ui)
                                .clicked()
                            {
                                ui.state_mut::<State>(state_id).modal_open = false;
                            }
                        });
                });
        });
        if resp.dismissed {
            ui.state_mut::<State>(state_id).modal_open = false;
        }
    }
}
