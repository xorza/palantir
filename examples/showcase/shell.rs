//! The showcase chrome: a grouped nav rail and a titled page card, both
//! driven by [`PAGES`], the single source of truth for each page's label,
//! group, blurb, key hints, scrolling and builder.

use palantir::SlotDefaults;
use palantir::{
    Align, AnimationSpec, App, Axis, Background, Block, Button, ButtonTheme, Checkbox, Configure,
    Corners, FocusPolicy, FontFamily, FontWeight, Justify, Key, Palette, Panel, RgbaF32, Scroll,
    Shortcut, Sizing, Spacing, StatefulLook, Stroke, Text, TextStyle, TextStyleOverrides, TextWrap,
    Theme, Tooltip, Ui, UserScale, VAlign, Vsync, WidgetLook, WindowConfig, WindowToken, fmt,
};

use crate::pages;
use crate::pages::state::AppState;
use crate::support;

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

const SIDEBAR_W: f32 = 196.0;

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
            Group::Widgets => "WIDGETS",
            Group::Input => "INPUT",
            Group::Layout => "LAYOUT",
            Group::Paint => "PAINT",
            Group::Runtime => "RUNTIME",
            Group::Diagnostics => "DIAGNOSTICS",
        }
    }

    const fn is_quiet(self) -> bool {
        matches!(self, Group::Diagnostics)
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
    /// Every page takes the app state; pages that ignore it take `_`.
    build: fn(&mut Ui, &mut AppState),
}

const PAGES: &[Page] = &[
    Page {
        group: Group::Widgets,
        label: "controls",
        blurb: "Form controls wired together, button themes, and disclosure sections.",
        keys: &["click", "drag", "Space", "←→"],
        flow: Flow::Scroll,
        build: |ui, _| pages::controls::build(ui),
    },
    Page {
        group: Group::Widgets,
        label: "colors",
        blurb: "The colour picker, its parts on their own, and the chip that opens it.",
        keys: &["drag", "Esc"],
        flow: Flow::Scroll,
        build: |ui, _| pages::colors::build(ui),
    },
    Page {
        group: Group::Widgets,
        label: "tabs",
        blurb: "A page view bound to an index, the chip strip on its own, and overflow.",
        keys: &["←→", "Home", "End", "Ctrl+Tab", "wheel"],
        flow: Flow::Scroll,
        build: |ui, _| pages::tabs::build(ui),
    },
    Page {
        group: Group::Widgets,
        label: "dock",
        blurb: "Tabbed panes that split, join and resize by drag.",
        keys: &["drag chip", "drag divider", "right-click"],
        flow: Flow::Fill,
        build: |ui, _| pages::dock::build(ui),
    },
    Page {
        group: Group::Widgets,
        label: "custom widget",
        blurb: "A widget written against the public authoring API and nothing else.",
        keys: &["click", "Tab", "↑↓"],
        flow: Flow::Scroll,
        build: |ui, _| pages::custom_widget::build(ui),
    },
    Page {
        group: Group::Widgets,
        label: "dialogs",
        blurb: "Modal dialogs, and a window close the app can veto.",
        keys: &["Esc", "Tab", "close window"],
        flow: Flow::Scroll,
        build: |ui, _| pages::dialogs::build(ui),
    },
    Page {
        group: Group::Widgets,
        label: "overlays",
        blurb: "Popups, anchors, tooltips and context menus, above the main tree.",
        keys: &["click", "hover", "right-click", "Esc"],
        flow: Flow::Scroll,
        build: |ui, _| pages::overlays::build(ui),
    },
    Page {
        group: Group::Input,
        label: "text input",
        blurb: "Single- and multi-line editors, the edges they report, and IME composition.",
        keys: &["type", "Enter", "Esc", "Shift+arrows", "IME"],
        flow: Flow::Scroll,
        build: |ui, _| pages::text_input::build(ui),
    },
    Page {
        group: Group::Input,
        label: "focus & keyboard",
        blurb: "Tab traversal, the focus ring, arrow groups, and focus that stays inside an overlay.",
        keys: &["Tab", "Shift+Tab", "←→↑↓", "Enter", "Space", "Esc"],
        flow: Flow::Scroll,
        build: |ui, _| pages::focus::build(ui),
    },
    Page {
        group: Group::Layout,
        label: "sizing & spacing",
        blurb: "Sizing, justification, alignment, padding, margin, gap and visibility.",
        keys: &[],
        flow: Flow::Scroll,
        build: |ui, _| pages::sizing::build(ui),
    },
    Page {
        group: Group::Layout,
        label: "containers",
        blurb: "Stacks, wrapping flow, and grid tracks.",
        keys: &["resize window"],
        flow: Flow::Scroll,
        build: |ui, _| pages::containers::build(ui),
    },
    Page {
        group: Group::Layout,
        label: "text",
        blurb: "Wrapping, and text inside Hug and Fill layouts.",
        keys: &["resize window"],
        flow: Flow::Scroll,
        build: |ui, _| pages::text::build(ui),
    },
    Page {
        group: Group::Layout,
        label: "clip & transform",
        blurb: "Clip modes against an overflowing child, and transforms on whole subtrees.",
        keys: &[],
        flow: Flow::Scroll,
        build: |ui, _| pages::clip::build(ui),
    },
    Page {
        group: Group::Layout,
        label: "scroll & split",
        blurb: "Scroll viewports on three axes, inside resizable splitter panes.",
        keys: &["wheel", "drag bar", "double-click bar"],
        flow: Flow::Fill,
        build: |ui, _| pages::scroll::build(ui),
    },
    Page {
        group: Group::Layout,
        label: "pan & zoom",
        blurb: "A zoomable viewport: the point under the cursor stays put.",
        keys: &["wheel", "Ctrl+wheel", "pinch"],
        flow: Flow::Fill,
        build: |ui, _| pages::pan_zoom::build(ui),
    },
    Page {
        group: Group::Paint,
        label: "shapes",
        blurb: "SDF triangles, raw meshes, and the windowed rect.",
        keys: &[],
        flow: Flow::Scroll,
        build: |ui, _| pages::shapes::build(ui),
    },
    Page {
        group: Group::Paint,
        label: "strokes",
        blurb: "Lines, polylines, béziers and arcs: widths, joins, caps and colour.",
        keys: &[],
        flow: Flow::Scroll,
        build: |ui, _| pages::strokes::build(ui),
    },
    Page {
        group: Group::Paint,
        label: "gradients",
        blurb: "Linear, radial and conic brushes, spread modes and interpolation spaces.",
        keys: &[],
        flow: Flow::Scroll,
        build: |ui, _| pages::gradients::build(ui),
    },
    Page {
        group: Group::Paint,
        label: "shadows",
        blurb: "Drop and inset shadows, as shapes and as widget chrome.",
        keys: &[],
        flow: Flow::Scroll,
        build: |ui, _| pages::shadows::build(ui),
    },
    Page {
        group: Group::Paint,
        label: "icons",
        blurb: "SVG icons rasterized at the exact physical size they land on.",
        keys: &["Ctrl +", "Ctrl −"],
        flow: Flow::Scroll,
        build: |ui, _| pages::icons::build(ui),
    },
    Page {
        group: Group::Paint,
        label: "images",
        blurb: "Fit modes, tint, tiling, and sampling under magnification and minification.",
        keys: &[],
        flow: Flow::Scroll,
        build: |ui, _| pages::images::build(ui),
    },
    Page {
        group: Group::Runtime,
        label: "motion",
        blurb: "Easing curves, and drag handling with no tracking code.",
        keys: &["click", "drag"],
        flow: Flow::Scroll,
        build: |ui, _| pages::motion::build(ui),
    },
    Page {
        group: Group::Runtime,
        label: "gpu view",
        blurb: "Raw wgpu inside a widget, composited as an ordinary image.",
        keys: &["drag"],
        flow: Flow::Fill,
        build: |ui, _| pages::gpu_view::build(ui),
    },
    Page {
        group: Group::Runtime,
        label: "state & windows",
        blurb: "App state passed through the record closure, shared with a second OS window.",
        keys: &["F8"],
        flow: Flow::Scroll,
        build: pages::state::build,
    },
    Page {
        group: Group::Diagnostics,
        label: "fixtures",
        blurb: "Deliberately ugly regression content: id collisions, z-order, alpha blending.",
        keys: &[],
        flow: Flow::Scroll,
        build: |ui, _| pages::fixtures::build(ui),
    },
    Page {
        group: Group::Diagnostics,
        label: "frame bench",
        blurb: "The workload the frame benchmark records, drawn live. Nothing animates.",
        keys: &[],
        flow: Flow::Fill,
        build: |ui, _| pages::frame_bench::build(ui),
    },
];

#[derive(Debug)]
pub(crate) struct State {
    active: usize,
    app: AppState,
}

impl State {
    pub(crate) fn new(ui: &mut Ui) -> Self {
        let mut theme = Theme::from_palette(&showcase_palette());
        // The library default is no button animation; the showcase demos it.
        theme.button.defaults.animation = Some(AnimationSpec::SPRING);
        ui.set_theme(theme);
        State {
            active: 0,
            app: AppState { counter: 0 },
        }
    }

    fn build(&mut self, ui: &mut Ui) {
        handle_shortcuts(ui);
        // The focus page sets its own policy; every other page gets the default.
        ui.set_focus_policy(FocusPolicy::default());

        Panel::hstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                self.rail(ui);
                Block::new()
                    .size((Sizing::fixed(1.0), Sizing::FILL))
                    .background(Background::fill(support::HAIRLINE))
                    .show(ui);
                self.card(ui);
            });

        pages::dialogs::intercept(ui, MAIN_WINDOW);
    }

    /// The nav rail: brand, grouped page list, debug-overlay footer. The list
    /// is an arrow group, so one Tab lands in it.
    fn rail(&mut self, ui: &mut Ui) {
        let idle = nav_style(NavLook::Idle);
        let quiet = nav_style(NavLook::Quiet);
        let selected = nav_style(NavLook::Selected);
        Panel::vstack()
            .size((Sizing::fixed(SIDEBAR_W), Sizing::FILL))
            .padding((12.0, 16.0, 12.0, 12.0))
            .gap(14.0)
            .background(Background::fill(support::SIDEBAR))
            .show(ui, |ui| {
                brand(ui);
                Scroll::vertical()
                    .size((Sizing::FILL, Sizing::FILL))
                    .overlay_bars()
                    .gap(2.0)
                    .arrow_focus(Axis::Y)
                    .show(ui, |ui| {
                        for (i, page) in PAGES.iter().enumerate() {
                            if i == 0 || PAGES[i - 1].group != page.group {
                                group_heading(ui, page.group, i == 0);
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

    /// The page card: title, blurb, key hints, rule, then the active page's body.
    fn card(&mut self, ui: &mut Ui) {
        let page = PAGES[self.active];
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(16.0)
            .show(ui, |ui| {
                Panel::vstack()
                    .size((Sizing::FILL, Sizing::FILL))
                    .padding(24.0)
                    .gap(20.0)
                    .background(
                        Background::rounded(support::CARD, Corners::all(10.0))
                            .with_border(Stroke::new(support::BORDER, 1.0)),
                    )
                    .clip_rounded()
                    .show(ui, |ui| {
                        page_header(ui, &page);
                        // Keyed by page so scroll offsets don't carry over.
                        match page.flow {
                            Flow::Scroll => {
                                Scroll::vertical()
                                    .id_salt(page.label)
                                    .size((Sizing::FILL, Sizing::FILL))
                                    .overlay_bars()
                                    .gap(support::PAGE_GAP)
                                    // Keeps the overlay bar off the page's right edge.
                                    .padding((0.0, 0.0, 14.0, 0.0))
                                    .show(ui, |ui| (page.build)(ui, &mut self.app));
                            }
                            Flow::Fill => {
                                Panel::vstack()
                                    .id_salt(page.label)
                                    .size((Sizing::FILL, Sizing::FILL))
                                    .gap(support::PAGE_GAP)
                                    .show(ui, |ui| (page.build)(ui, &mut self.app));
                            }
                        }
                    });
            });
    }
}

impl App for State {
    fn record(&mut self, win: WindowToken, ui: &mut Ui) {
        match win {
            INSPECTOR_WINDOW => {
                Panel::vstack()
                    .padding(16.0)
                    .gap(12.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| pages::state::counter(ui, &mut self.app));
            }
            _ => self.build(ui),
        }
    }
}

fn brand(ui: &mut Ui) {
    Panel::vstack()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(1.0)
        .show(ui, |ui| {
            Text::new("palantir")
                .style(
                    &TextStyle::default()
                        .with_font_size(17.0)
                        .with_weight(FontWeight::BOLD)
                        .with_color(support::INK),
                )
                .show(ui);
            Text::new("widget tour")
                .style(
                    &TextStyle::default()
                        .with_font_size(11.0)
                        .with_color(support::INK_FAINT),
                )
                .show(ui);
        });
}

fn group_heading(ui: &mut Ui, group: Group, first: bool) {
    let ink = if group.is_quiet() {
        support::INK_DISABLED
    } else {
        support::INK_FAINT
    };
    Text::new(group.heading())
        .id_salt(group.heading())
        .style(
            &TextStyle::default()
                .with_font_size(10.0)
                .with_weight(FontWeight::BOLD)
                .with_color(ink),
        )
        .margin((10.0, if first { 2.0 } else { 16.0 }, 0.0, 4.0))
        .show(ui);
}

/// The rail footer; each control mirrors its shortcut.
fn debug_toggles(ui: &mut Ui) {
    Block::new()
        .size((Sizing::FILL, Sizing::fixed(1.0)))
        .background(Background::fill(support::HAIRLINE))
        .show(ui);
    let mut overlay = ui.debug_overlay();
    let mut vsync_on = ui.vsync() == Vsync::On;
    Panel::vstack()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(6.0)
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
    let step = scale_step_style();

    let mut next = scale;
    Panel::hstack()
        .size((Sizing::FILL, Sizing::HUG))
        .margin(Spacing::new(0.0, 6.0, 0.0, 0.0))
        .justify(Justify::Center)
        .child_align(Align::v(VAlign::Center))
        .gap(4.0)
        .show(ui, |ui| {
            if Button::new().style(&step).label("−").show(ui).clicked() {
                next = scale.stepped_down();
            }
            // Fixed width, mono: the readout's width changes between 90% and 100%.
            Text::new(fmt!(ui, "{}%", scale.percent()))
                .family(FontFamily::MONO)
                .font_size(12.0)
                .color(support::INK)
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

/// The rail's flat button with a bigger glyph; `−` and `+` read as specks at
/// label size.
fn scale_step_style() -> ButtonTheme {
    let mut style = nav_style(NavLook::Idle);
    let grow = |look: &mut WidgetLook| {
        look.text.font_size = Some(15.0);
    };
    grow(&mut style.looks.normal);
    grow(&mut style.looks.hovered);
    grow(&mut style.looks.active);
    grow(&mut style.looks.disabled);
    style
}

fn page_header(ui: &mut Ui, page: &Page) {
    Panel::vstack()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(6.0)
        .show(ui, |ui| {
            Text::new(page.label)
                .style(&support::title_style())
                .show(ui);
            Text::new(page.blurb)
                .style(&support::blurb_style())
                .size((Sizing::FILL, Sizing::HUG))
                .text_wrap(TextWrap::WrapWithOverflow)
                .show(ui);
            if !page.keys.is_empty() {
                support::keys(ui, page.keys);
            }
            Block::new()
                .size((Sizing::FILL, Sizing::fixed(1.0)))
                .margin((0.0, 10.0, 0.0, 0.0))
                .background(Background::fill(support::HAIRLINE))
                .show(ui);
        });
}

/// Cool-neutral recolor of the stock palette for chrome and surfaces.
pub(crate) const fn showcase_palette() -> Palette {
    Palette {
        text: support::INK,
        text_muted: support::INK_DIM,
        text_disabled: support::INK_DISABLED,
        window_background: support::WINDOW,
        element: support::ELEMENT,
        element_mid: support::ELEM_MID,
        element_strong: support::ELEM_STRONG,
        border_focused: support::BORDER_FOCUSED,
        accent: support::ACCENT,
    }
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

/// Flat rail button: transparent at rest, accent-washed when open. Used by
/// nav items and the UI-scale stepper.
fn nav_style(look: NavLook) -> ButtonTheme {
    let label = |c: RgbaF32| TextStyleOverrides::NONE.with_font_size(12.0).with_color(c);
    let wash = |alpha: f32, c: RgbaF32| Background::rounded(c.with_alpha(alpha), Corners::all(5.0));
    let (rest, hover, press, ink, tint) = match look {
        NavLook::Idle => (0.0, 0.06, 0.10, support::INK_DIM, RgbaF32::WHITE),
        NavLook::Quiet => (0.0, 0.06, 0.10, support::INK_FAINT, RgbaF32::WHITE),
        NavLook::Selected => (0.16, 0.22, 0.28, support::ACCENT, support::ACCENT),
    };
    ButtonTheme {
        looks: StatefulLook {
            normal: WidgetLook {
                background: wash(rest, tint),
                text: label(ink),
            },
            hovered: WidgetLook {
                background: wash(hover, tint),
                text: label(support::INK),
            },
            active: WidgetLook {
                background: wash(press, tint),
                text: label(support::INK),
            },
            disabled: WidgetLook {
                background: Background::NONE,
                text: label(support::INK_FAINT),
            },
        },
        defaults: SlotDefaults {
            padding: Spacing::xy(10.0, 5.0),
            margin: Spacing::ZERO,
            animation: Some(AnimationSpec::FAST),
        },
    }
}

/// ⌘Q / Ctrl+Q quits (palantir drops winit's default macOS menu, and with it
/// the native ⌘Q). F8 mirrors the inspector button; F9 / F10 / F12 the rail's
/// overlay switches; Ctrl+`-` / `=` / `0` the UI-scale stepper.
fn handle_shortcuts(ui: &mut Ui) {
    if ui.key_pressed(Shortcut::ctrl('Q')) {
        ui.close_window(MAIN_WINDOW);
    }
    if ui.key_pressed(Shortcut::key(Key::F8)) {
        toggle_inspector(ui);
    }
    // `^= pressed` toggles when the key fired.
    let mut overlay = ui.debug_overlay();
    overlay.damage_rect ^= ui.key_pressed(Shortcut::key(Key::F12));
    overlay.dim_undamaged ^= ui.key_pressed(Shortcut::key(Key::F10));
    overlay.frame_stats ^= ui.key_pressed(Shortcut::key(Key::F9));
    ui.set_debug_overlay(overlay);

    // Browser bindings. Ctrl+plus is three presses by layout (`=` on US, `+` on
    // German, shifted `+`), and `Shortcut::matches` compares modifiers exactly,
    // so each needs its own binding.
    let mut scale = ui.user_scale();
    let up = ui.key_pressed(Shortcut::ctrl('='))
        || ui.key_pressed(Shortcut::ctrl('+'))
        || ui.key_pressed(Shortcut::ctrl_shift('+'));
    if up {
        scale = scale.stepped_up();
    }
    if ui.key_pressed(Shortcut::ctrl('-')) {
        scale = scale.stepped_down();
    }
    if ui.key_pressed(Shortcut::ctrl('0')) {
        scale = UserScale::ONE;
    }
    ui.set_user_scale(scale);
}
