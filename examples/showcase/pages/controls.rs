//! Form controls in one composition. The left column is a settings form
//! wiring switches, checkboxes, radios, a combo box, a slider, a
//! DragValue and themed buttons together: "Airplane mode" cascade-disables
//! the network group (the panel's `disabled` flows to every descendant),
//! and Apply drives a fake sync through `Ui::animate` (ProgressBar +
//! Spinner). The right column demos ButtonTheme styling, label eliding,
//! spinner sizing, and echoes the live form state.

use crate::support;
use crate::support::{Column, api, columns, note, note_style, readout, row, section, well};
use palantir::{
    Align, AnimationSpec, Background, Button, ButtonTheme, Checkbox, ComboBox, Configure, Corners,
    DragValue, Expander, ExpanderTheme, Panel, ProgressBar, RadioButton, RgbaF32, Separator,
    Sizing, Slider, SlotDefaults, Spinner, StatefulLook, Stroke, Switch, Text, TextEdit,
    TextStyleOverrides, TextWrap, Tooltip, Ui, VAlign, WidgetId, WidgetLook, fmt,
};

const REGIONS: [&str; 5] = [
    "Europe",
    "North America",
    "South America",
    "Asia",
    "Oceania",
];

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
    region: usize,
    appearance: Appearance,
    reduce_motion: bool,
    volume: f64,
    fps: i64,
    syncing: bool,
    /// The two disclosure demos below hold one of these each, so the
    /// state a collapsed body keeps — or loses — is visible.
    skipped_note: String,
    kept_note: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            airplane: false,
            wifi: true,
            bluetooth: false,
            metered: false,
            region: 0,
            appearance: Appearance::System,
            reduce_motion: false,
            volume: 0.6,
            fps: 120,
            syncing: false,
            skipped_note: String::from("type here"),
            kept_note: String::from("type here"),
        }
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::controls::state");
    let outlined = outlined_style();
    let danger = danger_style();

    ui.with_state::<State, _>(state_id, |ui, s| {
        columns(ui, |ui, column| match column {
            Column::Left => section(
                ui,
                "Settings form",
                &[
                    api!(Panel::disabled),
                    api!(
                        Ui::animate
                            as fn(
                                &mut Ui,
                                WidgetId,
                                &'static str,
                                f32,
                                Option<AnimationSpec>,
                            ) -> f32
                    ),
                ],
                |ui| form(ui, s, &outlined, &danger),
            ),
            Column::Right => side(ui, s, &outlined, &danger),
        });
        section(
            ui,
            "Disclosure",
            &[api!(Expander::start_open), api!(Expander::keep_body)],
            |ui| disclosure(ui, s),
        );
    });
}

/// The three things an [`Expander`] decides: whether it starts open,
/// whether its reveal animates, and whether its body keeps recording
/// while closed.
///
/// The second column is the demo worth reading. Type into both fields,
/// collapse both sections, then reopen them: the upper one is reset,
/// because Palantir sweeps the cross-frame state of any widget that
/// stops being recorded, and the lower one is not, because `keep_body`
/// records it collapsed instead.
fn disclosure(ui: &mut Ui, s: &mut State) {
    let base = ExpanderTheme::default();
    let animated = ExpanderTheme {
        defaults: SlotDefaults {
            animation: Some(AnimationSpec::MEDIUM),
            ..base.defaults
        },
        ..base
    };
    columns(ui, |ui, column| match column {
        Column::Left => {
            Expander::new("Open by default")
                .start_open(true)
                .show(ui, |ui| {
                    note(
                        ui,
                        "A plain section. The reveal snaps, because the library leaves \
                         animation opt-in.",
                    );
                });
            Expander::new("Animated reveal")
                .style(&animated)
                .show(ui, |ui| {
                    note(
                        ui,
                        "The same widget with an animation on its theme. The first open \
                         snaps — there is no measured height to tween against yet — and every \
                         one after it animates.",
                    );
                });
        }
        Column::Right => {
            Expander::new("Skips its body")
                .start_open(true)
                .show(ui, |ui| {
                    TextEdit::new(&mut s.skipped_note)
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui);
                });
            Expander::new("Keeps its body")
                .start_open(true)
                .keep_body(true)
                .show(ui, |ui| {
                    TextEdit::new(&mut s.kept_note)
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui);
                });
            note(
                ui,
                "Click into both fields, collapse both, then reopen: the upper field's caret \
                 and selection are gone with its state row, and the lower one's are not.",
            );
        }
    });
}

fn form(ui: &mut Ui, s: &mut State, outlined: &ButtonTheme, danger: &ButtonTheme) {
    Panel::vstack()
        .size((Sizing::fixed(360.0), Sizing::HUG))
        .padding(16.0)
        .gap(10.0)
        .background(support::well_bg())
        .show(ui, |ui| {
            group(ui, "NETWORK");
            Switch::new(&mut s.airplane).label("Airplane mode").show(ui);
            Panel::vstack()
                .size((Sizing::FILL, Sizing::HUG))
                .gap(10.0)
                .disabled(s.airplane)
                .show(ui, |ui| {
                    Switch::new(&mut s.wifi).label("Wi-Fi").show(ui);
                    Switch::new(&mut s.bluetooth).label("Bluetooth").show(ui);
                    Checkbox::new(&mut s.metered)
                        .label("Treat as metered")
                        .show(ui);
                    row(ui, |ui| {
                        Text::new("Region").style(&note_style()).show(ui);
                        ComboBox::new(&mut s.region, &REGIONS)
                            .size((Sizing::fixed(170.0), Sizing::HUG))
                            .show(ui);
                    });
                });

            Separator::horizontal().show(ui);
            group(ui, "APPEARANCE");
            Panel::hstack().gap(12.0).show(ui, |ui| {
                for (value, label) in [
                    (Appearance::System, "System"),
                    (Appearance::Light, "Light"),
                    (Appearance::Dark, "Dark"),
                ] {
                    RadioButton::new(&mut s.appearance, value)
                        .id_salt(label)
                        .label(label)
                        .show(ui);
                }
            });
            Checkbox::new(&mut s.reduce_motion)
                .label("Reduce motion")
                .show(ui);

            // Thick tinted variant of Separator, in situ.
            Separator::horizontal()
                .thickness(3.0)
                .color(support::A)
                .show(ui);
            group(ui, "AUDIO & VIDEO");
            row(ui, |ui| {
                Slider::new(&mut s.volume, 0.0..=1.0)
                    .size((Sizing::fill(1.0), Sizing::HUG))
                    .show(ui);
                let vol = fmt!(ui, "{:.0}%", s.volume * 100.0);
                Text::new(vol)
                    .style(&note_style())
                    .min_size((36.0, 0.0))
                    .show(ui);
            });
            row(ui, |ui| {
                DragValue::new(&mut s.fps)
                    .editable(true)
                    .speed(0.25)
                    .range(24.0..=240.0)
                    .decimals(0)
                    .suffix(" fps")
                    .size((Sizing::fixed(110.0), Sizing::HUG))
                    .show(ui);
                Text::new("drag to scrub, click to type")
                    .style(&note_style())
                    .show(ui);
            });

            Separator::horizontal().show(ui);
            Panel::hstack().gap(8.0).show(ui, |ui| {
                if Button::new().label("Apply").show(ui).clicked() {
                    s.syncing = true;
                }
                if Button::new()
                    .style(outlined)
                    .label("Reset")
                    .show(ui)
                    .clicked()
                {
                    *s = State::default();
                }
                let del = Button::new()
                    .style(danger)
                    .label("Delete profile")
                    .show(ui)
                    .snapshot();
                Tooltip::on(
                    &del,
                    "Deletes the profile. No undo — hence the danger theme.",
                )
                .show(ui);
            });

            let target = if s.syncing { 1.0 } else { 0.0 };
            let frac = ui.animate(
                WidgetId::from_hash("showcase::controls::sync"),
                "frac",
                target,
                Some(AnimationSpec::SPRING),
            );
            if s.syncing && frac > 0.995 {
                s.syncing = false;
            }
            ProgressBar::new(frac).show(ui);
            if s.syncing {
                row(ui, |ui| {
                    Spinner::new().diameter(16.0).show(ui);
                    let pct = fmt!(ui, "syncing {:.0}%", frac * 100.0);
                    Text::new(pct).style(&note_style()).show(ui);
                });
            }
        });
}

fn side(ui: &mut Ui, s: &State, outlined: &ButtonTheme, danger: &ButtonTheme) {
    section(ui, "Button themes", &[api!(type ButtonTheme)], |ui| {
        note(
            ui,
            "The default, an outlined and a danger ButtonTheme, each beside its disabled look.",
        );
        row(ui, |ui| {
            Button::new().label("default").show(ui);
            Button::new().label("disabled").disabled(true).show(ui);
        });
        row(ui, |ui| {
            Button::new().style(outlined).label("outlined").show(ui);
            Button::new()
                .style(outlined)
                .label("disabled")
                .disabled(true)
                .show(ui);
        });
        row(ui, |ui| {
            Button::new().style(danger).label("danger").show(ui);
            Button::new()
                .style(danger)
                .label("disabled")
                .disabled(true)
                .show(ui);
        });
    });

    section(ui, "Label overflow", &[api!(Button::text_wrap)], |ui| {
        note(
            ui,
            "A fixed-width button cuts a long label at its box. Single-line wrapping lets the \
             label run past the box; a Hug-width button grows to fit.",
        );
        row(ui, |ui| {
            Button::new()
                .size((Sizing::fixed(140.0), Sizing::HUG))
                .label("Screenshot 2026-05-28 at 01.21.25.png")
                .show(ui);
            Button::new().label("fits its content").show(ui);
        });
        row(ui, |ui| {
            Button::new()
                .size((Sizing::fixed(140.0), Sizing::HUG))
                .text_wrap(TextWrap::SingleLine)
                .label("Screenshot 2026-05-28 at 01.21.25.png")
                .show(ui);
        });
    });

    section(ui, "Spinners", &[api!(Spinner::diameter)], |ui| {
        Panel::hstack()
            .gap(20.0)
            .child_align(Align::v(VAlign::Center))
            .show(ui, |ui| {
                Spinner::new().diameter(20.0).show(ui);
                Spinner::new().diameter(32.0).show(ui);
                Spinner::new().diameter(48.0).color(support::B).show(ui);
            });
    });

    section(ui, "Live state", &[], |ui| {
        well(ui, |ui| {
            let net = fmt!(
                ui,
                "airplane={} wifi={} bluetooth={} metered={}",
                s.airplane,
                s.wifi,
                s.bluetooth,
                s.metered
            );
            readout(ui, "network", net);
            readout(ui, "region", REGIONS[s.region]);
            let look = fmt!(ui, "{:?}, reduce_motion={}", s.appearance, s.reduce_motion);
            readout(ui, "appearance", look);
            let av = fmt!(ui, "volume={:.2} fps={}", s.volume, s.fps);
            readout(ui, "audio & video", av);
        });
    });
}

fn group(ui: &mut Ui, label: &'static str) {
    Text::new(label)
        .id_salt(label)
        .style(&support::caption_style())
        .show(ui);
}

/// Transparent fill, accent stroke — reads as "selectable surface"
/// against the rest of the theme.
fn outlined_style() -> ButtonTheme {
    let accent = support::ACCENT;
    let stroke = Stroke::new(accent, 1.5);
    let bg =
        |fill: RgbaF32, stroke| Background::rounded(fill, Corners::all(4.0)).with_border(stroke);
    ButtonTheme {
        looks: StatefulLook {
            normal: WidgetLook {
                background: bg(RgbaF32::TRANSPARENT, stroke),
                text: TextStyleOverrides::NONE,
            },
            hovered: WidgetLook {
                background: bg(accent.with_alpha(0.18), stroke),
                text: TextStyleOverrides::NONE,
            },
            active: WidgetLook {
                background: bg(accent.with_alpha(0.35), stroke),
                text: TextStyleOverrides::NONE,
            },
            disabled: WidgetLook {
                background: bg(
                    RgbaF32::TRANSPARENT,
                    Stroke::new(accent.with_alpha(0.35), 1.5),
                ),
                text: TextStyleOverrides::NONE.with_color(support::INK_FAINT),
            },
        },
        ..Default::default()
    }
}

fn danger_style() -> ButtonTheme {
    let red = support::E;
    let look = |fill: RgbaF32, ink: RgbaF32| WidgetLook {
        background: Background::rounded(fill, Corners::all(4.0)),
        text: TextStyleOverrides::NONE.with_color(ink),
    };
    ButtonTheme {
        looks: StatefulLook {
            normal: look(red, RgbaF32::WHITE),
            hovered: look(RgbaF32::hex(0xff7e6a), RgbaF32::WHITE),
            active: look(RgbaF32::hex(0xc74734), RgbaF32::WHITE),
            disabled: look(red.with_alpha(0.4), RgbaF32::WHITE.with_alpha(0.55)),
        },
        ..Default::default()
    }
}
