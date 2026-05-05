use palantir::{
    Background, Button, ButtonTheme, Color, Configure, Corners, Panel, Sizing, Stroke, TextStyle,
    Ui, ButtonStyle,
};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Default style: filled buttons. Hover, press and disabled produce
            // visible state changes (hover / press require pointing at them).
            row(ui, "default", |ui| {
                Button::new().with_id("d-1").label("normal").show(ui);
                Button::new().with_id("d-2").label("normal 2").show(ui);
                Button::new()
                    .with_id("d-3")
                    .label("disabled")
                    .disabled(true)
                    .show(ui);
            });

            // Outlined style: transparent fill, hover tints in.
            row(ui, "outlined", |ui| {
                Button::new()
                    .with_id("o-1")
                    .style(outlined_style())
                    .label("normal")
                    .show(ui);
                Button::new()
                    .with_id("o-2")
                    .style(outlined_style())
                    .label("normal 2")
                    .show(ui);
                Button::new()
                    .with_id("o-3")
                    .style(outlined_style())
                    .label("disabled")
                    .disabled(true)
                    .show(ui);
            });

            // Custom style with sharper corners + bolder hover.
            row(ui, "custom", |ui| {
                Button::new()
                    .with_id("c-1")
                    .style(danger_style())
                    .label("delete")
                    .show(ui);
                Button::new()
                    .with_id("c-2")
                    .style(danger_style())
                    .label("danger")
                    .show(ui);
            });
        });
}

fn row(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .with_id(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(8.0)
        .show(ui, body);
}

fn outlined_style() -> ButtonTheme {
    let stroke = Some(Stroke {
        width: 1.5,
        color: Color::rgb(0.4, 0.5, 0.7),
    });
    let bg = |fill, stroke| Background {
        fill,
        stroke,
        radius: Corners::all(4.0),
    };
    ButtonTheme {
        normal: ButtonStyle {
            background: Some(bg(Color::TRANSPARENT, stroke)),
            text: TextStyle::default().with_color(Color::rgb(0.85, 0.88, 0.95)),
        },
        hovered: ButtonStyle {
            background: Some(bg(Color::rgba(0.4, 0.5, 0.7, 0.18), stroke)),
            text: TextStyle::default().with_color(Color::WHITE),
        },
        pressed: ButtonStyle {
            background: Some(bg(Color::rgba(0.4, 0.5, 0.7, 0.35), stroke)),
            text: TextStyle::default().with_color(Color::WHITE),
        },
        disabled: ButtonStyle {
            background: Some(bg(
                Color::TRANSPARENT,
                Some(Stroke {
                    width: 1.5,
                    color: Color::rgba(0.4, 0.5, 0.7, 0.35),
                }),
            )),
            text: TextStyle::default().with_color(Color::rgba(0.85, 0.88, 0.95, 0.45)),
        },
    }
}

fn danger_style() -> ButtonTheme {
    let red = Color::rgb(0.85, 0.30, 0.30);
    let bg = |fill| Background {
        fill,
        stroke: None,
        radius: Corners::all(2.0),
    };
    ButtonTheme {
        normal: ButtonStyle {
            background: Some(bg(red)),
            text: TextStyle::default().with_color(Color::WHITE),
        },
        hovered: ButtonStyle {
            background: Some(bg(Color::rgb(0.95, 0.40, 0.35))),
            text: TextStyle::default().with_color(Color::WHITE),
        },
        pressed: ButtonStyle {
            background: Some(bg(Color::rgb(0.70, 0.20, 0.20))),
            text: TextStyle::default().with_color(Color::WHITE),
        },
        disabled: ButtonStyle {
            background: Some(bg(Color::rgba(0.85, 0.30, 0.30, 0.4))),
            text: TextStyle::default().with_color(Color::rgba(1.0, 1.0, 1.0, 0.55)),
        },
    }
}
