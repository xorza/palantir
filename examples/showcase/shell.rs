//! The showcase chrome: a grouped nav rail and a titled page card, both
//! driven by [`PAGES`], the single source of truth for each page's label,
//! group, blurb, key hints, scrolling and builder.

use palantir::{
    Align, App, Axis, Button, ButtonTheme, Checkbox, Chrome, Configure, FocusPolicy, FontFamily,
    FontWeight, Justify, Key, Metrics, Panel, Scroll, Separator, Shortcut, Sizing, SlotDefaults,
    Spacing, StatefulLook, Text, Theme, Tooltip, Ui, UserScale, VAlign, Vsync, WindowConfig,
    WindowToken, fmt,
};

use crate::pages;
use crate::pages::state::AppState;
use crate::scaffold::header::Header;
use crate::scaffold::links::LINKS;
use crate::shot::ShotApp;
use crate::support::{Kit, Kits, PAGE_GAP};

pub(crate) const MAIN_WINDOW: WindowToken = WindowToken(0);
/// Optional second window mirroring the `state` page's counter.
pub(crate) const INSPECTOR_WINDOW: WindowToken = WindowToken(1);

/// Open the inspector, or close it if up; the live window set decides.
pub(crate) fn toggle_inspector(ui: &mut Ui) {
    if ui.is_window_open(INSPECTOR_WINDOW) {
        ui.close_window(INSPECTOR_WINDOW);
    } else {
        ui.open_window(INSPECTOR_WINDOW, WindowConfig::new("inspector"));
    }
}

const SIDEBAR_W: f32 = 208.0;
const M: Metrics = Metrics::DEFAULT;

/// A nav rail heading; pages of one group are adjacent in [`PAGES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Widgets,
    Input,
    Layout,
    Paint,
    Runtime,
    /// Regression content and the bench viewer, drawn in quieter ink.
    Diagnostics,
}

impl Group {
    const fn heading(self) -> &'static str {
        match self {
            Self::Widgets => "WIDGETS",
            Self::Input => "INPUT",
            Self::Layout => "LAYOUT",
            Self::Paint => "PAINT",
            Self::Runtime => "RUNTIME",
            Self::Diagnostics => "DIAGNOSTICS",
        }
    }

    const fn is_quiet(self) -> bool {
        matches!(self, Self::Diagnostics)
    }
}

/// How the shell hosts a page's body.
#[derive(Clone, Copy, Debug)]
enum Flow {
    /// Wrapped in a vertical `Scroll`, so it may exceed the window. The default.
    Scroll,
    /// Handed the card's leftover height, for pages whose demo is a viewport.
    Fill,
}

#[derive(Clone, Copy, Debug)]
struct Page {
    group: Group,
    label: &'static str,
    blurb: &'static str,
    /// Keys and gestures the page answers to, shown as keycaps.
    keys: &'static [&'static str],
    flow: Flow,
    /// The page's file, from the repository's root.
    source: &'static str,
    /// Every page takes the app state; pages that ignore it take `_`.
    build: fn(&mut Ui, &mut AppState, Kit),
}

/// A page's file, from the repository's root, spelled from its module, which the expansion names,
/// so a renamed module breaks the build rather than the header.
macro_rules! source {
    ($module:ident) => {{
        let _ = pages::$module::build;
        concat!("examples/showcase/pages/", stringify!($module), ".rs")
    }};
}

const PAGES: &[Page] = &[
    Page {
        group: Group::Widgets,
        label: "controls",
        blurb: "Form controls wired together, button themes, and disclosure sections.",
        keys: &["click", "drag", "Space", "←→"],
        flow: Flow::Scroll,
        source: source!(controls),
        build: |ui, _, k| pages::controls::build(ui, k),
    },
    Page {
        group: Group::Widgets,
        label: "colors",
        blurb: "The color picker, its parts on their own, and the chip that opens it.",
        keys: &["drag", "Esc"],
        flow: Flow::Scroll,
        source: source!(colors),
        build: |ui, _, k| pages::colors::build(ui, k),
    },
    Page {
        group: Group::Widgets,
        label: "tabs",
        blurb: "A page view bound to an index, the chip strip on its own, and overflow.",
        keys: &["←→", "Home", "End", "Ctrl+Tab", "wheel"],
        flow: Flow::Scroll,
        source: source!(tabs),
        build: |ui, _, k| pages::tabs::build(ui, k),
    },
    Page {
        group: Group::Widgets,
        label: "dock",
        blurb: "Tabbed panes that split, join and resize by drag.",
        keys: &["drag chip", "drag divider", "right-click"],
        flow: Flow::Fill,
        source: source!(dock),
        build: |ui, _, k| pages::dock::build(ui, k),
    },
    Page {
        group: Group::Widgets,
        label: "custom widget",
        blurb: "A widget written against the public authoring API and nothing else.",
        keys: &["click", "Tab", "↑↓"],
        flow: Flow::Scroll,
        source: source!(custom_widget),
        build: |ui, _, k| pages::custom_widget::build(ui, k),
    },
    Page {
        group: Group::Widgets,
        label: "dialogs",
        blurb: "Modal dialogs, and a window close the app can veto.",
        keys: &["Esc", "Tab", "close window"],
        flow: Flow::Scroll,
        source: source!(dialogs),
        build: |ui, _, k| pages::dialogs::build(ui, k),
    },
    Page {
        group: Group::Widgets,
        label: "overlays",
        blurb: "Popups, anchors, tooltips and context menus, above the main tree.",
        keys: &["click", "hover", "right-click", "Esc"],
        flow: Flow::Scroll,
        source: source!(overlays),
        build: |ui, _, k| pages::overlays::build(ui, k),
    },
    Page {
        group: Group::Input,
        label: "text input",
        blurb: "Single- and multi-line editors, the edges they report, and IME composition.",
        keys: &["type", "Enter", "Esc", "Shift+arrows", "IME"],
        flow: Flow::Scroll,
        source: source!(text_input),
        build: |ui, _, k| pages::text_input::build(ui, k),
    },
    Page {
        group: Group::Input,
        label: "focus & keyboard",
        blurb: "Tab traversal, the focus ring, arrow groups, and focus that stays inside an overlay.",
        keys: &["Tab", "Shift+Tab", "←→↑↓", "Enter", "Space", "Esc"],
        flow: Flow::Scroll,
        source: source!(focus),
        build: |ui, _, k| pages::focus::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "sizing & spacing",
        blurb: "Sizing, justification, alignment, padding, margin, gap and visibility.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(sizing),
        build: |ui, _, k| pages::sizing::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "containers",
        blurb: "Stacks, wrapping flow, and grid tracks.",
        keys: &["resize window"],
        flow: Flow::Scroll,
        source: source!(containers),
        build: |ui, _, k| pages::containers::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "text",
        blurb: "Wrapping, and text inside Hug and Fill layouts.",
        keys: &["resize window"],
        flow: Flow::Scroll,
        source: source!(text),
        build: |ui, _, k| pages::text::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "clip & transform",
        blurb: "Clip modes against an overflowing child, and transforms on whole subtrees.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(clip),
        build: |ui, _, k| pages::clip::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "scroll & split",
        blurb: "Scroll viewports on three axes, inside resizable splitter panes.",
        keys: &["wheel", "drag bar", "double-click bar"],
        flow: Flow::Fill,
        source: source!(scroll),
        build: |ui, _, k| pages::scroll::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "pan & zoom",
        blurb: "A zoomable viewport: the point under the cursor stays put.",
        keys: &["wheel", "Ctrl+wheel", "pinch"],
        flow: Flow::Fill,
        source: source!(pan_zoom),
        build: |ui, _, k| pages::pan_zoom::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "shapes",
        blurb: "SDF triangles, raw meshes, and the windowed rect.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(shapes),
        build: |ui, _, k| pages::shapes::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "strokes",
        blurb: "Lines, polylines, béziers and arcs: widths, joins, caps and color.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(strokes),
        build: |ui, _, k| pages::strokes::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "gradients",
        blurb: "Linear, radial and conic brushes, spread modes and interpolation spaces.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(gradients),
        build: |ui, _, k| pages::gradients::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "shadows",
        blurb: "Drop and inset shadows, as shapes and as widget chrome.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(shadows),
        build: |ui, _, k| pages::shadows::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "materials",
        blurb: "Backdrop filters: blur, frosted glass and glass, as shapes and as chrome.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(materials),
        build: |ui, _, k| pages::materials::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "icons",
        blurb: "SVG icons rasterized at the exact physical size they land on.",
        keys: &["Ctrl +", "Ctrl −"],
        flow: Flow::Scroll,
        source: source!(icons),
        build: |ui, _, k| pages::icons::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "images",
        blurb: "Fit modes, tint, tiling, and sampling under magnification and minification.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(images),
        build: |ui, _, k| pages::images::build(ui, k),
    },
    Page {
        group: Group::Runtime,
        label: "motion",
        blurb: "Easing curves, and drag handling with no tracking code.",
        keys: &["click", "drag"],
        flow: Flow::Scroll,
        source: source!(motion),
        build: |ui, _, k| pages::motion::build(ui, k),
    },
    Page {
        group: Group::Runtime,
        label: "gpu view",
        blurb: "Raw wgpu inside a widget, composited as an ordinary image.",
        keys: &["drag"],
        flow: Flow::Fill,
        source: source!(gpu_view),
        build: |ui, _, k| pages::gpu_view::build(ui, k),
    },
    Page {
        group: Group::Runtime,
        label: "state & windows",
        blurb: "App state passed through the record closure, shared with a second OS window.",
        keys: &["F8"],
        flow: Flow::Scroll,
        source: source!(state),
        build: |ui, app, k| pages::state::build(ui, k, app),
    },
    Page {
        group: Group::Diagnostics,
        label: "fixtures",
        blurb: "Deliberately ugly regression content: id collisions, z-order, alpha blending.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(fixtures),
        build: |ui, _, k| pages::fixtures::build(ui, k),
    },
    Page {
        group: Group::Diagnostics,
        label: "frame bench",
        blurb: "The workload the frame benchmark records, drawn live. Nothing animates.",
        keys: &[],
        flow: Flow::Fill,
        source: source!(frame_bench),
        build: |ui, _, _| pages::frame_bench::build(ui),
    },
];

#[derive(Debug)]
pub(crate) struct Shell {
    active: usize,
    app: AppState,
    kits: Kits,
}

impl Shell {
    /// The tour wears the stock theme the `Ui` starts with, in whichever scheme shows, so every
    /// page shows the default look.
    pub(crate) fn new() -> Self {
        Self {
            active: 0,
            app: AppState { counter: 0 },
            kits: Kits::new(),
        }
    }

    fn build(&mut self, ui: &mut Ui) {
        handle_shortcuts(ui);
        // The focus page sets its own policy; every other page gets the default.
        ui.set_focus_policy(FocusPolicy::default());
        let k = self.kits.kit(ui.color_scheme());

        Panel::hstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                self.rail(ui, k);
                self.card(ui, k);
            });

        pages::dialogs::intercept(ui, MAIN_WINDOW);
    }

    /// The nav rail: brand, grouped page list, debug-overlay footer. The list
    /// is an arrow group, so one Tab lands in it.
    fn rail(&mut self, ui: &mut Ui, k: Kit) {
        let idle = nav_style(ui.theme(), k, NavLook::Idle);
        let quiet = nav_style(ui.theme(), k, NavLook::Quiet);
        let selected = nav_style(ui.theme(), k, NavLook::Selected);
        Panel::vstack()
            .size((Sizing::fixed(SIDEBAR_W), Sizing::FILL))
            .padding((M.space.md, M.space.lg, M.space.md, M.space.md))
            .gap(M.space.md)
            .chrome(Chrome::fill(k.t.surround))
            .show(ui, |ui| {
                brand(ui, k);
                Scroll::vertical()
                    .size((Sizing::FILL, Sizing::FILL))
                    .overlay_bars()
                    .gap(2.0)
                    .arrow_focus(Axis::Y)
                    .show(ui, |ui| {
                        for (i, page) in PAGES.iter().enumerate() {
                            if i == 0 || PAGES[i - 1].group != page.group {
                                let top = if i == 0 { M.space.xs } else { M.space.lg };
                                group_heading(ui, k, page.group, top);
                            }
                            let style = if i == self.active {
                                &selected
                            } else if page.group.is_quiet() {
                                &quiet
                            } else {
                                &idle
                            };
                            let hit = Button::new()
                                .id_salt(page.label)
                                .label(page.label)
                                .style(style)
                                .text_align(Align::LEFT)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui)
                                .clicked();
                            if hit {
                                self.active = i;
                            }
                        }
                    });
                debug_toggles(ui);
            });
    }

    /// The page, on the window's ground: its header, a rule, then the active page's body.
    fn card(&mut self, ui: &mut Ui, k: Kit) {
        let page = PAGES[self.active];
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding((M.space.xl, M.space.xl, M.space.sm, M.space.lg))
            .gap(M.space.xl)
            .show(ui, |ui| {
                page_header(ui, k, &page);
                Separator::horizontal().show(ui);
                // Keyed by page so scroll offsets don't carry over.
                match page.flow {
                    Flow::Scroll => {
                        Scroll::vertical()
                            .id_salt(page.label)
                            .size((Sizing::FILL, Sizing::FILL))
                            .overlay_bars()
                            .gap(PAGE_GAP)
                            // Keeps the overlay bar off the page's right edge.
                            .padding((0.0, 0.0, M.space.lg, 0.0))
                            .show(ui, |ui| (page.build)(ui, &mut self.app, k));
                    }
                    Flow::Fill => {
                        Panel::vstack()
                            .id_salt(page.label)
                            .size((Sizing::FILL, Sizing::FILL))
                            .padding((0.0, 0.0, M.space.lg, 0.0))
                            .gap(PAGE_GAP)
                            .show(ui, |ui| (page.build)(ui, &mut self.app, k));
                    }
                }
            });
    }
}

impl ShotApp for Shell {
    fn page_labels() -> impl Iterator<Item = &'static str> {
        PAGES.iter().map(|page| page.label)
    }

    fn open(page: usize, _ui: &mut Ui) -> Self {
        Self {
            active: page,
            ..Self::new()
        }
    }
}

impl App for Shell {
    fn record(&mut self, win: WindowToken, ui: &mut Ui) {
        match win {
            INSPECTOR_WINDOW => {
                let k = self.kits.kit(ui.color_scheme());
                Panel::vstack()
                    .padding(M.space.lg)
                    .gap(M.space.md)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| pages::state::counter(ui, k, &mut self.app));
            }
            _ => self.build(ui),
        }
    }
}

fn brand(ui: &mut Ui, k: Kit) {
    Panel::vstack()
        .size((Sizing::FILL, Sizing::HUG))
        .padding((M.space.md, 0.0, 0.0, 0.0))
        .gap(2.0)
        .show(ui, |ui| {
            Text::new("palantir")
                .style(
                    &k.t.body()
                        .with_font_size(M.text.title)
                        .with_weight(FontWeight::BOLD),
                )
                .show(ui);
            Text::new("widget tour").style(&k.t.caption()).show(ui);
        });
}

/// `top`: the margin above the heading. A quiet group's heading is a step fainter still.
fn group_heading(ui: &mut Ui, k: Kit, group: Group, top: f32) {
    let mut style = k.t.heading();
    if group.is_quiet() {
        style = style.with_color(style.color.with_alpha(0.6));
    }
    Text::new(group.heading())
        .id_salt(group.heading())
        .style(&style)
        .margin((M.space.md, top, 0.0, M.space.xs))
        .show(ui);
}

/// The rail footer; each control mirrors its shortcut.
fn debug_toggles(ui: &mut Ui) {
    Separator::horizontal().show(ui);
    let mut overlay = ui.debug_overlay();
    let mut vsync_on = ui.vsync() == Vsync::On;
    Panel::vstack()
        .size((Sizing::FILL, Sizing::HUG))
        .padding((M.space.md, 0.0, 0.0, 0.0))
        .gap(M.space.sm)
        .show(ui, |ui| {
            Checkbox::new(&mut overlay.damage_rect)
                .label("damage rects  F12")
                .show(ui);
            Checkbox::new(&mut overlay.dim_undamaged)
                .label("dim undamaged  F10")
                .show(ui);
            Checkbox::new(&mut overlay.frame_stats)
                .label("frame stats  F9")
                .show(ui);
            // Turning vsync off shows in `frame stats` as the rate leaving the refresh cap.
            Checkbox::new(&mut vsync_on).label("vsync").show(ui);
            ui_scale_row(ui);
        });
    ui.set_vsync(if vsync_on { Vsync::On } else { Vsync::Off });
    ui.set_debug_overlay(overlay);
}

/// The UI-scale stepper: `−  100%  +`. The readout only reads: a
/// scrubbable value would re-lay-out under the pointer mid-drag.
fn ui_scale_row(ui: &mut Ui) {
    let scale = ui.user_scale();
    let bar = &ui.theme().menu_bar;
    let step = ButtonTheme {
        unselected: bar.title.clone(),
        selected: bar.title.clone(),
        defaults: bar.defaults,
    };

    let mut next = scale;
    Panel::hstack()
        .size((Sizing::FILL, Sizing::HUG))
        .margin(Spacing::new(0.0, M.space.xs, 0.0, 0.0))
        .justify(Justify::Center)
        .child_align(Align::v(VAlign::Center))
        .gap(M.space.xs)
        .show(ui, |ui| {
            if Button::new().style(&step).label("−").show(ui).clicked() {
                next = scale.stepped_down();
            }
            // Fixed width, mono: the readout's width changes between 90% and 100%.
            Text::new(fmt!(ui, "{}%", scale.percent()))
                .family(FontFamily::MONO)
                .font_size(12.0)
                .size((Sizing::fixed(34.0), Sizing::HUG))
                .text_align(Align::CENTER)
                .show(ui);
            let up = Button::new().style(&step).label("+").show(ui);
            let clicked = up.clicked();
            let up = up.snapshot();
            if clicked {
                next = scale.stepped_up();
            }
            Tooltip::on(&up, "UI scale — ctrl +/− · ctrl 0 resets").show(ui);
        });
    ui.set_user_scale(next);
}

fn page_header(ui: &mut Ui, k: Kit, page: &Page) {
    Header {
        title: page.label,
        blurb: page.blurb,
        keys: page.keys,
        apis: &[],
        related_label: "observatory",
        related: LINKS
            .iter()
            .filter(|link| link.page == page.label)
            .map(|link| link.study),
        source: page.source,
    }
    .show(ui, k.t);
}

/// The three ways a rail item can look.
#[derive(Clone, Copy, Debug)]
enum NavLook {
    Idle,
    /// An item of a [`Group::is_quiet`] group.
    Quiet,
    /// The open page.
    Selected,
}

/// A rail item: the theme's menu-bar title, muted at rest and lit under the pointer, in the
/// accent while its page is open, on an `xs` vertical inset rather than the title's `sm`, so the
/// whole list fits a window.
fn nav_style(theme: &Theme, k: Kit, look: NavLook) -> ButtonTheme {
    let bar = &theme.menu_bar;
    let mut looks = bar.title.clone();
    match look {
        NavLook::Idle => {}
        NavLook::Quiet => looks.normal.text = looks.normal.text.with_color(k.t.ink_faint),
        NavLook::Selected => {
            let open = bar.open.clone();
            looks = StatefulLook {
                normal: open.clone(),
                hovered: open,
                active: looks.active,
                disabled: looks.disabled,
            };
            for look in [&mut looks.normal, &mut looks.hovered, &mut looks.active] {
                look.text = look.text.with_color(k.t.accent);
            }
        }
    }
    ButtonTheme {
        selected: looks.clone(),
        unselected: looks,
        defaults: SlotDefaults {
            padding: Spacing::xy(M.space.md, M.space.xs),
            ..bar.defaults
        },
    }
}

/// ⌘Q / Ctrl+Q quits (palantir drops winit's default macOS menu, and with it
/// the native ⌘Q). F8 mirrors the inspector button; F9 / F10 / F12 the rail's
/// overlay switches; Ctrl+`-` / `=` / `0` the UI-scale stepper.
fn handle_shortcuts(ui: &mut Ui) {
    if ui.shortcut_pressed(Shortcut::ctrl('Q')) {
        ui.close_window(MAIN_WINDOW);
    }
    if ui.shortcut_pressed(Key::F8) {
        toggle_inspector(ui);
    }
    // `^= pressed` toggles when the key fired.
    let mut overlay = ui.debug_overlay();
    overlay.damage_rect ^= ui.shortcut_pressed(Key::F12);
    overlay.dim_undamaged ^= ui.shortcut_pressed(Key::F10);
    overlay.frame_stats ^= ui.shortcut_pressed(Key::F9);
    ui.set_debug_overlay(overlay);

    // Browser bindings. Ctrl+plus is three presses by layout (`=` on US, `+` on
    // German, shifted `+`), and `Shortcut::matches` compares modifiers exactly,
    // so each needs its own binding.
    let mut scale = ui.user_scale();
    let up = ui.shortcut_pressed(Shortcut::ctrl('='))
        || ui.shortcut_pressed(Shortcut::ctrl('+'))
        || ui.shortcut_pressed(Shortcut::ctrl_shift('+'));
    if up {
        scale = scale.stepped_up();
    }
    if ui.shortcut_pressed(Shortcut::ctrl('-')) {
        scale = scale.stepped_down();
    }
    if ui.shortcut_pressed(Shortcut::ctrl('0')) {
        scale = UserScale::ONE;
    }
    ui.set_user_scale(scale);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::scaffold::links::LINKS;
    use crate::shell::PAGES;

    /// Every page the shared links table names is a page of the tour, and every page's source is
    /// a file of the repository, so neither header row names something that is gone.
    #[test]
    fn the_links_and_sources_name_real_pages_and_files() {
        for link in LINKS {
            assert!(
                PAGES.iter().any(|page| page.label == link.page),
                "{link:?} names no page"
            );
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for page in PAGES {
            assert!(root.join(page.source).is_file(), "{}", page.source);
        }
    }
}
