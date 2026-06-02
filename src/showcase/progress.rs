//! Separator + ProgressBar showcase. Static bars pin the fill geometry
//! at representative fractions; the animated row drives a bar through
//! `Ui::animate` so the fill tracks a spring tween end-to-end.

use palantir::{
    AnimSpec, Button, Color, Configure, Panel, ProgressBar, Separator, Sizing, Spinner, Text, Ui,
    WidgetId,
};

#[derive(Default)]
struct State {
    full: bool,
}

pub fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::progress::state");
    let mut clicked = false;

    Panel::vstack()
        .auto_id()
        .gap(10.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("Separator").id_salt(("pg", "sep-title")).show(ui);
            Separator::horizontal().id_salt(("pg", "sep-1")).show(ui);
            Text::new(
                "A horizontal rule sits above this line; a thicker, \
                       tinted one sits below.",
            )
            .id_salt(("pg", "sep-body"))
            .show(ui);
            Separator::horizontal()
                .id_salt(("pg", "sep-2"))
                .thickness(3.0)
                .color(palantir::Color::hex(0x9adbfb))
                .show(ui);

            Text::new("ProgressBar — static")
                .id_salt(("pg", "static"))
                .show(ui);
            for (i, frac) in [0.0_f32, 0.35, 0.7, 1.0].into_iter().enumerate() {
                let label = ui.fmt(format_args!("{:.0}%", frac * 100.0));
                Text::new(label).id_salt(("pg", "lbl", i)).show(ui);
                ProgressBar::new(frac).id_salt(("pg", "bar", i)).show(ui);
            }

            Text::new("ProgressBar — animated")
                .id_salt(("pg", "anim"))
                .show(ui);
            if Button::new()
                .id_salt(("pg", "go"))
                .label("toggle")
                .show(ui)
                .clicked()
            {
                clicked = true;
            }
            let full = ui.state_mut::<State>(state_id).full;
            let target = if full { 1.0 } else { 0.0 };
            let frac = ui.animate(
                WidgetId::from_hash("showcase::progress::anim"),
                "frac",
                target,
                Some(AnimSpec::SPRING),
            );
            ProgressBar::new(frac).id_salt(("pg", "anim-bar")).show(ui);
            let pct = ui.fmt(format_args!("{:.0}%", frac * 100.0));
            Text::new(pct).id_salt(("pg", "anim-pct")).show(ui);

            Separator::horizontal().id_salt(("pg", "sep-3")).show(ui);
            Text::new("Spinner — indeterminate (spins continuously)")
                .id_salt(("pg", "spin-title"))
                .show(ui);
            Panel::hstack()
                .id_salt(("pg", "spin-row"))
                .gap(20.0)
                .show(ui, |ui| {
                    Spinner::new().size(20.0).id_salt(("pg", "spin-a")).show(ui);
                    Spinner::new().size(32.0).id_salt(("pg", "spin-b")).show(ui);
                    Spinner::new()
                        .size(48.0)
                        .color(Color::hex(0xff8866))
                        .id_salt(("pg", "spin-c"))
                        .show(ui);
                });
        });

    if clicked {
        let s = ui.state_mut::<State>(state_id);
        s.full = !s.full;
    }
}
