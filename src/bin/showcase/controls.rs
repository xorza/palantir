//! Form controls in one composition. The left card is a settings form
//! wiring switches, checkboxes, radios, a slider, a DragValue, and
//! themed buttons together: "Airplane mode" cascade-disables the
//! network group (the panel's `disabled` flows to every descendant),
//! and Apply drives a fake sync through `Ui::animate` (ProgressBar +
//! Spinner). The right column demos ButtonTheme styling, label
//! eliding, spinner sizing, and echoes the live form state.

use crate::support;
use crate::support::{caption_style, row, section};
use aperture::{
    AnimSpec, Background, Button, ButtonTheme, Checkbox, Color, Configure, Corners, DragValue,
    Panel, ProgressBar, RadioButton, Separator, Shadow, Sizing, Slider, Spinner, StatefulLook,
    Stroke, Switch, Text, TextStyle, TextWrap, Tooltip, Ui, WidgetId, WidgetLook,
};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug)]
struct State {
    airplane: bool,
    wifi: bool,
    bluetooth: bool,
    metered: bool,
    theme: Theme,
    reduce_motion: bool,
    volume: f32,
    fps: i64,
    syncing: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            airplane: false,
            wifi: true,
            bluetooth: false,
            metered: false,
            theme: Theme::System,
            reduce_motion: false,
            volume: 0.6,
            fps: 120,
            syncing: false,
        }
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::controls::state");
    let mut s = std::mem::take(ui.state_mut::<State>(state_id));

    support::page(ui, |ui| {
        support::header(
            ui,
            "Form controls working together — flip 'Airplane mode' to cascade-disable \
             the network group; Apply runs a fake sync through Ui::animate.",
        );
        Panel::hstack()
            .auto_id()
            .gap(24.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                form(ui, &mut s);
                side(ui, &s);
            });
    });

    *ui.state_mut::<State>(state_id) = s;
}

fn form(ui: &mut Ui, s: &mut State) {
    Panel::vstack()
        .id_salt("form")
        .size((Sizing::fixed(340.0), Sizing::HUG))
        .padding(16.0)
        .gap(10.0)
        .background(support::panel_bg())
        .show(ui, |ui| {
            group_caption(ui, "network");
            Switch::new(&mut s.airplane)
                .id_salt("airplane")
                .label("Airplane mode")
                .show(ui);
            Panel::vstack()
                .id_salt("net-group")
                .size((Sizing::FILL, Sizing::HUG))
                .gap(10.0)
                .disabled(s.airplane)
                .show(ui, |ui| {
                    Switch::new(&mut s.wifi)
                        .id_salt("wifi")
                        .label("Wi-Fi")
                        .show(ui);
                    Switch::new(&mut s.bluetooth)
                        .id_salt("bt")
                        .label("Bluetooth")
                        .show(ui);
                    Checkbox::new(&mut s.metered)
                        .id_salt("metered")
                        .label("Treat as metered")
                        .show(ui);
                });

            Separator::horizontal().id_salt("sep-1").show(ui);
            group_caption(ui, "appearance");
            Panel::hstack()
                .id_salt("theme-row")
                .gap(12.0)
                .show(ui, |ui| {
                    for (value, label) in [
                        (Theme::System, "System"),
                        (Theme::Light, "Light"),
                        (Theme::Dark, "Dark"),
                    ] {
                        RadioButton::new(&mut s.theme, value)
                            .id_salt(("theme", label))
                            .label(label)
                            .show(ui);
                    }
                });
            Checkbox::new(&mut s.reduce_motion)
                .id_salt("motion")
                .label("Reduce motion")
                .show(ui);

            // Thick tinted variant of Separator, in situ.
            Separator::horizontal()
                .id_salt("sep-2")
                .thickness(3.0)
                .color(support::A)
                .show(ui);
            group_caption(ui, "audio & video");
            Slider::new(&mut s.volume, 0.0..=1.0)
                .id_salt("volume")
                .show(ui);
            let vol = ui.fmt(format_args!("volume {:.0}%", s.volume * 100.0));
            Text::new(vol)
                .id_salt("volume-pct")
                .style(caption_style())
                .show(ui);
            Panel::hstack().id_salt("fps-row").gap(8.0).show(ui, |ui| {
                DragValue::new(&mut s.fps)
                    .editable(true)
                    .speed(0.25)
                    .range(24.0..=240.0)
                    .decimals(0)
                    .suffix(" fps")
                    .size((Sizing::fixed(110.0), Sizing::HUG))
                    .id_salt("fps")
                    .show(ui);
                Text::new("drag to scrub, click to type")
                    .id_salt("fps-cap")
                    .style(caption_style())
                    .show(ui);
            });

            Separator::horizontal().id_salt("sep-3").show(ui);
            Panel::hstack().id_salt("actions").gap(8.0).show(ui, |ui| {
                if Button::new()
                    .id_salt("apply")
                    .label("Apply")
                    .show(ui)
                    .left
                    .clicked()
                {
                    s.syncing = true;
                }
                if Button::new()
                    .id_salt("reset")
                    .style(outlined_style())
                    .label("Reset")
                    .show(ui)
                    .left
                    .clicked()
                {
                    *s = State::default();
                }
                let del = Button::new()
                    .id_salt("delete")
                    .style(danger_style())
                    .label("Delete profile")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&del)
                    .text("Deletes the profile. No undo — hence the danger theme.")
                    .show(ui);
            });

            let target = if s.syncing { 1.0 } else { 0.0 };
            let frac = ui.animate(
                WidgetId::from_hash("showcase::controls::sync"),
                "frac",
                target,
                Some(AnimSpec::SPRING),
            );
            if s.syncing && frac > 0.995 {
                s.syncing = false;
            }
            ProgressBar::new(frac).id_salt("sync-bar").show(ui);
            if s.syncing {
                Panel::hstack().id_salt("sync-row").gap(8.0).show(ui, |ui| {
                    Spinner::new().size(16.0).id_salt("sync-spin").show(ui);
                    let pct = ui.fmt(format_args!("syncing {:.0}%", frac * 100.0));
                    Text::new(pct)
                        .id_salt("sync-pct")
                        .style(caption_style())
                        .show(ui);
                });
            }
        });
}

fn side(ui: &mut Ui, s: &State) {
    Panel::vstack()
        .id_salt("side")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(16.0)
        .show(ui, |ui| {
            section(
                ui,
                "styles",
                "button styles — default / outlined / danger ButtonThemes, hover + press + disabled states",
                |ui| {
                    row(ui, "b-default", |ui| {
                        Button::new().id_salt("d-1").label("normal").show(ui);
                        Button::new()
                            .id_salt("d-2")
                            .label("disabled")
                            .disabled(true)
                            .show(ui);
                        Button::new()
                            .id_salt("o-1")
                            .style(outlined_style())
                            .label("outlined")
                            .show(ui);
                        Button::new()
                            .id_salt("o-2")
                            .style(outlined_style())
                            .label("disabled")
                            .disabled(true)
                            .show(ui);
                        Button::new()
                            .id_salt("c-1")
                            .style(danger_style())
                            .label("danger")
                            .show(ui);
                    });
                },
            );

            // Single-line labels are hard-cut to the box width by default: a
            // fixed-width button whose label is longer than its box is
            // truncated instead of spilling outside the chrome.
            // `.text_wrap(SingleLine)` opts out — the label runs past the box
            // on one line. A `Hug`-width button commits its natural width.
            section(
                ui,
                "elide",
                "label overflow — hard cut (default) / SingleLine opt-out / Hug",
                |ui| {
                    row(ui, "b-elide", |ui| {
                        Button::new()
                            .id_salt("e-1")
                            .size((Sizing::fixed(140.0), Sizing::HUG))
                            .label("Screenshot 2026-05-28 at 01.21.25.png")
                            .show(ui);
                        Button::new()
                            .id_salt("e-2")
                            .size((Sizing::fixed(140.0), Sizing::HUG))
                            .text_wrap(TextWrap::SingleLine)
                            .label("Screenshot 2026-05-28 at 01.21.25.png")
                            .show(ui);
                        Button::new().id_salt("e-3").label("fits its content").show(ui);
                    });
                },
            );

            section(
                ui,
                "spinners",
                "Spinner — indeterminate, three sizes + custom color",
                |ui| {
                    Panel::hstack().id_salt("spin-row").gap(20.0).show(ui, |ui| {
                        Spinner::new().size(20.0).id_salt("spin-a").show(ui);
                        Spinner::new().size(32.0).id_salt("spin-b").show(ui);
                        Spinner::new()
                            .size(48.0)
                            .color(Color::hex(0xff8866))
                            .id_salt("spin-c")
                            .show(ui);
                    });
                },
            );

            section(ui, "state", "live form state", |ui| {
                let net = ui.fmt(format_args!(
                    "airplane={}  wifi={}  bluetooth={}  metered={}",
                    s.airplane, s.wifi, s.bluetooth, s.metered
                ));
                Text::new(net).id_salt("st-net").style(caption_style()).show(ui);
                let app = ui.fmt(format_args!(
                    "theme={:?}  reduce_motion={}  volume={:.2}  fps={}",
                    s.theme, s.reduce_motion, s.volume, s.fps
                ));
                Text::new(app).id_salt("st-app").style(caption_style()).show(ui);
            });
        });
}

fn group_caption(ui: &mut Ui, label: &'static str) {
    Text::new(label)
        .id_salt(("group", label))
        .style(caption_style())
        .show(ui);
}

fn outlined_style() -> ButtonTheme {
    // Stroke uses the palette's `border_focused` so the outlined
    // variant reads as "selectable surface" matching the rest of the
    // theme.
    let accent = Color::hex(0x4cd3ff);
    let stroke = Stroke::solid(accent, 1.5);
    let bg = |fill: Color, stroke| Background {
        fill: fill.into(),
        stroke,
        corners: Corners::all(4.0),
        shadow: Shadow::NONE,
    };
    ButtonTheme {
        looks: StatefulLook {
            normal: WidgetLook {
                background: Some(bg(Color::TRANSPARENT, stroke)),
                text: None,
            },
            hovered: WidgetLook {
                background: Some(bg(accent.with_alpha(0.18), stroke)),
                text: None,
            },
            active: WidgetLook {
                background: Some(bg(accent.with_alpha(0.35), stroke)),
                text: None,
            },
            disabled: WidgetLook {
                background: Some(bg(
                    Color::TRANSPARENT,
                    Stroke::solid(accent.with_alpha(0.35), 1.5),
                )),
                text: Some(TextStyle::default().with_color(Color::hex(0x878a8d))),
            },
        },
        ..Default::default()
    }
}

fn danger_style() -> ButtonTheme {
    // Palette `error = #ff5e44`.
    let red = Color::hex(0xff5e44);
    let bg = |fill: Color| Background {
        fill: fill.into(),
        stroke: Stroke::ZERO,
        corners: Corners::all(2.0),
        shadow: Shadow::NONE,
    };
    ButtonTheme {
        looks: StatefulLook {
            normal: WidgetLook {
                background: Some(bg(red)),
                text: Some(TextStyle::default().with_color(Color::WHITE)),
            },
            hovered: WidgetLook {
                background: Some(bg(Color::hex(0xff7e6a))),
                text: Some(TextStyle::default().with_color(Color::WHITE)),
            },
            active: WidgetLook {
                background: Some(bg(Color::hex(0xc74734))),
                text: Some(TextStyle::default().with_color(Color::WHITE)),
            },
            disabled: WidgetLook {
                background: Some(bg(red.with_alpha(0.4))),
                text: Some(
                    TextStyle::default().with_color(Color::linear_rgba(1.0, 1.0, 1.0, 0.55)),
                ),
            },
        },
        ..Default::default()
    }
}
