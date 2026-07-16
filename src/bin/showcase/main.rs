use aperture::{
    AnimSpec, App, Background, Button, Color, Configure, HostHandle, Key, Panel, Shortcut, Sizing,
    Ui, WindowConfig, WindowToken, WinitHost,
};
use std::cell::RefCell;
use std::rc::Rc;

mod animations;
mod app_state;
mod clip;
mod controls;
mod cube;
mod debug;
mod dialogs;
mod drag;
mod gradients;
mod image;
mod layout;
mod lines;
mod overlays;
mod pan_zoom;
mod panels;
mod scroll;
mod shadow;
mod shapes;
mod support;
mod text;
mod text_edit;

use app_state::AppState;

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
    /// Persistent renderer for the `cube` page — its GPU resources build
    /// lazily on first paint (no device at construction).
    cube: Rc<RefCell<cube::Cube>>,
}

/// Each non-stateful showcase: a label for the toolbar button, and a
/// builder that fills the central panel. Adding a new showcase = one
/// line here + one new module. The `app_state` page is dispatched
/// separately so it can receive `&mut AppState`.
type ShowcaseFn = fn(&mut Ui);

const APP_STATE_LABEL: &str = "app state";
/// Dispatched separately so it can receive the persistent `Cube` renderer.
const CUBE_LABEL: &str = "cube";

const SHOWCASES: &[(&str, ShowcaseFn)] = &[
    ("controls", controls::build),
    ("text", text::build),
    ("text edit", text_edit::build),
    ("layout", layout::build),
    ("panels & grid", panels::build),
    ("clip & transform", clip::build),
    ("scroll & split", scroll::build),
    ("pan + zoom", pan_zoom::build),
    ("overlays", overlays::build),
    ("dialogs", dialogs::build),
    ("animations", animations::build),
    ("drag", drag::build),
    ("gradients", gradients::build),
    ("shadows", shadow::build),
    ("lines & curves", lines::build),
    ("shapes", shapes::build),
    ("image", image::build),
    (CUBE_LABEL, |_ui| {}),
    (APP_STATE_LABEL, |_ui| {}),
    ("debug", debug::build),
];

fn main() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    WinitHost::builder(MAIN_WINDOW)
        .title("aperture showcase")
        .build(State::new)
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
            cube: Rc::new(RefCell::new(cube::Cube::new())),
        }
    }
}

impl App for State {
    fn record(&mut self, win: WindowToken, ui: &mut Ui) {
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
    // ⌘Q / Ctrl+Q quits — aperture drops winit's default macOS menu (so its
    // Quit item can't hard-terminate past a close-request veto), which also
    // removes the native ⌘Q, so wire it here.
    if ui.key_pressed(Shortcut::ctrl('Q')) {
        ui.close_window(MAIN_WINDOW);
    }
    // F8 toggles a second OS window mirroring the counter page. The live
    // window set is the source of truth (`Ui::window_open`), so closing
    // the inspector via its titlebar X stays in sync — the next F8
    // reopens it with no stale bool to track.
    if ui.key_pressed(Shortcut::key(Key::F8)) {
        if ui.window_open(INSPECTOR_WINDOW) {
            ui.close_window(INSPECTOR_WINDOW);
        } else {
            ui.open_window(INSPECTOR_WINDOW, WindowConfig::new("inspector"));
        }
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
                        if btn.show(ui).left.clicked() {
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
                .background(
                    Background::rounded(Color::hex(0x343434), aperture::Corners::all(8.0))
                        .with_stroke(aperture::Stroke::solid(Color::hex(0x363636), 1.0)),
                )
                .show(ui, |ui| {
                    let (label, build_fn) = SHOWCASES[state.active];
                    if label == APP_STATE_LABEL {
                        app_state::build(ui, &mut state.app);
                    } else if label == CUBE_LABEL {
                        cube::build(ui, &state.cube);
                    } else {
                        build_fn(ui);
                    }
                });
        });

    // Catch the window's close request: with "unsaved changes" toggled on
    // (the `dialogs` tab), veto it and prompt instead of quitting.
    dialogs::intercept(ui, MAIN_WINDOW);
}

/// Build a one-off ButtonTheme that highlights the active toolbar
/// entry: copy the default theme, swap the `normal` slot to use the
/// `hovered` background. Pressed / disabled / hovered fall through to
/// the defaults.
fn active_toolbar_button(default: &aperture::ButtonTheme) -> aperture::ButtonTheme {
    aperture::ButtonTheme {
        looks: aperture::StatefulLook {
            normal: default.looks.hovered.clone(),
            ..default.looks.clone()
        },
        ..default.clone()
    }
}

/// F12 toggles damage-rect outlines; F10 toggles darken-undamaged;
/// F9 toggles the frame/FPS readout. The overlay is app-global, so
/// toggling from whichever window has focus updates every window. Only
/// `build_ui` needs to call this — the inspector inherits the same
/// config. `key_pressed` auto-subscribes so off-focus presses still wake
/// the loop.
fn handle_debug_keys(ui: &mut Ui) {
    if ui.key_pressed(Shortcut::key(Key::F12)) {
        let mut o = ui.debug_overlay_mut();
        o.damage_rect = !o.damage_rect;
        eprintln!(
            "[F12] damage rect overlay: {}",
            if o.damage_rect { "on" } else { "off" }
        );
    }
    if ui.key_pressed(Shortcut::key(Key::F10)) {
        let mut o = ui.debug_overlay_mut();
        o.dim_undamaged = !o.dim_undamaged;
        eprintln!("[F10] darken undamaged: {}", o.dim_undamaged);
    }
    if ui.key_pressed(Shortcut::key(Key::F9)) {
        let mut o = ui.debug_overlay_mut();
        o.frame_stats = !o.frame_stats;
        eprintln!("[F9] frame stats: {}", o.frame_stats);
    }
}
