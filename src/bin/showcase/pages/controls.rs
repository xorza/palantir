//! Form controls in one composition. The left card is a settings form
//! wiring switches, checkboxes, radios, a slider, a DragValue, and
//! themed buttons together: "Airplane mode" cascade-disables the network
//! group (the panel's `disabled` flows to every descendant), and Apply
//! drives a fake sync through `Ui::animate` (ProgressBar + Spinner). The
//! right column demos ButtonTheme styling, label eliding, spinner
//! sizing, and echoes the live form state.

use crate::support;
use crate::support::{note_style, row, section};
use palantir::{
    AnimSpec, Background, Button, ButtonTheme, Checkbox, Color, Configure, Corners, DragValue,
    Panel, ProgressBar, RadioButton, Separator, Sizing, Slider, Spinner, StatefulLook, Stroke,
    Switch, Text, TextStyle, TextWrap, Tooltip, Ui, WidgetId, WidgetLook, fmt,
};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Appearance {
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
    appearance: Appearance,
    reduce_motion: bool,
    volume: f64,
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
            appearance: Appearance::System,
            reduce_motion: false,
            volume: 0.6,
            fps: 120,
            syncing: false,
        }
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::controls::state");
    let outlined = outlined_style();
    let danger = danger_style();

    ui.with_state::<State, _>(state_id, |ui, s| {
        Panel::hstack()
            .id_salt("columns")
            .gap(24.0)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                section(
                    ui,
                    "settings form — switches, radios, a slider, a DragValue, and buttons \
                 wired together",
                    |ui| {
                        form(ui, s, &outlined, &danger);
                    },
                );
                support::column(ui, "col-r", |ui| side(ui, s, &outlined, &danger));
            });
    });
}

fn form(ui: &mut Ui, s: &mut State, outlined: &ButtonTheme, danger: &ButtonTheme) {
    Panel::vstack()
        .id_salt("form-card")
        .size((Sizing::fixed(340.0), Sizing::HUG))
        .padding(16.0)
        .gap(10.0)
        .background(support::well_bg())
        .show(ui, |ui| {
            group(ui, "network");
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
            group(ui, "appearance");
            Panel::hstack()
                .id_salt("theme-row")
                .gap(12.0)
                .show(ui, |ui| {
                    for (value, label) in [
                        (Appearance::System, "System"),
                        (Appearance::Light, "Light"),
                        (Appearance::Dark, "Dark"),
                    ] {
                        RadioButton::new(&mut s.appearance, value)
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
            group(ui, "audio & video");
            Slider::new(&mut s.volume, 0.0..=1.0)
                .id_salt("volume")
                .show(ui);
            let vol = fmt!(ui, "volume {:.0}%", s.volume * 100.0);
            Text::new(vol)
                .id_salt("volume-pct")
                .style(&note_style())
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
                    .style(&note_style())
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
                    .style(outlined)
                    .label("Reset")
                    .show(ui)
                    .left
                    .clicked()
                {
                    *s = State::default();
                }
                let del = Button::new()
                    .id_salt("delete")
                    .style(danger)
                    .label("Delete profile")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&del)
                    .label("Deletes the profile. No undo — hence the danger theme.")
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
                    Spinner::new().diameter(16.0).id_salt("sync-spin").show(ui);
                    let pct = fmt!(ui, "syncing {:.0}%", frac * 100.0);
                    Text::new(pct)
                        .id_salt("sync-pct")
                        .style(&note_style())
                        .show(ui);
                });
            }
        });
}

fn side(ui: &mut Ui, s: &State, outlined: &ButtonTheme, danger: &ButtonTheme) {
    section(
        ui,
        "button styles — default · outlined · danger, each with a disabled state",
        |ui| {
            row(ui, |ui| {
                Button::new().id_salt("d-1").label("normal").show(ui);
                Button::new()
                    .id_salt("d-2")
                    .label("disabled")
                    .disabled(true)
                    .show(ui);
                Button::new()
                    .id_salt("o-1")
                    .style(outlined)
                    .label("outlined")
                    .show(ui);
                Button::new()
                    .id_salt("o-2")
                    .style(outlined)
                    .label("disabled")
                    .disabled(true)
                    .show(ui);
                Button::new()
                    .id_salt("c-1")
                    .style(danger)
                    .label("danger")
                    .show(ui);
            });
        },
    );

    // Single-line labels are hard-cut to the box width by default: a
    // fixed-width button whose label is longer than its box is truncated
    // instead of spilling outside the chrome. `.text_wrap(SingleLine)`
    // opts out — the label runs past the box on one line. A `Hug`-width
    // button commits its natural width.
    section(
        ui,
        "label overflow — hard cut (default) · SingleLine opt-out · Hug width",
        |ui| {
            row(ui, |ui| {
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
                Button::new()
                    .id_salt("e-3")
                    .label("fits its content")
                    .show(ui);
            });
        },
    );

    section(
        ui,
        "spinners — indeterminate, three diameters plus a custom colour",
        |ui| {
            Panel::hstack()
                .id_salt("spin-row")
                .gap(20.0)
                .show(ui, |ui| {
                    Spinner::new().diameter(20.0).id_salt("spin-a").show(ui);
                    Spinner::new().diameter(32.0).id_salt("spin-b").show(ui);
                    Spinner::new()
                        .diameter(48.0)
                        .color(support::B)
                        .id_salt("spin-c")
                        .show(ui);
                });
        },
    );

    section(
        ui,
        "live state — what the form above currently holds",
        |ui| {
            let net = fmt!(
                ui,
                "airplane={}  wifi={}  bluetooth={}  metered={}",
                s.airplane,
                s.wifi,
                s.bluetooth,
                s.metered
            );
            Text::new(net)
                .id_salt("st-net")
                .style(&note_style())
                .show(ui);
            let app = fmt!(
                ui,
                "appearance={:?}  reduce_motion={}  volume={:.2}  fps={}",
                s.appearance,
                s.reduce_motion,
                s.volume,
                s.fps
            );
            Text::new(app)
                .id_salt("st-app")
                .style(&note_style())
                .show(ui);
        },
    );
}

fn group(ui: &mut Ui, label: &'static str) {
    Text::new(label)
        .id_salt(("group", label))
        .style(&support::caption_style())
        .show(ui);
}

/// Transparent fill, accent stroke — reads as "selectable surface"
/// against the rest of the theme.
fn outlined_style() -> ButtonTheme {
    let accent = support::ACCENT;
    let stroke = Stroke::solid(accent, 1.5);
    let bg = |fill: Color, stroke| Background::rounded(fill, Corners::all(4.0)).with_stroke(stroke);
    ButtonTheme {
        looks: StatefulLook {
            normal: WidgetLook {
                background: bg(Color::TRANSPARENT, stroke),
                text: None,
            },
            hovered: WidgetLook {
                background: bg(accent.with_alpha(0.18), stroke),
                text: None,
            },
            active: WidgetLook {
                background: bg(accent.with_alpha(0.35), stroke),
                text: None,
            },
            disabled: WidgetLook {
                background: bg(
                    Color::TRANSPARENT,
                    Stroke::solid(accent.with_alpha(0.35), 1.5),
                ),
                text: Some(TextStyle::default().with_color(support::INK_FAINT)),
            },
        },
        ..Default::default()
    }
}

fn danger_style() -> ButtonTheme {
    let red = support::E;
    let look = |fill: Color, ink: Color| WidgetLook {
        background: Background::rounded(fill, Corners::all(4.0)),
        text: Some(TextStyle::default().with_color(ink)),
    };
    ButtonTheme {
        looks: StatefulLook {
            normal: look(red, Color::WHITE),
            hovered: look(Color::hex(0xff7e6a), Color::WHITE),
            active: look(Color::hex(0xc74734), Color::WHITE),
            disabled: look(red.with_alpha(0.4), Color::WHITE.with_alpha(0.55)),
        },
        ..Default::default()
    }
}
