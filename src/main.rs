use palantir::{
    AnimSpec, Background, Button, Color, Configure, Key, Panel, Shadow, Shortcut, Sizing, Ui,
    WinitHost, WinitHostConfig,
};

mod showcase;
use showcase::app_state::AppState;
use showcase::{
    alignment, animations, app_state, bezier, buttons, checkbox, clip, context_menu, disabled,
    drag, gap, gradients, grid, id_collisions, image, justify, lines, mesh, pan_zoom,
    pan_zoom_auto, panels, popup, radio, scroll, shadow, sizing, spacing, text, text_edit,
    text_zorder, tooltips, transform, visibility, wrap,
};

/// Each showcase: a label for the toolbar button, and a builder that fills the
/// central panel. Adding a new showcase = one line here + one new module.
type ShowcaseFn = fn(&mut Ui<AppState>);

const SHOWCASES: &[(&str, ShowcaseFn)] = &[
    ("text", text::build),
    ("text layouts", text::build_layouts),
    ("text edit", text_edit::build),
    ("text edit align", text_edit::build_align),
    ("z-order", text_zorder::build),
    ("panels", panels::build),
    ("scroll", scroll::build),
    ("pan+zoom", pan_zoom::build),
    (pan_zoom_auto::NAME, pan_zoom_auto::build),
    ("wrap", wrap::build),
    ("grid", grid::build),
    ("sizing", sizing::build),
    ("alignment", alignment::build),
    ("justify", justify::build),
    ("clip", clip::build),
    ("transform", transform::build),
    ("visibility", visibility::build),
    ("disabled", disabled::build),
    ("gap", gap::build),
    ("spacing", spacing::build),
    ("buttons", buttons::build),
    ("checkbox", checkbox::build),
    ("radio", radio::build),
    ("popup", popup::build),
    ("tooltips", tooltips::build),
    ("context menu", context_menu::build),
    ("animations", animations::build),
    ("app state", app_state::build),
    ("mesh", mesh::build),
    ("image", image::build),
    ("lines", lines::build),
    ("bezier", bezier::build),
    ("drag", drag::build),
    ("gradients", gradients::build),
    ("shadow", shadow::build),
    ("id collisions", id_collisions::build),
];

fn main() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let mut active = 0usize;
    WinitHost::new(
        WinitHostConfig::new("palantir showcase"),
        AppState { counter: 0 },
        move |ui| build_ui(ui, &mut active),
    )
    .with_setup(|ui| {
        // Library default is no button animation (`anim = None`).
        // Showcase exists to demo the animation primitive — opt in.
        ui.theme.button.anim = Some(AnimSpec::SPRING);
    })
    .run();
}

fn build_ui(ui: &mut Ui<AppState>, active: &mut usize) {
    handle_debug_keys(ui);
    let active_style = active_toolbar_button(&ui.theme.button);
    Panel::vstack()
        .auto_id()
        .padding(12.0)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Toolbar: one button per showcase. WrapHStack so the buttons
            // wrap to a new row when the window is too narrow to fit them
            // all on one line. Active button is rendered with the
            // hovered-state fill so it reads as "selected" — minimal
            // override on top of the default theme.
            Panel::wrap_hstack()
                .auto_id()
                .gap(6.0)
                .line_gap(6.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    for (i, (label, _)) in SHOWCASES.iter().enumerate() {
                        let mut btn = Button::new().id_salt(*label).label(*label);
                        if i == *active {
                            btn = btn.style(active_style.clone());
                        }
                        if btn.show(ui).clicked() {
                            *active = i;
                        }
                    }
                });

            // Central panel: hosts the selected showcase. Uses palette
            // `surface` + `border` so the showcase cards sit visually
            // contained against the window's `bg`.
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(16.0)
                .background(Background {
                    fill: Color::hex(0x343434).into(),
                    stroke: palantir::Stroke::solid(Color::hex(0x363636), 1.0),
                    radius: palantir::Corners::all(8.0),
                    shadow: Shadow::NONE,
                })
                .show(ui, |ui| {
                    let (_, build_fn) = SHOWCASES[*active];
                    build_fn(ui);
                });
        });
}

/// Build a one-off ButtonTheme that highlights the active toolbar
/// entry: copy the default theme, swap the `normal` slot to use the
/// `hovered` background. Pressed / disabled / hovered fall through to
/// the defaults.
fn active_toolbar_button(default: &palantir::ButtonTheme) -> palantir::ButtonTheme {
    palantir::ButtonTheme {
        normal: default.hovered,
        ..default.clone()
    }
}

/// F12 toggles damage-rect outlines; F10 toggles darken-undamaged;
/// F9 toggles the frame/FPS readout. Subscribes via the canonical
/// `Ui::subscribe_key` so off-focus presses still wake the loop.
fn handle_debug_keys(ui: &mut Ui<AppState>) {
    let f12 = Shortcut::key(Key::F12);
    let f10 = Shortcut::key(Key::F10);
    let f9 = Shortcut::key(Key::F9);
    ui.subscribe_key(f12);
    ui.subscribe_key(f10);
    ui.subscribe_key(f9);
    let toggle_damage = ui.key_pressed(f12);
    let toggle_dim = ui.key_pressed(f10);
    let toggle_stats = ui.key_pressed(f9);
    let o = &mut ui.debug_overlay;
    if toggle_damage {
        o.damage_rect = !o.damage_rect;
        eprintln!(
            "[F12] damage rect overlay: {}",
            if o.damage_rect { "on" } else { "off" }
        );
    }
    if toggle_dim {
        o.dim_undamaged = !o.dim_undamaged;
        eprintln!("[F10] darken undamaged: {}", o.dim_undamaged);
    }
    if toggle_stats {
        o.frame_stats = !o.frame_stats;
        eprintln!("[F9] frame stats: {}", o.frame_stats);
    }
}
