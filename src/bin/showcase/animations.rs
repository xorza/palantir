//! Animations showcase. Click "go" to retarget every bar to the
//! opposite width; each row uses a different `AnimSpec` so the easing
//! curves can be compared side-by-side. Doubles as the regression
//! fixture for `Ui::animate` end-to-end (target → tick → record →
//! repaint loop).

use crate::support;
use aperture::{AnimSpec, Button, Configure, Easing, Frame, Panel, Sizing, Text, Ui, WidgetId};

#[derive(Default)]
struct Demo {
    wide: bool,
}

pub(crate) fn build(ui: &mut Ui) {
    let demo_id = WidgetId::from_hash("anim-demo");
    let mut clicked = false;

    support::page(ui, |ui| {
        support::header(
            ui,
            "Click 'go' to retarget every bar's width. Each row uses a \
             different AnimSpec. Hover any control to see the button-fade \
             driven by the same primitive.",
        );

        if Button::new()
            .id_salt("anim-go")
            .label("go")
            .show(ui)
            .left
            .clicked()
        {
            clicked = true;
        }

        let wide = ui.state_mut::<Demo>(demo_id).wide;
        let target = if wide { 400.0 } else { 80.0 };

        bar(
            ui,
            "linear-200",
            "linear 200ms",
            AnimSpec::Duration {
                secs: 0.2,
                ease: Easing::Linear,
            },
            target,
        );
        bar(
            ui,
            "out-cubic-200",
            "out-cubic 200ms",
            AnimSpec::Duration {
                secs: 0.2,
                ease: Easing::OutCubic,
            },
            target,
        );
        bar(
            ui,
            "out-back-300",
            "out-back 300ms (overshoots)",
            AnimSpec::Duration {
                secs: 0.3,
                ease: Easing::OutBack,
            },
            target,
        );
        bar(ui, "spring-soft", "soft spring", AnimSpec::SPRING, target);
    });

    if clicked {
        let s = ui.state_mut::<Demo>(demo_id);
        s.wide = !s.wide;
    }
}

fn bar(ui: &mut Ui, key: &'static str, label: &'static str, spec: AnimSpec, target_width: f32) {
    let id = WidgetId::from_hash(("anim-bar", key));
    let width = ui.animate(id, "width", target_width, Some(spec));
    Panel::hstack()
        .id_salt(("anim-row", key))
        .gap(8.0)
        .show(ui, |ui| {
            Frame::new()
                .id(id)
                .size((Sizing::Fixed(width), Sizing::Fixed(20.0)))
                .background(support::swatch_bg(support::A))
                .show(ui);
            Text::new(label).auto_id().show(ui);
        });
}
