use palantir::{Configure, Panel, RadioButton, Sizing, Text, Ui, WidgetId};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Flavor {
    #[default]
    Vanilla,
    Chocolate,
    Strawberry,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

#[derive(Default)]
struct State {
    flavor: Flavor,
    size: Size,
}

pub fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::radio::state");
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("Radio buttons").id_salt(("rb", "title")).show(ui);

            // Group 1: flavor
            let mut flavor = ui.state_mut::<State>(state_id).flavor;
            Panel::vstack()
                .id_salt(("rb", "flavor"))
                .gap(4.0)
                .show(ui, |ui| {
                    Text::new("Flavor").id_salt(("rb", "flavor-h")).show(ui);
                    for (value, label) in [
                        (Flavor::Vanilla, "Vanilla"),
                        (Flavor::Chocolate, "Chocolate"),
                        (Flavor::Strawberry, "Strawberry"),
                    ] {
                        RadioButton::new(&mut flavor, value)
                            .id_salt(("rb", "flavor", label))
                            .label(label)
                            .show(ui);
                    }
                });
            ui.state_mut::<State>(state_id).flavor = flavor;

            // Group 2: size (separate group, separate &mut)
            let mut size = ui.state_mut::<State>(state_id).size;
            Panel::vstack()
                .id_salt(("rb", "size"))
                .gap(4.0)
                .show(ui, |ui| {
                    Text::new("Size").id_salt(("rb", "size-h")).show(ui);
                    for (value, label) in [
                        (Size::Small, "Small"),
                        (Size::Medium, "Medium"),
                        (Size::Large, "Large"),
                    ] {
                        RadioButton::new(&mut size, value)
                            .id_salt(("rb", "size", label))
                            .label(label)
                            .show(ui);
                    }
                });
            ui.state_mut::<State>(state_id).size = size;

            let summary = ui.fmt(format_args!("flavor={flavor:?}  size={size:?}"));
            Text::new(summary).id_salt(("rb", "summary")).show(ui);
        });
}
