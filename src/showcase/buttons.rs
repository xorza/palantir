//! Button styling showcase. The whole point of this tab IS custom
//! styling — the `outlined` and `danger` themes show how to build a
//! ButtonTheme from scratch when an app wants something different
//! from the framework default. Default styling demoed in the first row.

use palantir::{
    Background, Button, ButtonTheme, Color, Configure, Corners, Panel, Sizing, Stroke, TextStyle,
    Ui, WidgetLook,
};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Default style: framework-provided theme. Hover/press/disabled
            // states all visible without any per-button override.
            row(ui, "default", |ui| {
                Button::new().id_salt("d-1").label("normal").show(ui);
                Button::new().id_salt("d-2").label("normal 2").show(ui);
                Button::new()
                    .id_salt("d-3")
                    .label("disabled")
                    .disabled(true)
                    .show(ui);
            });

            // Outlined style: transparent fill + accent stroke. App
            // override demonstrating a different visual treatment.
            row(ui, "outlined", |ui| {
                Button::new()
                    .id_salt("o-1")
                    .style(outlined_style())
                    .label("normal")
                    .show(ui);
                Button::new()
                    .id_salt("o-2")
                    .style(outlined_style())
                    .label("normal 2")
                    .show(ui);
                Button::new()
                    .id_salt("o-3")
                    .style(outlined_style())
                    .label("disabled")
                    .disabled(true)
                    .show(ui);
            });

            // Danger style: bold red fill, sharp corners.
            row(ui, "custom", |ui| {
                Button::new()
                    .id_salt("c-1")
                    .style(danger_style())
                    .label("delete")
                    .show(ui);
                Button::new()
                    .id_salt("c-2")
                    .style(danger_style())
                    .label("danger")
                    .show(ui);
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

fn outlined_style() -> ButtonTheme {
    // Stroke uses the palette's `border_focused` so the outlined
    // variant reads as "selectable surface" matching the rest of the
    // theme.
    let accent = Color::hex(0x4cd3ff);
    let stroke = Stroke::solid(accent, 1.5);
    let bg = |fill: Color, stroke| Background {
        fill: fill.into(),
        stroke,
        radius: Corners::all(4.0),
        shadow: None,
    };
    ButtonTheme {
        normal: WidgetLook {
            background: Some(bg(Color::TRANSPARENT, stroke)),
            text: None,
        },
        hovered: WidgetLook {
            background: Some(bg(
                Color::linear_rgba(accent.r, accent.g, accent.b, 0.18),
                stroke,
            )),
            text: None,
        },
        pressed: WidgetLook {
            background: Some(bg(
                Color::linear_rgba(accent.r, accent.g, accent.b, 0.35),
                stroke,
            )),
            text: None,
        },
        disabled: WidgetLook {
            background: Some(bg(
                Color::TRANSPARENT,
                Stroke::solid(Color::linear_rgba(accent.r, accent.g, accent.b, 0.35), 1.5),
            )),
            text: Some(TextStyle::default().with_color(Color::hex(0x878a8d))),
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
        radius: Corners::all(2.0),
        shadow: None,
    };
    ButtonTheme {
        normal: WidgetLook {
            background: Some(bg(red)),
            text: Some(TextStyle::default().with_color(Color::WHITE)),
        },
        hovered: WidgetLook {
            background: Some(bg(Color::hex(0xff7e6a))),
            text: Some(TextStyle::default().with_color(Color::WHITE)),
        },
        pressed: WidgetLook {
            background: Some(bg(Color::hex(0xc74734))),
            text: Some(TextStyle::default().with_color(Color::WHITE)),
        },
        disabled: WidgetLook {
            background: Some(bg(Color::linear_rgba(red.r, red.g, red.b, 0.4))),
            text: Some(TextStyle::default().with_color(Color::linear_rgba(1.0, 1.0, 1.0, 0.55))),
        },
        ..Default::default()
    }
}
