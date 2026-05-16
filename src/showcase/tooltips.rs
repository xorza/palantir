//! Tooltip showcase. Hover any of the buttons below for ~0.5 s — the
//! bubble appears in `Layer::Tooltip` above all other content. Move
//! between adjacent buttons quickly to see the warmup window (no
//! re-delay within ~1 s of the previous bubble).

use palantir::{Button, Configure, Panel, Sizing, Tooltip, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            row(ui, "tt-delays", |ui| {
                let r = Button::new().id_salt("d-default").label("default").show(ui);
                Tooltip::for_(&r)
                    .text("Default 0.5 s delay before this appears.")
                    .show(ui);

                let r = Button::new().id_salt("d-instant").label("instant").show(ui);
                Tooltip::for_(&r)
                    .text("No delay — fires the frame the pointer arrives.")
                    .delay(0.0)
                    .show(ui);

                let r = Button::new()
                    .id_salt("d-slow")
                    .label("slow (1.5 s)")
                    .show(ui);
                Tooltip::for_(&r)
                    .text("Held for 1.5 s before showing.")
                    .delay(1.5)
                    .show(ui);
            });

            row(ui, "tt-wrapping", |ui| {
                let r = Button::new().id_salt("w-1").label("long text").show(ui);
                Tooltip::for_(&r)
                    .text(
                        "Tooltips wrap to the configured max width — the default \
                         is 280 logical pixels. Long bodies stack into multiple \
                         lines automatically; the bubble's height hugs the \
                         shaped text.",
                    )
                    .show(ui);

                let r = Button::new().id_salt("w-2").label("narrow").show(ui);
                Tooltip::for_(&r)
                    .text("Override max width to force tighter wrap on a single tooltip.")
                    .max_size((140.0, f32::INFINITY))
                    .show(ui);
            });

            row(ui, "tt-disabled", |ui| {
                let r = Button::new()
                    .id_salt("dis-1")
                    .label("disabled (no tooltip)")
                    .disabled(true)
                    .show(ui);
                Tooltip::for_(&r)
                    .text("This text is suppressed by the default skip-on-disabled rule.")
                    .show(ui);

                let r = Button::new()
                    .id_salt("dis-2")
                    .label("disabled (with tooltip)")
                    .disabled(true)
                    .show(ui);
                Tooltip::for_(&r)
                    .text("Opt-in via .show_when_disabled(true) for 'why is this disabled' hints.")
                    .show_when_disabled(true)
                    .show(ui);
            });

            row(ui, "tt-warmup", |ui| {
                for i in 0..5 {
                    let r = Button::new()
                        .id_salt(("warm", i))
                        .label(format!("item {}", i + 1))
                        .show(ui);
                    Tooltip::for_(&r)
                        .text(match i {
                            0 => "Hover, then move to the next item within ~1 s.",
                            1 => "See how the next bubble appears instantly?",
                            2 => "Warmup window keeps scanning a row snappy.",
                            3 => "Pause for ~1 s and the next one re-delays.",
                            _ => "Last one.",
                        })
                        .show(ui);
                }
            });
        });
}

fn row(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(8.0)
        .show(ui, body);
}
