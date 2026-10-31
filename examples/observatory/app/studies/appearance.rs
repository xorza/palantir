//! The appearance study: the palette of the shown scheme and a few steps, edited live and applied
//! to every study, each color with its contrast against the ground it sits on.

use palantir::{
    Align, Button, ColorButton, ColorPicker, ColorScheme, Configure, DragValue, Palette, Panel,
    RadioButton, RgbaF32, SchemePreference, Sizing, Text, Theme, Ui, VAlign, fmt,
};

use crate::app::model::Observatory;
use crate::app::model::appearance::Preset;
use crate::app::scaffold::{StudyHeader, Tokens, api};

pub(crate) fn header(ui: &mut Ui, t: Tokens) {
    StudyHeader {
        title: "Appearance",
        blurb: "The palette of the shown scheme and the main steps, applied to every study as you \
                edit. Each color reads its contrast against the ground it sits on, against the \
                WCAG floor palantir's palettes are tested to. Night vision swaps in red on black.",
        keys: &["click", "drag", "Esc"],
        apis: &[
            api!(Theme::from_parts),
            api!(Ui::set_theme_for as fn(&mut Ui, ColorScheme, Theme)),
            api!(Ui::set_scheme_preference),
            api!(RgbaF32::contrast_ratio),
            api!(type palantir::ColorButton),
        ],
        showcase: &["colors", "controls"],
        source: "examples/observatory/app/studies/appearance.rs",
    }
    .show(ui, t);
}

/// A palette role: its name, how to read and write it, the role it sits on, and the floor it
/// holds there (`None` for a role no text or mark sits on).
#[derive(Clone, Copy, Debug)]
struct Role {
    name: &'static str,
    get: fn(&Palette) -> RgbaF32,
    set: fn(&mut Palette, RgbaF32),
    /// The role it sits on; `None` for a role that sits on nothing, as a shadow under a surface.
    ground: Option<fn(&Palette) -> RgbaF32>,
    floor: Option<f32>,
}

/// Text is held to WCAG 1.4.3's 4.5:1, a focus ring or an accent mark to 1.4.11's 3:1, a rung to
/// the 1.10:1 that keeps two stacked surfaces apart.
const ROLES: [Role; 12] = [
    Role {
        name: "text",
        get: |p| p.text,
        set: |p, c| p.text = c,
        ground: Some(|p| p.window_bg),
        floor: Some(4.5),
    },
    Role {
        name: "text_muted",
        get: |p| p.text_muted,
        set: |p, c| p.text_muted = c,
        ground: Some(|p| p.element_mid),
        floor: Some(4.5),
    },
    Role {
        name: "text_disabled",
        get: |p| p.text_disabled,
        set: |p, c| p.text_disabled = c,
        ground: Some(|p| p.window_bg),
        floor: None,
    },
    Role {
        name: "window_bg",
        get: |p| p.window_bg,
        set: |p, c| p.window_bg = c,
        ground: Some(|p| p.surround),
        floor: Some(1.10),
    },
    Role {
        name: "surround",
        get: |p| p.surround,
        set: |p, c| p.surround = c,
        ground: Some(|p| p.element_mid),
        floor: Some(1.10),
    },
    Role {
        name: "element",
        get: |p| p.element,
        set: |p, c| p.element = c,
        ground: Some(|p| p.window_bg),
        floor: None,
    },
    Role {
        name: "element_mid",
        get: |p| p.element_mid,
        set: |p, c| p.element_mid = c,
        ground: Some(|p| p.element_strong),
        floor: Some(1.10),
    },
    Role {
        name: "element_strong",
        get: |p| p.element_strong,
        set: |p, c| p.element_strong = c,
        ground: Some(|p| p.window_bg),
        floor: None,
    },
    Role {
        name: "border_focused",
        get: |p| p.border_focused,
        set: |p, c| p.border_focused = c,
        ground: Some(|p| p.element_mid),
        floor: Some(3.0),
    },
    Role {
        name: "accent",
        get: |p| p.accent,
        set: |p, c| p.accent = c,
        ground: Some(|p| p.element_mid),
        floor: Some(3.0),
    },
    Role {
        name: "on_accent",
        get: |p| p.on_accent,
        set: |p, c| p.on_accent = c,
        ground: Some(|p| p.accent),
        floor: Some(4.5),
    },
    Role {
        name: "shadow",
        get: |p| p.shadow,
        set: |p, c| p.shadow = c,
        ground: None,
        floor: None,
    },
];

pub(crate) fn show(ui: &mut Ui, model: &mut Observatory, t: Tokens) {
    let scheme = ui.color_scheme();
    Panel::hstack()
        .id_salt("appearance")
        .size((Sizing::FILL, Sizing::FILL))
        .gap(24.0)
        .show(ui, |ui| {
            Panel::vstack()
                .id_salt("palette")
                .size((Sizing::fixed(520.0), Sizing::HUG))
                .padding(16.0)
                .gap(6.0)
                .chrome(t.card())
                .show(ui, |ui| {
                    let label = match scheme {
                        ColorScheme::Light => "LIGHT PALETTE",
                        ColorScheme::Dark => "DARK PALETTE",
                    };
                    Text::new(label).style(&t.heading()).show(ui);
                    for role in ROLES {
                        role_row(ui, model, scheme, role, t);
                    }
                });
            Panel::vstack()
                .id_salt("side")
                .size((Sizing::FILL, Sizing::HUG))
                .gap(16.0)
                .show(ui, |ui| {
                    presets(ui, model, t);
                    steps(ui, model, t);
                    export(ui, model, scheme, t);
                });
        });
}

fn role_row(ui: &mut Ui, model: &mut Observatory, scheme: ColorScheme, role: Role, t: Tokens) {
    let palette = model.appearance.palette(scheme);
    let mut color = (role.get)(palette);
    let ratio = role
        .ground
        .map(|ground| color.contrast_ratio(ground(palette)));
    let mut changed = false;
    Panel::hstack()
        .id_salt(role.name)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(12.0)
        .child_align(Align::v(VAlign::Center))
        .show(ui, |ui| {
            changed = ColorButton::new(ColorPicker::new(&mut color).alpha(true))
                .show(ui)
                .changed;
            Text::new(role.name)
                .style(&t.mono(t.ink))
                .size((Sizing::fixed(140.0), Sizing::HUG))
                .show(ui);
            let srgb = color.to_srgba8();
            Text::new(fmt!(ui, "#{:02x}{:02x}{:02x}", srgb.r, srgb.g, srgb.b))
                .style(&t.mono(t.ink_dim))
                .size((Sizing::fixed(80.0), Sizing::HUG))
                .show(ui);
            let verdict = match (ratio, role.floor) {
                (Some(ratio), Some(floor)) if ratio >= floor => {
                    fmt!(ui, "{ratio:.2}:1 \u{2713} {floor:.2}")
                }
                (Some(ratio), Some(floor)) => fmt!(ui, "{ratio:.2}:1 \u{2717} {floor:.2}"),
                (Some(ratio), None) => fmt!(ui, "{ratio:.2}:1"),
                (None, _) => ui.intern("\u{2014}"),
            };
            let ink = match (ratio, role.floor) {
                (Some(ratio), Some(floor)) if ratio < floor => t.accent,
                _ => t.ink_dim,
            };
            Text::new(verdict).style(&t.mono(ink)).show(ui);
        });
    if changed {
        (role.set)(model.appearance.palette_mut(scheme), color);
    }
}

fn presets(ui: &mut Ui, model: &mut Observatory, t: Tokens) {
    let mut preset = model.appearance.preset();
    Panel::vstack().id_salt("presets").gap(6.0).show(ui, |ui| {
        Text::new("PRESET").style(&t.heading()).show(ui);
        for (value, label) in [
            (Preset::Stock, "stock"),
            (Preset::NightVision, "night vision"),
        ] {
            RadioButton::new(&mut preset, value)
                .id_salt(label)
                .label(label)
                .show(ui);
        }
    });
    if preset != model.appearance.preset() {
        model.appearance.set_preset(preset);
        if preset == Preset::NightVision {
            ui.set_scheme_preference(SchemePreference::Dark);
        }
    }
}

/// The steps the study edits, each bound through an `f64`, as a drag value binds one.
fn steps(ui: &mut Ui, model: &mut Observatory, t: Tokens) {
    let m = &model.appearance.metrics;
    let mut values = [
        f64::from(m.text.body),
        f64::from(m.space.md),
        f64::from(m.radius.control),
        f64::from(m.radius.float),
    ];
    let mut changed = false;
    Panel::vstack().id_salt("steps").gap(6.0).show(ui, |ui| {
        Text::new("STEPS").style(&t.heading()).show(ui);
        let labels = ["body text", "space md", "control radius", "float radius"];
        for (value, label) in values.iter_mut().zip(labels) {
            Panel::hstack()
                .id_salt(label)
                .gap(12.0)
                .child_align(Align::v(VAlign::Center))
                .show(ui, |ui| {
                    Text::new(label)
                        .style(&t.dim())
                        .size((Sizing::fixed(120.0), Sizing::HUG))
                        .show(ui);
                    changed |= DragValue::new(value)
                        .range(0.0..=32.0)
                        .decimals(0)
                        .suffix(" px")
                        .size((Sizing::fixed(100.0), Sizing::HUG))
                        .show(ui)
                        .changed;
                });
        }
    });
    if changed {
        let m = model.appearance.metrics_mut();
        let [body, md, control, float] = values.map(|v| v as f32);
        // A body under 1 px measures nothing, so the text floor is 1.
        m.text.body = body.max(1.0);
        m.space.md = md;
        m.radius.control = control;
        m.radius.float = float;
    }
}

fn export(ui: &mut Ui, model: &mut Observatory, scheme: ColorScheme, t: Tokens) {
    Panel::vstack().id_salt("export").gap(6.0).show(ui, |ui| {
        Text::new("SOURCE").style(&t.heading()).show(ui);
        Panel::hstack().id_salt("buttons").gap(8.0).show(ui, |ui| {
            if Button::new()
                .id_salt("copy")
                .label("Copy as Rust")
                .show(ui)
                .clicked()
            {
                let source = model.appearance.rust_source(scheme);
                ui.clipboard().set_text(&source).ok();
            }
            if Button::new()
                .id_salt("reset")
                .label("Reset")
                .show(ui)
                .clicked()
            {
                let preset = model.appearance.preset();
                model.appearance.set_preset(preset);
            }
        });
        Text::new("Copies this scheme's Palette and the steps as Rust literals.")
            .style(&t.caption())
            .show(ui);
    });
}
