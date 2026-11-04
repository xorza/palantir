//! Form controls in one composition. The left column is a settings form (switches, checkboxes, radios, combo box, slider, DragValue, themed buttons): "Airplane mode" cascade-disables the network group, and Apply drives a fake sync through `Ui::animate` (ProgressBar + Spinner). The right column demos ButtonTheme styling, and a progress bar beside spinners.

use crate::appearance::Appearance;
use crate::scaffold::api;
use crate::support;
use crate::support::{Col, Kit, Knob, Section};
use palantir::widget::AnimationRequest;
use palantir::{
    Align, Animated, AnimationSpec, Axis, Button, ButtonTheme, Checkbox, Chrome, ColorScheme,
    ComboBox, Configure, Corners, DragValue, Expander, ExpanderTheme, Metrics, Panel, ProgressBar,
    RadioGroup, RgbaF32, Separator, Sizing, Slider, SlotDefaults, Spinner, StatefulLook, Stroke,
    Switch, Text, TextEdit, TextStyleOverrides, Tooltip, Ui, VAlign, WidgetId, WidgetLook, fmt,
};

const M: Metrics = Metrics::DEFAULT;

static REGIONS: [&str; 5] = [
    "Europe",
    "North America",
    "South America",
    "Asia",
    "Oceania",
];

/// The form's radio group: when updates download. Page state, as the tour's own preferences sit
/// in the rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Updates {
    Automatic,
    Ask,
    Never,
}

#[derive(Debug)]
struct State {
    airplane: bool,
    wifi: bool,
    bluetooth: bool,
    metered: bool,
    region: &'static &'static str,
    updates: Updates,
    volume: f64,
    fps: i64,
    syncing: bool,
    /// The progress bar's share, which the knob sets.
    progress: f32,
    /// Each disclosure demo holds one, so the state a collapsed body keeps or loses is visible.
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
            region: &REGIONS[0],
            updates: Updates::Automatic,
            volume: 0.6,
            fps: 120,
            syncing: false,
            progress: 0.4,
            skipped_note: String::from("type here"),
            kept_note: String::from("type here"),
        }
    }
}

pub(crate) fn build(ui: &mut Ui, k: Kit, look: &Appearance) {
    let state_id = WidgetId::from_hash("showcase::controls::state");
    let themes = Themes {
        accent: look.accent_button(ui.color_scheme()),
        outlined: outlined_style(k, &ui.theme().button),
        danger: danger_style(k, &ui.theme().button),
    };

    ui.with_state::<State, _>(state_id, |ui, s| {
        support::cols(ui, |ui, col| match col {
            Col::Left => Section::new("Settings form")
                .apis(&[
                    api!(Panel::disabled),
                    api!(
                        Ui::animate
                            as fn(&mut Ui, AnimationRequest<f32>, AnimationSpec) -> Animated<f32>
                    ),
                ])
                .show(ui, k, |ui| form(ui, k, s, &themes)),
            Col::Right => side(ui, k, s, &themes),
        });
        Section::new("Disclosure")
            .apis(&[api!(Expander::start_open), api!(Expander::keep_body)])
            .show(ui, k, |ui| disclosure(ui, k, s));
    });
}

/// The three things an [`Expander`] decides: whether it starts open, whether its reveal animates, and whether its body keeps recording while closed. In the second column, type into both fields, collapse and reopen: the upper resets because Palantir sweeps the state of unrecorded widgets, the lower doesn't because `keep_body` records it collapsed.
fn disclosure(ui: &mut Ui, k: Kit, s: &mut State) {
    let base = ExpanderTheme::stock(ColorScheme::Dark);
    let animated = ExpanderTheme {
        defaults: SlotDefaults {
            transition: AnimationSpec::MEDIUM,
            ..base.defaults
        },
        ..base
    };
    support::cols(ui, |ui, col| match col {
        Col::Left => {
            Expander::new("Open by default")
                .start_open(true)
                .show(ui, |ui| {
                    support::note(
                        ui,
                        k,
                        "A plain section. The reveal snaps, as the stock expander theme \
                         sets no animation.",
                    );
                });
            Expander::new("Animated reveal")
                .style(&animated)
                .show(ui, |ui| {
                    support::note(
                        ui,
                        k,
                        "The same widget with an animation on its theme. The first open \
                         snaps — there is no measured height to tween against yet — and every \
                         one after it animates.",
                    );
                });
        }
        Col::Right => {
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
            support::note(
                ui,
                k,
                "Click into both fields, collapse both, then reopen: the upper field's caret \
                 and selection are gone with its state row, and the lower one's are not.",
            );
        }
    });
}

/// The button themes the page shows beside the default: palantir's accent, and two of its own.
#[derive(Debug)]
struct Themes {
    accent: ButtonTheme,
    outlined: ButtonTheme,
    danger: ButtonTheme,
}

fn form(ui: &mut Ui, k: Kit, s: &mut State, themes: &Themes) {
    Panel::vstack()
        .size((Sizing::fixed(360.0), Sizing::HUG))
        .padding(M.space.lg)
        .gap(M.space.md)
        .chrome(k.t.card())
        .show(ui, |ui| {
            group(ui, k, "NETWORK");
            Switch::new(&mut s.airplane).label("Airplane mode").show(ui);
            Panel::vstack()
                .size((Sizing::FILL, Sizing::HUG))
                .gap(M.space.md)
                .disabled(s.airplane)
                .show(ui, |ui| {
                    Switch::new(&mut s.wifi).label("Wi-Fi").show(ui);
                    Switch::new(&mut s.bluetooth).label("Bluetooth").show(ui);
                    Checkbox::new(&mut s.metered)
                        .label("Treat as metered")
                        .show(ui);
                    support::row(ui, |ui| {
                        Text::new("Region").style(&k.t.caption()).show(ui);
                        ComboBox::new(&mut s.region, &REGIONS)
                            .size((Sizing::fixed(170.0), Sizing::HUG))
                            .show(ui);
                    });
                });

            Separator::horizontal().show(ui);
            group(ui, k, "UPDATES");
            RadioGroup::new(&mut s.updates)
                .axis(Axis::X)
                .gap(M.space.md)
                .show(ui, |ui, options| {
                    for (value, label) in [
                        (Updates::Automatic, "Automatic"),
                        (Updates::Ask, "Ask"),
                        (Updates::Never, "Never"),
                    ] {
                        options.option(value).label(label).show(ui);
                    }
                });

            Separator::horizontal().show(ui);
            group(ui, k, "AUDIO & VIDEO");
            support::row(ui, |ui| {
                Slider::new(&mut s.volume, 0.0..=1.0)
                    .size((Sizing::fill(1.0), Sizing::HUG))
                    .show(ui);
                let vol = fmt!(ui, "{:.0}%", s.volume * 100.0);
                Text::new(vol)
                    .style(&k.t.caption())
                    .min_size((36.0, 0.0))
                    .show(ui);
            });
            support::row(ui, |ui| {
                DragValue::new(&mut s.fps)
                    .speed(0.25)
                    .range(24.0..=240.0)
                    .decimals(0)
                    .suffix(" fps")
                    .size((Sizing::fixed(110.0), Sizing::HUG))
                    .show(ui);
                Text::new("drag to scrub, click to type")
                    .style(&k.t.caption())
                    .show(ui);
            });

            Separator::horizontal().show(ui);
            Panel::hstack().gap(M.space.sm).show(ui, |ui| {
                if Button::new()
                    .label("Apply")
                    .style(&themes.accent)
                    .show(ui)
                    .clicked()
                {
                    s.syncing = true;
                }
                if Button::new()
                    .style(&themes.outlined)
                    .label("Reset")
                    .show(ui)
                    .clicked()
                {
                    *s = State::default();
                }
                let del = Button::new()
                    .style(&themes.danger)
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
            let frac = ui
                .animate(
                    AnimationRequest::new(
                        WidgetId::from_hash("showcase::controls::sync"),
                        "frac",
                        target,
                    ),
                    AnimationSpec::SPRING,
                )
                .value;
            if s.syncing && frac > 0.995 {
                s.syncing = false;
            }
            // The bar shows only while a sync runs, so the form holds no empty track at rest.
            if s.syncing {
                ProgressBar::new(frac).show(ui);
                support::row(ui, |ui| {
                    Spinner::new().size(16.0).show(ui);
                    let pct = fmt!(ui, "syncing {:.0}%", frac * 100.0);
                    Text::new(pct).style(&k.t.caption()).show(ui);
                });
            }
        });
}

fn side(ui: &mut Ui, k: Kit, s: &mut State, themes: &Themes) {
    Section::new("Button themes")
        .apis(&[api!(type ButtonTheme), api!(ButtonTheme::accent)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The default and palantir's accent, then an outlined and a danger ButtonTheme of \
                 the page's own, each beside its disabled look.",
            );
            support::well(ui, k, |ui| {
                support::row(ui, |ui| {
                    Button::new().label("default").show(ui);
                    Button::new().label("disabled").disabled(true).show(ui);
                });
                support::row(ui, |ui| {
                    Button::new().style(&themes.accent).label("accent").show(ui);
                    Button::new()
                        .style(&themes.accent)
                        .label("disabled")
                        .disabled(true)
                        .show(ui);
                });
                support::row(ui, |ui| {
                    Button::new()
                        .style(&themes.outlined)
                        .label("outlined")
                        .show(ui);
                    Button::new()
                        .style(&themes.outlined)
                        .label("disabled")
                        .disabled(true)
                        .show(ui);
                });
                support::row(ui, |ui| {
                    Button::new().style(&themes.danger).label("danger").show(ui);
                    Button::new()
                        .style(&themes.danger)
                        .label("disabled")
                        .disabled(true)
                        .show(ui);
                });
            });
        });

    Section::new("Progress")
        .apis(&[api!(ProgressBar::new), api!(Spinner::new)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A bar for work whose share is known, set here by hand, and spinners for work \
                 whose share is not.",
            );
            support::well(ui, k, |ui| {
                Knob {
                    label: "done",
                    range: 0.0..=1.0,
                    step: 0.01,
                    unit: "",
                }
                .show(ui, k, &mut s.progress);
                ProgressBar::new(s.progress).show(ui);
                Panel::hstack()
                    .gap(M.space.lg)
                    .child_align(Align::v(VAlign::Center))
                    .show(ui, |ui| {
                        Spinner::new().size(20.0).show(ui);
                        Spinner::new().size(32.0).show(ui);
                        Spinner::new().size(48.0).color(k.swatch.orange).show(ui);
                    });
            });
        });
}

fn group(ui: &mut Ui, k: Kit, label: &'static str) {
    Text::new(label)
        .id_salt(label)
        .style(&k.t.caption())
        .show(ui);
}

/// Transparent fill, accent stroke: reads as a "selectable surface".
fn outlined_style(k: Kit, base: &ButtonTheme) -> ButtonTheme {
    let accent = k.t.accent;
    let stroke = Stroke::new(accent, 1.5);
    let chrome = |fill: RgbaF32, stroke| {
        Chrome::rounded(fill, Corners::all(M.radius.control)).with_border(stroke)
    };
    ButtonTheme {
        unselected: StatefulLook {
            normal: WidgetLook {
                chrome: chrome(RgbaF32::TRANSPARENT, stroke),
                text: TextStyleOverrides::NONE,
            },
            hovered: WidgetLook {
                chrome: chrome(accent.with_alpha(0.18), stroke),
                text: TextStyleOverrides::NONE,
            },
            active: WidgetLook {
                chrome: chrome(accent.with_alpha(0.35), stroke),
                text: TextStyleOverrides::NONE,
            },
            disabled: WidgetLook {
                chrome: chrome(
                    RgbaF32::TRANSPARENT,
                    Stroke::new(accent.with_alpha(0.35), 1.5),
                ),
                text: TextStyleOverrides::NONE.with_color(k.t.ink_faint),
            },
        },
        ..base.clone()
    }
}

/// The red swatch under the swatch ink, which it holds 4.5:1 against; hovered and pressed it
/// moves toward white, which only raises that contrast; disabled, it is the theme's button.
fn danger_style(k: Kit, base: &ButtonTheme) -> ButtonTheme {
    let red = k.swatch.red;
    let ink = support::on_swatch_style().color;
    let look = |toward_white: f32| {
        let fill = RgbaF32::new(
            red.r + (1.0 - red.r) * toward_white,
            red.g + (1.0 - red.g) * toward_white,
            red.b + (1.0 - red.b) * toward_white,
            1.0,
        );
        WidgetLook {
            chrome: Chrome::rounded(fill, Corners::all(M.radius.control)),
            text: TextStyleOverrides::NONE.with_color(ink),
        }
    };
    ButtonTheme {
        unselected: StatefulLook {
            normal: look(0.0),
            hovered: look(0.12),
            active: look(0.24),
            disabled: base.unselected.disabled.clone(),
        },
        ..base.clone()
    }
}
