use palantir::{
    Button, ButtonStyle, Color, Corners, Element, HStack, Sizing, Stroke, Ui, VStack, Visuals,
};

pub fn build(ui: &mut Ui) {
    VStack::new()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Default style: filled buttons. Hover, press and disabled produce
            // visible state changes (hover / press require pointing at them).
            row(ui, "default", |ui| {
                Button::with_id("d-1").label("normal").show(ui);
                Button::with_id("d-2").label("normal 2").show(ui);
                Button::with_id("d-3")
                    .label("disabled")
                    .disabled(true)
                    .show(ui);
            });

            // Outlined style: transparent fill, hover tints in.
            row(ui, "outlined", |ui| {
                Button::with_id("o-1")
                    .style(outlined_style())
                    .label("normal")
                    .show(ui);
                Button::with_id("o-2")
                    .style(outlined_style())
                    .label("normal 2")
                    .show(ui);
                Button::with_id("o-3")
                    .style(outlined_style())
                    .label("disabled")
                    .disabled(true)
                    .show(ui);
            });

            // Custom style with sharper corners + bolder hover.
            row(ui, "custom", |ui| {
                Button::with_id("c-1")
                    .style(danger_style())
                    .label("delete")
                    .show(ui);
                Button::with_id("c-2")
                    .style(danger_style())
                    .label("danger")
                    .show(ui);
            });
        });
}

fn row(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    HStack::with_id(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(8.0)
        .show(ui, body);
}

fn outlined_style() -> ButtonStyle {
    let stroke = Some(Stroke {
        width: 1.5,
        color: Color::rgb(0.4, 0.5, 0.7),
    });
    ButtonStyle {
        normal: Visuals {
            fill: Color::TRANSPARENT,
            stroke,
            text: Color::rgb(0.85, 0.88, 0.95),
        },
        hovered: Visuals {
            fill: Color::rgba(0.4, 0.5, 0.7, 0.18),
            stroke,
            text: Color::WHITE,
        },
        pressed: Visuals {
            fill: Color::rgba(0.4, 0.5, 0.7, 0.35),
            stroke,
            text: Color::WHITE,
        },
        disabled: Visuals {
            fill: Color::TRANSPARENT,
            stroke: Some(Stroke {
                width: 1.5,
                color: Color::rgba(0.4, 0.5, 0.7, 0.35),
            }),
            text: Color::rgba(0.85, 0.88, 0.95, 0.45),
        },
        radius: Corners::all(4.0),
    }
}

fn danger_style() -> ButtonStyle {
    let red = Color::rgb(0.85, 0.30, 0.30);
    ButtonStyle {
        normal: Visuals {
            fill: red,
            stroke: None,
            text: Color::WHITE,
        },
        hovered: Visuals {
            fill: Color::rgb(0.95, 0.40, 0.35),
            stroke: None,
            text: Color::WHITE,
        },
        pressed: Visuals {
            fill: Color::rgb(0.70, 0.20, 0.20),
            stroke: None,
            text: Color::WHITE,
        },
        disabled: Visuals {
            fill: Color::rgba(0.85, 0.30, 0.30, 0.4),
            stroke: None,
            text: Color::rgba(1.0, 1.0, 1.0, 0.55),
        },
        radius: Corners::all(2.0),
    }
}
