use palantir::{
    AnimSpec, App, Background, Button, Color, Configure, HostHandle, Key, Panel, Shadow, Shortcut,
    Sizing, Ui, WindowConfig, WindowToken, WinitHost, WinitHostConfig,
};

mod showcase;
use showcase::app_state::{self, AppState};
use showcase::{
    alignment, animations, bezier, buttons, checkbox, clip, context_menu, dialogs, disabled, drag,
    gap, gradients, grid, id_collisions, image, justify, lines, mesh, pan_zoom, pan_zoom_auto,
    panels, popup, progress, radio, rect_demo, scroll, shadow, sizing, slider, spacing, switch,
    text, text_edit, text_zorder, tooltips, transform, visibility, wrap,
};

/// Token for the bootstrap window (the showcase itself).
const MAIN_WINDOW: WindowToken = WindowToken(0);
/// Token for the optional secondary window (F8) that mirrors the
/// `app_state` counter page in its own OS window.
const INSPECTOR_WINDOW: WindowToken = WindowToken(1);

/// State the showcase binary carries across frames: which tab is
/// active, plus the counter the `app_state` page reads/writes.
struct State {
    active: usize,
    app: AppState,
    /// Whether the secondary inspector window is currently open. Toggled
    /// by F8 from the main window; cleared when that window is closed.
    inspector_open: bool,
}

/// Each non-stateful showcase: a label for the toolbar button, and a
/// builder that fills the central panel. Adding a new showcase = one
/// line here + one new module. The `app_state` page is dispatched
/// separately so it can receive `&mut AppState`.
type ShowcaseFn = fn(&mut Ui);

const APP_STATE_LABEL: &str = "app state";

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
    ("progress", progress::build),
    ("switch", switch::build),
    ("slider", slider::build),
    ("combo + modal", dialogs::build),
    ("popup", popup::build),
    ("tooltips", tooltips::build),
    ("context menu", context_menu::build),
    ("animations", animations::build),
    (APP_STATE_LABEL, |_ui| {}),
    ("mesh", mesh::build),
    ("image", image::build),
    ("lines", lines::build),
    ("bezier", bezier::build),
    ("drag", drag::build),
    ("gradients", gradients::build),
    ("shadow", shadow::build),
    ("id collisions", id_collisions::build),
    ("rect demo", rect_demo::build),
];

fn main() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    WinitHost::new(
        MAIN_WINDOW,
        WinitHostConfig::new("palantir showcase"),
        State::new,
    )
    .run();
}

impl State {
    fn new(ui: &mut Ui, _handle: HostHandle<Self>) -> Self {
        // Library default is no button animation (`anim = None`).
        // Showcase exists to demo the animation primitive — opt in.
        ui.theme.button.anim = Some(AnimSpec::SPRING);
        State {
            active: 0,
            app: AppState { counter: 0 },
            inspector_open: false,
        }
    }
}

impl App for State {
    fn frame(&mut self, win: WindowToken, ui: &mut Ui) {
        match win {
            INSPECTOR_WINDOW => build_inspector(ui, self),
            _ => build_ui(ui, self),
        }
    }
}

/// The secondary window's content: the same counter the `app_state` page
/// drives, in its own OS window. Demonstrates an independent UI tree
/// sharing app state across windows via `&mut State`.
fn build_inspector(ui: &mut Ui, state: &mut State) {
    Panel::vstack()
        .auto_id()
        .padding(16.0)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            app_state::build(ui, &mut state.app);
        });
}

fn build_ui(ui: &mut Ui, state: &mut State) {
    handle_debug_keys(ui);
    // F8 toggles a second OS window mirroring the counter page. If the
    // user closed it via its titlebar X, `inspector_open` is stale —
    // close_window then no-ops and the next F8 reopens.
    if ui.key_pressed(Shortcut::key(Key::F8)) {
        if state.inspector_open {
            ui.close_window(INSPECTOR_WINDOW);
        } else {
            ui.open_window(INSPECTOR_WINDOW, WindowConfig::new("inspector"));
        }
        state.inspector_open = !state.inspector_open;
    }
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
                        if i == state.active {
                            btn = btn.style(active_style.clone());
                        }
                        if btn.show(ui).clicked() {
                            state.active = i;
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
                    corners: palantir::Corners::all(8.0),
                    shadow: Shadow::NONE,
                })
                .show(ui, |ui| {
                    let (label, build_fn) = SHOWCASES[state.active];
                    if label == APP_STATE_LABEL {
                        app_state::build(ui, &mut state.app);
                    } else {
                        build_fn(ui);
                    }
                });
        });
}

/// Build a one-off ButtonTheme that highlights the active toolbar
/// entry: copy the default theme, swap the `normal` slot to use the
/// `hovered` background. Pressed / disabled / hovered fall through to
/// the defaults.
fn active_toolbar_button(default: &palantir::ButtonTheme) -> palantir::ButtonTheme {
    palantir::ButtonTheme {
        normal: default.hovered.clone(),
        ..default.clone()
    }
}

/// F12 toggles damage-rect outlines; F10 toggles darken-undamaged;
/// F9 toggles the frame/FPS readout. `key_pressed` auto-subscribes so
/// off-focus presses still wake the loop.
fn handle_debug_keys(ui: &mut Ui) {
    let toggle_damage = ui.key_pressed(Shortcut::key(Key::F12));
    let toggle_dim = ui.key_pressed(Shortcut::key(Key::F10));
    let toggle_stats = ui.key_pressed(Shortcut::key(Key::F9));
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
