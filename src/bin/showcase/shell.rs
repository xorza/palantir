//! The showcase chrome: a grouped nav rail on the left, a titled page
//! card on the right, and the table that drives both.
//!
//! [`PAGES`] is the single source of truth — nav label, group heading,
//! the blurb rendered under the page title, whether the page scrolls,
//! and how its body is built. Adding a page is one row here plus one
//! module under `pages/`; nothing else in the shell knows page names.

use palantir::SlotDefaults;
use palantir::{
    Align, AnimSpec, App, Background, Button, ButtonTheme, Checkbox, Color, Configure, Corners,
    FontWeight, Frame, FrameFixture, Key, Palette, Panel, Scroll, Shortcut, Sizing, Spacing,
    StatefulLook, Stroke, Text, TextStyle, TextWrap, Theme, Ui, UserScale, Vsync, WidgetLook,
    WindowConfig, WindowToken, fmt,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::pages;
use crate::support;

/// Token for the bootstrap window (the showcase itself).
pub(crate) const MAIN_WINDOW: WindowToken = WindowToken(0);
/// Token for the optional secondary window that mirrors the counter
/// from the `state` page in its own OS window.
pub(crate) const INSPECTOR_WINDOW: WindowToken = WindowToken(1);

/// Open the inspector, or close it if it is already up.
///
/// The live window set is the source of truth for which of the two this
/// is, so both the F8 shortcut and the `state` page's button ask it here
/// rather than each tracking a bool of its own.
pub(crate) fn toggle_inspector(ui: &mut Ui) {
    if ui.window_open(INSPECTOR_WINDOW) {
        ui.close_window(INSPECTOR_WINDOW);
    } else {
        ui.open_window(INSPECTOR_WINDOW, WindowConfig::new("inspector"));
    }
}

const SIDEBAR_W: f32 = 196.0;

/// How the shell hosts a page's body.
#[derive(Clone, Copy, Debug)]
enum Flow {
    /// Body is wrapped in a vertical `Scroll`, so it may be taller than
    /// the window. The default — a page that hugs its content works at
    /// any window size this way.
    Scroll,
    /// Body is handed the card's leftover height directly. For the
    /// handful of pages whose demo *is* a viewport and must fill.
    Fill,
}

/// How a page's body is built. Most pages are a plain function; the three
/// that own cross-frame resources are dispatched explicitly rather than
/// smuggled through a function pointer that can't carry them.
#[derive(Clone, Copy, Debug)]
enum Body {
    Simple(fn(&mut Ui)),
    State,
    GpuView,
    Fixture,
}

#[derive(Clone, Copy, Debug)]
struct Page {
    /// Heading this page sits under in the nav rail. Pages sharing a
    /// group must be adjacent — the rail emits a heading whenever this
    /// changes.
    group: &'static str,
    label: &'static str,
    /// One line under the page title saying what to look at or try.
    blurb: &'static str,
    flow: Flow,
    body: Body,
}

const PAGES: &[Page] = &[
    Page {
        group: "WIDGETS",
        label: "controls",
        blurb: "Form controls wired together — flip 'Airplane mode' to cascade-disable the \
                network group; Apply runs a fake sync through Ui::animate.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::controls::build),
    },
    Page {
        group: "WIDGETS",
        label: "text edit",
        blurb: "Click to focus, type to insert; arrows / Home / End / Backspace / Delete \
                navigate, Escape blurs.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::text_edit::build),
    },
    Page {
        group: "WIDGETS",
        label: "tabs",
        blurb: "A page view bound to an index, the chip row on its own, and a strip with \
                more tabs than room. Arrow keys travel; Ctrl+Tab cycles.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::tabs::build),
    },
    Page {
        group: "WIDGETS",
        label: "dock",
        blurb: "Drag a chip onto a pane edge to split it, into a strip to join, and \
                right-click one for the split menu.",
        flow: Flow::Fill,
        body: Body::Simple(pages::dock::build),
    },
    Page {
        group: "WIDGETS",
        label: "dialogs",
        blurb: "Modal flows — a dropdown, a confirm dialog, and OS close-request \
                interception.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::dialogs::build),
    },
    Page {
        group: "WIDGETS",
        label: "overlays",
        blurb: "Popups, tooltips, and context menus paint above the main tree and \
                hit-test on top of it.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::overlays::build),
    },
    Page {
        group: "LAYOUT",
        label: "sizing & spacing",
        blurb: "Sizing, justification, alignment, padding / margin / gap, and visibility. \
                The colored chips show where layout puts each child.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::sizing::build),
    },
    Page {
        group: "LAYOUT",
        label: "containers",
        blurb: "Stacks, wrapping flow, and Grid tracks. Resize the window to watch wrap \
                lines and Fill tracks re-divide.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::containers::build),
    },
    Page {
        group: "LAYOUT",
        label: "text",
        blurb: "Wrapping mechanics and intrinsic-dimension composition patterns. Resize \
                the window — the right column reflows live.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::text::build),
    },
    Page {
        group: "LAYOUT",
        label: "clip & transform",
        blurb: "Clip modes against a child that overflows on all four sides, and \
                TranslateScale applied to whole subtrees.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::clip::build),
    },
    Page {
        group: "LAYOUT",
        label: "scroll & split",
        blurb: "Scroll viewports inside splitter panes — drag the bars (double-click \
                recenters); hover a pane and wheel / two-finger scroll.",
        flow: Flow::Fill,
        body: Body::Simple(pages::scroll::build),
    },
    Page {
        group: "LAYOUT",
        label: "pan & zoom",
        blurb: "Wheel pans, Ctrl/Cmd + wheel zooms about the cursor, pinch zooms on a \
                touchpad. Cells stay under the cursor while zooming.",
        flow: Flow::Fill,
        body: Body::Simple(pages::pan_zoom::build),
    },
    Page {
        group: "PAINT",
        label: "shapes",
        blurb: "SDF triangles, raw meshes, and the windowed rect — the inverted-fill \
                corner mask that fakes rounded clipping.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::shapes::build),
    },
    Page {
        group: "PAINT",
        label: "strokes",
        blurb: "Lines, polylines, béziers, and arcs — widths down to hairlines, joins, \
                caps, and per-point / gradient brushes.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::strokes::build),
    },
    Page {
        group: "PAINT",
        label: "gradients",
        blurb: "Linear, radial, and conic brushes through the full path — composer to \
                atlas to shader to premultiplied blend.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::gradients::build),
    },
    Page {
        group: "PAINT",
        label: "shadows",
        blurb: "Drop shadows via raw shape pushes and via widget chrome. Light surfaces \
                because black-on-dark shadows don't read.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::shadows::build),
    },
    Page {
        group: "PAINT",
        label: "icons",
        blurb: "Baked SVG icons rasterized at their exact physical size — gradients, a \
                filtered drop shadow, tintable outlines, and non-square artwork.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::icons::build),
    },
    Page {
        group: "PAINT",
        label: "images",
        blurb: "Fit modes, tint and alpha, tiled repeat, and linear vs nearest sampling \
                under magnification and minification.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::images::build),
    },
    Page {
        group: "RUNTIME",
        label: "motion",
        blurb: "Ui::animate easing curves side by side, and drag responses that report \
                an anchored position with no caller-side tracking.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::motion::build),
    },
    Page {
        group: "RUNTIME",
        label: "gpu view",
        blurb: "Raw wgpu inside a widget — the framework owns the target texture and \
                composites the result as an ordinary image.",
        flow: Flow::Fill,
        body: Body::GpuView,
    },
    Page {
        group: "RUNTIME",
        label: "state & windows",
        blurb: "Carrier-only app state threaded through the record closure, shared live \
                with a second OS window.",
        flow: Flow::Scroll,
        body: Body::State,
    },
    Page {
        group: "RUNTIME",
        label: "fixtures",
        blurb: "Deliberately ugly regression content — id collisions, text z-order, \
                chrome concentricity, premultiplied-alpha repros.",
        flow: Flow::Scroll,
        body: Body::Simple(pages::fixtures::build),
    },
    Page {
        group: "RUNTIME",
        label: "frame bench",
        blurb: "The workload `cargo bench -p palantir --bench criterion -- -d frame` records, \
                drawn live. Nothing here animates — the benches need damage to settle.",
        flow: Flow::Fill,
        body: Body::Fixture,
    },
];

/// State the showcase binary carries across frames.
#[derive(Debug)]
pub(crate) struct State {
    active: usize,
    app: pages::state::AppState,
    /// Persistent renderer for the `gpu view` page — its GPU resources
    /// build lazily on first paint (no device at construction).
    cube: Rc<RefCell<pages::gpu_view::Cube>>,
    /// Backing values for the `frame bench` page. Held across frames for
    /// the same reason the benches hold it: the tree binds `&mut` to these
    /// fields, so a fresh one each frame would reset every control.
    fixture: FrameFixture,
}

impl State {
    pub(crate) fn new(ui: &mut Ui) -> Self {
        let mut theme = Theme::from_palette(&showcase_palette());
        // Library default is no button animation (`anim = None`). The
        // showcase exists to demo the animation primitive — opt in.
        theme.button.defaults.anim = Some(AnimSpec::SPRING);
        ui.set_theme(theme);
        State {
            active: 0,
            app: pages::state::AppState { counter: 0 },
            cube: Rc::new(RefCell::new(pages::gpu_view::Cube::new())),
            fixture: FrameFixture::default(),
        }
    }

    fn build(&mut self, ui: &mut Ui) {
        handle_shortcuts(ui);

        Panel::hstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                self.rail(ui);
                Frame::new()
                    .id_salt("rail-divider")
                    .size((Sizing::fixed(1.0), Sizing::FILL))
                    .background(Background::fill(support::HAIRLINE))
                    .show(ui);
                self.card(ui);
            });

        pages::dialogs::intercept(ui, MAIN_WINDOW);
    }

    /// The nav rail: brand, grouped page list, debug-overlay footer.
    fn rail(&mut self, ui: &mut Ui) {
        let idle = nav_style(false);
        let selected = nav_style(true);
        Panel::vstack()
            .id_salt("rail")
            .size((Sizing::fixed(SIDEBAR_W), Sizing::FILL))
            .padding((12.0, 16.0, 12.0, 12.0))
            .gap(14.0)
            .background(Background::fill(support::SIDEBAR))
            .show(ui, |ui| {
                brand(ui);
                Scroll::vertical()
                    .id_salt("rail-scroll")
                    .size((Sizing::FILL, Sizing::FILL))
                    .overlay_bars()
                    .gap(2.0)
                    .show(ui, |ui| {
                        for (i, page) in PAGES.iter().enumerate() {
                            if i == 0 || PAGES[i - 1].group != page.group {
                                group_heading(ui, page.group, i == 0);
                            }
                            let style = if i == self.active { &selected } else { &idle };
                            let hit = Button::new()
                                .id_salt(page.label)
                                .label(page.label)
                                .style(style)
                                .text_align(Align::LEFT)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui)
                                .left
                                .clicked();
                            if hit {
                                self.active = i;
                            }
                        }
                    });
                debug_toggles(ui);
            });
    }

    /// The page card: title, blurb, rule, then the active page's body.
    fn card(&mut self, ui: &mut Ui) {
        let page = PAGES[self.active];
        Panel::vstack()
            .id_salt("card-gutter")
            .size((Sizing::FILL, Sizing::FILL))
            .padding(16.0)
            .show(ui, |ui| {
                Panel::vstack()
                    .id_salt("card")
                    .size((Sizing::FILL, Sizing::FILL))
                    .padding(20.0)
                    .gap(16.0)
                    .background(
                        Background::rounded(support::CARD, Corners::all(10.0))
                            .with_stroke(Stroke::solid(support::BORDER, 1.0)),
                    )
                    .clip_rounded()
                    .show(ui, |ui| {
                        page_header(ui, page.label, page.blurb);
                        match page.flow {
                            Flow::Scroll => {
                                Scroll::vertical()
                                    .id_salt("page-scroll")
                                    .size((Sizing::FILL, Sizing::FILL))
                                    .overlay_bars()
                                    .gap(support::PAGE_GAP)
                                    .show(ui, |ui| self.body(ui));
                            }
                            Flow::Fill => {
                                Panel::vstack()
                                    .id_salt("page-fill")
                                    .size((Sizing::FILL, Sizing::FILL))
                                    .gap(support::PAGE_GAP)
                                    .show(ui, |ui| self.body(ui));
                            }
                        }
                    });
            });
    }

    fn body(&mut self, ui: &mut Ui) {
        match PAGES[self.active].body {
            Body::Simple(build) => build(ui),
            Body::State => pages::state::build(ui, &mut self.app),
            Body::GpuView => pages::gpu_view::build(ui, &self.cube),
            Body::Fixture => pages::frame_bench::build(ui, &mut self.fixture),
        }
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
        .id_salt("brand")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(1.0)
        .show(ui, |ui| {
            Text::new("palantir")
                .id_salt("brand-name")
                .style(
                    &TextStyle::default()
                        .with_font_size(17.0)
                        .with_weight(FontWeight::BOLD)
                        .with_color(support::INK),
                )
                .show(ui);
            Text::new("widget tour")
                .id_salt("brand-sub")
                .style(
                    &TextStyle::default()
                        .with_font_size(11.0)
                        .with_color(support::INK_FAINT),
                )
                .show(ui);
        });
}

fn group_heading(ui: &mut Ui, name: &'static str, first: bool) {
    Frame::new()
        .id_salt(("group-space", name))
        .size((
            Sizing::fixed(1.0),
            Sizing::fixed(if first { 2.0 } else { 14.0 }),
        ))
        .show(ui);
    Text::new(name)
        .id_salt(("group", name))
        .style(
            &TextStyle::default()
                .with_font_size(10.0)
                .with_weight(FontWeight::BOLD)
                .with_color(support::INK_FAINT),
        )
        .margin((10.0, 0.0, 0.0, 4.0))
        .show(ui);
}

/// The rail footer, where each control mirrors its shortcut so the keys
/// are discoverable instead of living only in a comment.
fn debug_toggles(ui: &mut Ui) {
    Frame::new()
        .id_salt("footer-rule")
        .size((Sizing::FILL, Sizing::fixed(1.0)))
        .background(Background::fill(support::HAIRLINE))
        .show(ui);
    let mut overlay = ui.debug_overlay();
    let mut vsync_on = ui.vsync() == Vsync::On;
    Panel::vstack()
        .id_salt("debug-toggles")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(6.0)
        .show(ui, |ui| {
            Checkbox::new(&mut overlay.damage_rect)
                .id_salt("dbg-damage")
                .label("damage rects  F12")
                .show(ui);
            Checkbox::new(&mut overlay.dim_undamaged)
                .id_salt("dbg-dim")
                .label("dim undamaged  F10")
                .show(ui);
            Checkbox::new(&mut overlay.frame_stats)
                .id_salt("dbg-stats")
                .label("frame stats  F9")
                .show(ui);
            // Paired with `frame stats` on purpose: turning vsync off is
            // visible there as the frame rate coming off the refresh cap.
            Checkbox::new(&mut vsync_on)
                .id_salt("dbg-vsync")
                .label("vsync")
                .show(ui);
            ui_scale_row(ui);
        });
    ui.set_vsync(if vsync_on { Vsync::On } else { Vsync::Off });
    ui.set_debug_overlay(overlay);
}

/// The UI-scale stepper, mirroring the Ctrl+`-` / Ctrl+`=` / Ctrl+`0`
/// shortcuts.
///
/// Two buttons and a readout rather than a slider, which is the control
/// this setting wants: every distinct scale re-rasterizes every glyph on
/// screen, so a ladder of a dozen rungs is worth far more than a
/// continuous drag through hundreds of them.
fn ui_scale_row(ui: &mut Ui) {
    let scale = ui.user_scale();
    let mut next = scale;
    Panel::hstack()
        .id_salt("ui-scale")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(6.0)
        .align(Align::LEFT)
        .show(ui, |ui| {
            if Button::new()
                .id_salt("ui-scale-down")
                .label("−")
                .show(ui)
                .left
                .clicked()
            {
                next = scale.stepped_down();
            }
            if Button::new()
                .id_salt("ui-scale-up")
                .label("+")
                .show(ui)
                .left
                .clicked()
            {
                next = scale.stepped_up();
            }
            let readout = fmt!(ui, "UI {}%  ctrl -/+", scale.percent());
            Text::new(readout)
                .id_salt("ui-scale-readout")
                .style(
                    &TextStyle::default()
                        .with_font_size(11.0)
                        .with_color(support::INK_FAINT),
                )
                .show(ui);
        });
    ui.set_user_scale(next);
}

fn page_header(ui: &mut Ui, title: &'static str, blurb: &'static str) {
    Panel::vstack()
        .id_salt("page-header")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(4.0)
        .show(ui, |ui| {
            Text::new(title)
                .id_salt(("page-title", title))
                .style(&support::title_style())
                .show(ui);
            Text::new(blurb)
                .id_salt(("page-blurb", title))
                .style(&support::blurb_style())
                .size((Sizing::FILL, Sizing::HUG))
                .text_wrap(TextWrap::WrapWithOverflow)
                .show(ui);
            Frame::new()
                .id_salt("header-rule")
                .size((Sizing::FILL, Sizing::fixed(1.0)))
                .margin((0.0, 10.0, 0.0, 0.0))
                .background(Background::fill(support::HAIRLINE))
                .show(ui);
        });
}

/// Cool-neutral recolor of the stock palette so widget chrome and the
/// showcase's own surfaces come from one ladder.
fn showcase_palette() -> Palette {
    Palette {
        text: support::INK,
        text_muted: support::INK_DIM,
        text_disabled: support::INK_DISABLED,
        terminal_bg: support::WINDOW,
        elem: support::ELEM,
        elem_mid: support::ELEM_MID,
        elem_strong: support::ELEM_STRONG,
        border_focused: support::BORDER_FOCUSED,
        accent: support::ACCENT,
    }
}

/// Flat nav-rail button: transparent at rest, accent-washed when it's
/// the open page.
fn nav_style(selected: bool) -> ButtonTheme {
    let label = |c: Color| Some(TextStyle::default().with_font_size(12.0).with_color(c));
    let wash = |alpha: f32, c: Color| Background::rounded(c.with_alpha(alpha), Corners::all(5.0));
    let (rest, hover, press, ink) = if selected {
        (0.16, 0.22, 0.28, support::ACCENT)
    } else {
        (0.0, 0.06, 0.10, support::INK_DIM)
    };
    let tint = if selected {
        support::ACCENT
    } else {
        Color::WHITE
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
            anim: Some(AnimSpec::FAST),
        },
    }
}

/// ⌘Q / Ctrl+Q quits — palantir drops winit's default macOS menu (so its
/// Quit item can't hard-terminate past a close-request veto), which also
/// removes the native ⌘Q, so wire it here. F8 mirrors the `state` page's
/// inspector button; F9 / F10 / F12 mirror the rail's overlay switches,
/// and Ctrl+`-` / Ctrl+`=` / Ctrl+`0` its UI-scale stepper.
fn handle_shortcuts(ui: &mut Ui) {
    if ui.key_pressed(Shortcut::ctrl('Q')) {
        ui.close_window(MAIN_WINDOW);
    }
    if ui.key_pressed(Shortcut::key(Key::F8)) {
        toggle_inspector(ui);
    }
    // One read and one write for all three: `^= pressed` is "toggle if the
    // key fired", and every `key_pressed` was evaluated on this path anyway.
    let mut overlay = ui.debug_overlay();
    overlay.damage_rect ^= ui.key_pressed(Shortcut::key(Key::F12));
    overlay.dim_undamaged ^= ui.key_pressed(Shortcut::key(Key::F10));
    overlay.frame_stats ^= ui.key_pressed(Shortcut::key(Key::F9));
    ui.set_debug_overlay(overlay);

    // The browser bindings, on the ladder `UserScale` carries. Written
    // unconditionally: `set_user_scale` ignores the value already in
    // force, so the no-key case costs a comparison.
    //
    // Three chords step up, because "ctrl and plus" is three different
    // key presses depending on the layout: `=` unshifted where `+` is
    // its shifted twin (US), `+` unshifted where the layout gives it a
    // key of its own (German), and the shifted `+` itself. Each carries
    // its own modifier set, and `Shortcut::matches` compares those
    // exactly, so one binding cannot stand for the others.
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

/// The automated half of "the showcase still feels right by eye": every page
/// records, twice, without panicking and without colliding ids.
///
/// Two frames because the first is the recorder's warmup — a page that reads
/// last frame's response, or whose state row only exists after one pass, only
/// exercises that path on the second.
///
/// This is what stands behind an identity scheme built on `#[track_caller]`
/// helpers rather than hand-written keys: `UiHarness::collisions` reports
/// every pair of siblings that resolved to the same *caller-supplied* id, so a
/// page whose `id_salt`s stopped being distinct fails here rather than
/// silently sharing one widget's cross-frame state between two.
#[cfg(all(test, feature = "internals"))]
mod tests {
    use super::{MAIN_WINDOW, PAGES, State, handle_shortcuts};
    use palantir::internals::UiHarness;
    use palantir::{App, Key, Modifiers, UVec2, UserScale};

    #[test]
    fn every_page_records_two_clean_frames() {
        for (index, page) in PAGES.iter().enumerate() {
            let mut h = UiHarness::new(UVec2::new(1280, 800));
            let mut state = State::new(h.ui());
            state.active = index;
            for frame in 0..2 {
                h.frame(|ui| state.record(MAIN_WINDOW, ui));
                // `fixtures` reuses one explicit id across siblings on
                // purpose — the magenta collision outline is the framework
                // behaviour that page exists to pin. Asserted rather than
                // skipped, so the fixture losing its point also fails here.
                if page.label == "fixtures" {
                    assert!(
                        !h.collisions().is_empty(),
                        "fixtures (frame {frame}): the id-collision fixture \
                         stopped colliding",
                    );
                    continue;
                }
                assert_eq!(
                    h.collisions(),
                    [],
                    "{} (frame {frame}): sibling widgets share an explicit id",
                    page.label,
                );
            }
        }
    }

    /// The UI-scale chords walk the ladder and Ctrl+0 returns to 100%.
    ///
    /// Every spelling of "ctrl and plus" is exercised, because each is a
    /// different `KeyPress` and only the layout decides which one a user
    /// produces — a binding that stopped matching would otherwise fail
    /// for that layout's users alone.
    ///
    /// Each chord is checked to fire **once**. `handle_shortcuts` steps
    /// rather than toggles, so a key visible to two record passes of one
    /// frame would step twice; the frame lifecycle drains the key queue
    /// before pass B, and this is what says so.
    #[test]
    fn the_ui_scale_chords_walk_the_ladder() {
        let mut h = UiHarness::new(UVec2::new(1280, 800));
        // One frame first: a `KeyDown` with nothing focused is dropped at
        // ingress unless a chord subscription from the previous frame
        // claims it, and `key_pressed` is what files those. The running
        // showcase has always recorded a frame before a user can press
        // anything, so this is the state every real press arrives in.
        h.frame(handle_shortcuts);

        const CTRL: Modifiers = Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
            mac_ctrl: false,
        };
        const CTRL_SHIFT: Modifiers = Modifiers {
            shift: true,
            ..CTRL
        };

        for (spelling, mods) in [('=', CTRL), ('+', CTRL), ('+', CTRL_SHIFT)] {
            h.set_modifiers(mods);
            h.ui().set_user_scale(UserScale::ONE);
            h.key(Key::Char(spelling));
            h.frame(handle_shortcuts);
            assert_eq!(
                h.ui().user_scale().get(),
                1.1,
                "ctrl+{spelling} with {mods:?} did not step up exactly once",
            );
        }

        h.set_modifiers(CTRL);
        h.key(Key::Char('-'));
        h.frame(handle_shortcuts);
        assert_eq!(h.ui().user_scale().get(), 1.0);

        h.key(Key::Char('-'));
        h.frame(handle_shortcuts);
        assert_eq!(h.ui().user_scale().get(), 0.9);

        h.key(Key::Char('0'));
        h.frame(handle_shortcuts);
        assert_eq!(h.ui().user_scale(), UserScale::ONE);

        h.frame(handle_shortcuts);
        assert_eq!(
            h.ui().user_scale(),
            UserScale::ONE,
            "a frame with no chord left the scale alone",
        );
    }
}
