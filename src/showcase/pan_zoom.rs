use palantir::{
    AnimSpec, Background, Button, ButtonTheme, Color, Configure, Corners, Panel, Scroll, Sizing,
    Stroke, TextStyle, Ui, WidgetLook,
};

/// `Scroll::both().with_zoom()` over a dense grid of buttons. Bare wheel pans;
/// `Ctrl/Cmd + wheel` zooms about the cursor; pinch zooms unconditionally.
/// Pin the cursor to a cell and scroll-zoom — the cell stays under the
/// cursor. Cells are buttons so hover / press / click input still works
/// correctly through the scroll viewport's transform; the hovered cell
/// brightens via the standard ButtonTheme animation path.
pub fn build(ui: &mut Ui) {
    let last_click_id = palantir::WidgetId::from_hash("pz-last-click");
    let mut clicked: Option<(u32, u32)> = *ui.state_mut::<Option<(u32, u32)>>(last_click_id);
    Panel::vstack()
        .auto_id()
        .gap(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            let header = match clicked {
                Some((r, c)) => format!(
                    "Pan + zoom — wheel pans, Ctrl/Cmd + wheel zooms about the cursor, \
                     pinch zooms on touchpad. Last click: r{r} c{c}."
                ),
                None => "Pan + zoom — wheel pans, Ctrl/Cmd + wheel zooms about the cursor, \
                     pinch zooms on touchpad. Click a cell to confirm hit-testing through \
                     the zoom transform."
                    .to_string(),
            };
            palantir::Text::new(header)
                .auto_id()
                .wrapping()
                .style(TextStyle::default().with_font_size(13.0))
                .show(ui);

            Scroll::both()
                .auto_id()
                .with_zoom()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::vstack().id_salt("pz-grid").gap(4.0).show(ui, |ui| {
                        for r in 0..24u32 {
                            Panel::hstack()
                                .id_salt(("pz-row", r))
                                .gap(4.0)
                                .show(ui, |ui| {
                                    for c in 0..24u32 {
                                        if cell(ui, r, c) {
                                            clicked = Some((r, c));
                                        }
                                    }
                                });
                        }
                    });
                });
        });
    *ui.state_mut::<Option<(u32, u32)>>(last_click_id) = clicked;
}

fn cell(ui: &mut Ui, r: u32, c: u32) -> bool {
    Button::new()
        .id_salt(("pz-cell", r, c))
        .label(format!("{r},{c}"))
        .size((Sizing::Fixed(56.0), Sizing::Fixed(40.0)))
        .padding((6.0, 4.0))
        .style(cell_theme(r, c))
        .show(ui)
        .clicked()
}

/// Per-cell ButtonTheme: normal = the cell's base color, hovered =
/// brightened, pressed = brightest with a focus stroke. Anim drives a
/// smooth fill transition on hover/press. Constructed per-frame —
/// cheap (a few struct copies) and keeps each cell visually distinct.
fn cell_theme(r: u32, c: u32) -> ButtonTheme {
    let base = cell_color(r, c);
    let bg = |fill: Color| -> Background {
        Background {
            fill: fill.into(),
            radius: Corners::all(3.0),
            ..Default::default()
        }
    };
    let pressed_bg = Background {
        fill: brighten(base, 0.3).into(),
        stroke: Stroke::solid(Color::hex(0xffffff), 1.0),
        radius: Corners::all(3.0),
        shadow: None,
    };
    let label_text = TextStyle::default()
        .with_font_size(11.0)
        .with_color(Color::hex(0x1a1a1a));
    ButtonTheme {
        normal: WidgetLook {
            background: Some(bg(base)),
            text: Some(label_text),
        },
        hovered: WidgetLook {
            background: Some(bg(brighten(base, 0.15))),
            text: Some(label_text),
        },
        pressed: WidgetLook {
            background: Some(pressed_bg),
            text: Some(label_text),
        },
        disabled: WidgetLook {
            background: Some(bg(base)),
            text: Some(label_text),
        },
        padding: palantir::Spacing::xy(6.0, 4.0),
        margin: palantir::Spacing::ZERO,
        anim: Some(AnimSpec::FAST),
    }
}

fn brighten(c: Color, t: f32) -> Color {
    Color::linear_rgba(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
        c.a,
    )
}

fn cell_color(r: u32, c: u32) -> Color {
    let tr = r as f32 / 24.0;
    let tc = c as f32 / 24.0;
    Color::rgb(
        0.30 + 0.55 * tc,
        0.55 - 0.25 * (tr - 0.5).abs(),
        0.85 - 0.55 * tr,
    )
}
