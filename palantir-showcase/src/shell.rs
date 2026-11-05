//! The showcase chrome: a grouped nav rail and a titled page card, both
//! driven by [`PAGES`], the single source of truth for each page's label,
//! group, blurb, key hints, scrolling and builder.

use palantir::{
    Align, App, Block, Button, ButtonTheme, Checkbox, Chrome, Configure, Corners, EditEnd,
    FocusPolicy, FontFamily, Justify, Key, Metrics, Palette, Panel, PopupTrigger, Scroll, Sense,
    Separator, Shortcut, ShortcutMods, Sizing, Stroke, Text, TextEdit, TextWrap, Tooltip, Ui,
    UserScale, VAlign, Vsync, WidgetId, WindowConfig, WindowToken, fmt,
};

use crate::app_state::AppState;
use crate::appearance::{Appearance, SEEDS};
use crate::job::Job;
use crate::pages;
use crate::scaffold::chip::Chip;
use crate::scaffold::rail;
use crate::scaffold::rail::Nav;
use crate::scaffold::tokens::{MEASURE, Tokens};
use crate::support::{Kit, Kits, PAGE_GAP};

pub const MAIN_WINDOW: WindowToken = WindowToken(0);
/// Optional second window mirroring the windows page's counter.
pub(crate) const INSPECTOR_WINDOW: WindowToken = WindowToken(1);

/// Open the inspector, or close it if up; the live window set decides.
pub(crate) fn toggle_inspector(ui: &mut Ui) {
    if ui.is_window_open(INSPECTOR_WINDOW) {
        ui.close_window(INSPECTOR_WINDOW);
    } else {
        ui.open_window(INSPECTOR_WINDOW, WindowConfig::new("inspector"));
    }
}

const M: Metrics = Metrics::DEFAULT;
/// The rail's search field, which Ctrl+K focuses.
const SEARCH: &str = "showcase::rail::search";
/// The rail's scrolling list, which an item scrolls into view in.
const LIST: &str = "showcase::rail::list";
/// The widest a scrolling page's content runs, so a wide window does not strand its stages on
/// the left and stretch its rows to the right.
const COLUMN: f32 = 1040.0;
/// The edge of an accent swatch in the rail's footer.
const SWATCH: f32 = 20.0;

/// A nav rail heading; pages of one group are adjacent in [`PAGES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Widgets,
    Input,
    Layout,
    Paint,
    Runtime,
}

impl Group {
    const fn heading(self) -> &'static str {
        match self {
            Self::Widgets => "WIDGETS",
            Self::Input => "INPUT",
            Self::Layout => "LAYOUT",
            Self::Paint => "PAINT",
            Self::Runtime => "RUNTIME",
        }
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

impl Page {
    /// Whether `query` finds this page: in its label or its blurb, ignoring ASCII case; an
    /// empty query finds every page.
    fn matches(&self, query: &str) -> bool {
        contains_ignoring_case(self.label, query) || contains_ignoring_case(self.blurb, query)
    }

    /// The id of this page's item in the rail.
    fn item_id(&self) -> WidgetId {
        WidgetId::from_hash(("showcase::rail", self.label))
    }

    /// What the page is, the keys it answers to, and where its source is; no keys row for none.
    fn header(&self, ui: &mut Ui, t: Tokens) {
        Panel::vstack()
            .id_salt(("header", self.label))
            .size((Sizing::FILL, Sizing::HUG))
            .gap(M.space.sm)
            .show(ui, |ui| {
                Text::new(self.label).style(&t.title()).show(ui);
                Text::new(self.blurb)
                    .style(&t.dim())
                    .size((Sizing::FILL, Sizing::HUG))
                    .max_size((MEASURE, f32::INFINITY))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
                if !self.keys.is_empty() {
                    Panel::wrap_hstack()
                        .id_salt("keys")
                        .size((Sizing::FILL, Sizing::HUG))
                        .gap(M.space.xs)
                        .line_gap(M.space.xs)
                        .child_align(Align::v(VAlign::Center))
                        .show(ui, |ui| {
                            Text::new("keys")
                                .style(&t.caption())
                                .margin((0.0, 0.0, M.space.xxs, 0.0))
                                .show(ui);
                            for &key in self.keys {
                                Chip::Key.show(ui, t, key);
                            }
                        });
                }
                Text::new(self.source)
                    .id_salt("source")
                    .style(&t.mono(t.ink_faint))
                    .show(ui);
            });
    }
}

/// Whether `needle` is in `haystack`, ASCII letters compared ignoring case and every other byte
/// as it is, so a query allocates nothing; every text holds the empty needle.
fn contains_ignoring_case(haystack: &str, needle: &str) -> bool {
    needle.is_empty()
        || haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// A page's file, from the repository's root, spelled from its module, which the expansion names,
/// so a renamed module breaks the build rather than the header.
macro_rules! source {
    ($module:ident) => {{
        let _ = pages::$module::build;
        concat!("palantir-showcase/src/pages/", stringify!($module), ".rs")
    }};
}

const PAGES: &[Page] = &[
    Page {
        group: Group::Widgets,
        label: "Controls",
        blurb: "Form controls wired together, button themes, and disclosure sections.",
        keys: &["click", "drag", "Space", "←→"],
        flow: Flow::Scroll,
        source: source!(controls),
        build: |ui, app, k| pages::controls::build(ui, k, &app.appearance),
    },
    Page {
        group: Group::Widgets,
        label: "Colors",
        blurb: "The color picker, its parts on their own, and the chip that opens it.",
        keys: &["drag", "Esc"],
        flow: Flow::Scroll,
        source: source!(colors),
        build: |ui, app, k| pages::colors::build(ui, k, &mut app.appearance),
    },
    Page {
        group: Group::Widgets,
        label: "Tabs",
        blurb: "A page view bound to an index, the chip strip on its own, and overflow.",
        keys: &["←→", "Home", "End", "Ctrl+Tab", "wheel"],
        flow: Flow::Scroll,
        source: source!(tabs),
        build: |ui, _, k| pages::tabs::build(ui, k),
    },
    Page {
        group: Group::Widgets,
        label: "Dock",
        blurb: "Tabbed panes that split, join and resize by drag.",
        keys: &["drag chip", "drag divider", "right-click"],
        flow: Flow::Fill,
        source: source!(dock),
        build: |ui, _, k| pages::dock::build(ui, k),
    },
    Page {
        group: Group::Widgets,
        label: "Custom widget",
        blurb: "A widget written against the public authoring API and nothing else.",
        keys: &["click", "Tab", "↑↓"],
        flow: Flow::Scroll,
        source: source!(custom_widget),
        build: |ui, _, k| pages::custom_widget::build(ui, k),
    },
    Page {
        group: Group::Widgets,
        label: "Overlays",
        blurb: "Popups, anchors, a modal, tooltips, context menus and a menu bar, above the main tree.",
        keys: &["click", "hover", "right-click", "Esc"],
        flow: Flow::Scroll,
        source: source!(overlays),
        build: |ui, app, k| pages::overlays::build(ui, k, &mut app.appearance),
    },
    Page {
        group: Group::Input,
        label: "Text input",
        blurb: "Single- and multi-line editors, the edges they report, and IME composition.",
        keys: &["type", "Enter", "Esc", "Shift+arrows", "IME"],
        flow: Flow::Scroll,
        source: source!(text_input),
        build: |ui, _, k| pages::text_input::build(ui, k),
    },
    Page {
        group: Group::Input,
        label: "Focus & keyboard",
        blurb: "Tab traversal, the focus ring, arrow groups, and focus that stays inside an overlay.",
        keys: &["Tab", "Shift+Tab", "←→↑↓", "Enter", "Space", "Esc"],
        flow: Flow::Scroll,
        source: source!(focus),
        build: |ui, app, k| pages::focus::build(ui, k, &mut app.appearance),
    },
    Page {
        group: Group::Layout,
        label: "Sizing & spacing",
        blurb: "Sizing, justification, alignment, padding, margin, gap and visibility.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(sizing),
        build: |ui, _, k| pages::sizing::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "Containers",
        blurb: "Stacks, wrapping flow, and grid tracks.",
        keys: &["drag", "resize window"],
        flow: Flow::Scroll,
        source: source!(containers),
        build: |ui, _, k| pages::containers::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "Text",
        blurb: "Wrapping, and text inside Hug and Fill layouts.",
        keys: &["drag"],
        flow: Flow::Scroll,
        source: source!(text),
        build: |ui, _, k| pages::text::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "Clip & transform",
        blurb: "Clip modes against an overflowing child, and transforms on whole subtrees.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(clip),
        build: |ui, _, k| pages::clip::build(ui, k),
    },
    Page {
        group: Group::Layout,
        label: "Scroll & zoom",
        blurb: "Scroll viewports on three axes in resizable splitter panes; the two-axis one zooms \
                about the pointer.",
        keys: &[
            "wheel",
            "Ctrl+wheel",
            "pinch",
            "drag bar",
            "double-click bar",
        ],
        flow: Flow::Fill,
        source: source!(scroll),
        build: |ui, _, k| pages::scroll::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "Shapes",
        blurb: "SDF triangles, raw meshes, and the windowed rect.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(shapes),
        build: |ui, _, k| pages::shapes::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "Strokes",
        blurb: "Lines, polylines, béziers and arcs: widths, joins, caps and color.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(strokes),
        build: |ui, _, k| pages::strokes::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "Gradients",
        blurb: "Linear, radial and conic brushes, spread modes and interpolation spaces.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(gradients),
        build: |ui, _, k| pages::gradients::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "Shadows",
        blurb: "Drop and inset shadows, as shapes and as widget chrome.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(shadows),
        build: |ui, _, k| pages::shadows::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "Materials",
        blurb: "Backdrop filters: blur, frosted glass and glass, as shapes and as chrome.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(materials),
        build: |ui, _, k| pages::materials::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "Icons",
        blurb: "SVG icons rasterized at the exact physical size they land on.",
        keys: &["Ctrl +", "Ctrl −"],
        flow: Flow::Scroll,
        source: source!(icons),
        build: |ui, _, k| pages::icons::build(ui, k),
    },
    Page {
        group: Group::Paint,
        label: "Images",
        blurb: "Fit modes, tint, tiling, and sampling under magnification and minification.",
        keys: &[],
        flow: Flow::Scroll,
        source: source!(images),
        build: |ui, _, k| pages::images::build(ui, k),
    },
    Page {
        group: Group::Runtime,
        label: "Motion",
        blurb: "Easing curves, and drag handling with no tracking code.",
        keys: &["click", "drag"],
        flow: Flow::Scroll,
        source: source!(motion),
        build: |ui, _, k| pages::motion::build(ui, k),
    },
    Page {
        group: Group::Runtime,
        label: "GPU view",
        blurb: "Raw wgpu inside a widget, composited as an ordinary image.",
        keys: &["drag"],
        flow: Flow::Fill,
        source: source!(gpu_view),
        build: |ui, _, k| pages::gpu_view::build(ui, k),
    },
    Page {
        group: Group::Runtime,
        label: "App & windows",
        blurb: "App state passed through the record closure, a second OS window that shares it, \
                and a window close the app can veto.",
        keys: &["F8", "close window"],
        flow: Flow::Scroll,
        source: source!(windows),
        build: |ui, app, k| pages::windows::build(ui, k, app),
    },
];

/// Which way an Alt+arrow steps through the pages.
#[derive(Clone, Copy, Debug)]
enum Step {
    Back,
    Forward,
}

#[derive(Debug)]
pub struct Shell {
    active: usize,
    app: AppState,
    kits: Kits,
    /// The rail's search: the pages it finds are the ones the rail lists.
    query: String,
    /// The open page was picked from the keyboard, so the rail scrolls its item into view.
    reveal: bool,
}

impl Shell {
    /// The tour on the page at `page`, an index into `Self::page_labels`, its item scrolled into
    /// view, in the stock look.
    pub fn open(page: usize) -> Self {
        debug_assert!(page < PAGES.len(), "page {page} of {}", PAGES.len());
        Self {
            active: page,
            app: AppState::new(),
            kits: Kits::default(),
            query: String::new(),
            reveal: true,
        }
    }

    /// Each page's label, in the rail's order.
    pub(crate) fn page_labels() -> impl Iterator<Item = &'static str> {
        PAGES.iter().map(|page| page.label)
    }

    fn build(&mut self, ui: &mut Ui) {
        handle_shortcuts(ui);
        self.navigate(ui);
        // The focus page sets its own policy; every other page gets the default.
        ui.set_focus_policy(FocusPolicy::default());
        self.app.appearance.apply(ui);
        let k = self.kit(ui);

        Panel::hstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                self.rail(ui, k);
                self.card(ui, k);
            });

        pages::windows::intercept(ui, k, &self.app.appearance);
        // An edit made in this frame builds its theme now; palantir asks for the frame that
        // shows it, which no input may otherwise wake.
        self.app.appearance.apply(ui);
    }

    /// The kit of the shown scheme, over the palette the appearance built its theme from.
    const fn kit(&self, ui: &Ui) -> Kit {
        let scheme = ui.color_scheme();
        self.kits.kit(scheme, self.app.appearance.palette(scheme))
    }

    /// Ctrl+K starts a new search, empty and focused; Alt+↑ and Alt+↓ step through the pages
    /// the search finds, as Storybook binds them.
    fn navigate(&mut self, ui: &mut Ui) {
        if ui.shortcut_pressed(Shortcut::ctrl('K')) {
            self.query.clear();
            ui.set_focus(WidgetId::from_hash(SEARCH));
        }
        if ui.shortcut_pressed(Shortcut::new(ShortcutMods::ALT, Key::ArrowUp)) {
            self.step(Step::Back);
        }
        if ui.shortcut_pressed(Shortcut::new(ShortcutMods::ALT, Key::ArrowDown)) {
            self.step(Step::Forward);
        }
    }

    /// Open the found page before or after the open one; nothing past either end.
    fn step(&mut self, step: Step) {
        let mut found = PAGES
            .iter()
            .enumerate()
            .filter(|(_, page)| page.matches(&self.query))
            .map(|(idx, _)| idx);
        let next = match step {
            Step::Back => found.rev().find(|&idx| idx < self.active),
            Step::Forward => found.find(|&idx| idx > self.active),
        };
        if let Some(idx) = next {
            self.open_page(idx);
        }
    }

    const fn open_page(&mut self, idx: usize) {
        self.active = idx;
        self.reveal = true;
    }

    /// The nav rail: brand, search, the grouped pages it finds, and the appearance footer.
    fn rail(&mut self, ui: &mut Ui, k: Kit) {
        let nav = Nav::new(ui.theme(), k.t);
        let mut list = rail::list(WidgetId::from_hash(LIST));
        if let Some(item) = self.reveal_target(ui) {
            list = list.reveal(item);
        }
        rail::show(ui, k.t, |ui| {
            rail::brand(ui, k.t, "palantir", "widget tour");
            rail::block(ui, |ui| self.search(ui));
            list.show(ui, |ui| self.items(ui, k, &nav));
            rail::block(ui, |ui| self.footer(ui, k));
        });
    }

    /// The search field: Enter opens the first page it finds and hands focus to its item, so
    /// the arrows walk the found list and Alt+arrows the pages, which the field would keep;
    /// Escape empties it.
    fn search(&mut self, ui: &mut Ui) {
        let response = TextEdit::new(&mut self.query)
            .id(WidgetId::from_hash(SEARCH))
            .placeholder("Search pages  Ctrl+K")
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui);
        match response.ended {
            Some(EditEnd::Submitted) => {
                if let Some(idx) = PAGES.iter().position(|page| page.matches(&self.query)) {
                    self.open_page(idx);
                    ui.set_focus(PAGES[idx].item_id());
                }
            }
            Some(EditEnd::Canceled) => self.query.clear(),
            _ => {}
        }
    }

    /// The pages the search finds, each group under its heading.
    fn items(&mut self, ui: &mut Ui, k: Kit, nav: &Nav) {
        let mut group = None;
        for (idx, page) in PAGES.iter().enumerate() {
            if !page.matches(&self.query) {
                continue;
            }
            if group != Some(page.group) {
                group_heading(ui, k, page.group, group.is_none());
                group = Some(page.group);
            }
            if nav.item(ui, page.item_id(), page.label, idx == self.active) {
                self.active = idx;
            }
        }
        if group.is_none() {
            Text::new("No page matches")
                .style(&k.t.caption())
                .margin((rail::INSET, 0.0, 0.0, 0.0))
                .show(ui);
        }
    }

    /// The open page's item while the keyboard's pick waits to come into view. The list's
    /// [`Scroll::reveal`] pans to it once both laid out, so the wait ends on the frame they have;
    /// before that, each frame asks for the next.
    fn reveal_target(&mut self, ui: &mut Ui) -> Option<WidgetId> {
        if !self.reveal {
            return None;
        }
        let page = &PAGES[self.active];
        if !page.matches(&self.query) {
            // The search hides the item, so there is nothing to bring into view.
            self.reveal = false;
            return None;
        }
        let item = page.item_id();
        let laid_out = |id| ui.response_for(id).layout_rect.is_some();
        if laid_out(WidgetId::from_hash(LIST)) && laid_out(item) {
            self.reveal = false;
        } else {
            ui.request_repaint();
        }
        Some(item)
    }

    /// The tour's preferences, the accent and the UI scale, and the diagnostics behind one button.
    fn footer(&mut self, ui: &mut Ui, k: Kit) {
        Separator::horizontal().show(ui);
        rail::label(ui, k.t, "APPEARANCE");
        rail::preferences(ui, k.t);
        accent_row(ui, k, &mut self.app.appearance);
        ui_scale_row(ui, k);
        diagnostics(ui);
    }

    /// The page, on the window's ground: its header, a rule, then the active page's body.
    fn card(&mut self, ui: &mut Ui, k: Kit) {
        let page = PAGES[self.active];
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding((M.space.xl, M.space.xl, M.space.sm, M.space.lg))
            .gap(M.space.xl)
            .show(ui, |ui| {
                page.header(ui, k.t);
                Separator::horizontal().show(ui);
                // Keyed by page so scroll offsets don't carry over.
                match page.flow {
                    Flow::Scroll => {
                        Scroll::vertical()
                            .id_salt(page.label)
                            .size((Sizing::FILL, Sizing::FILL))
                            .overlay_bars()
                            // Keeps the overlay bar off the page's right edge.
                            .padding((0.0, 0.0, M.space.lg, 0.0))
                            .show(ui, |ui| {
                                Panel::vstack()
                                    .size((Sizing::FILL, Sizing::HUG))
                                    .max_size((COLUMN, f32::INFINITY))
                                    .gap(PAGE_GAP)
                                    .show(ui, |ui| (page.build)(ui, &mut self.app, k));
                            });
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

impl App for Shell {
    fn record(&mut self, win: WindowToken, ui: &mut Ui) {
        match win {
            INSPECTOR_WINDOW => {
                self.app.appearance.apply(ui);
                let k = self.kit(ui);
                Panel::vstack()
                    .padding(M.space.lg)
                    .gap(M.space.md)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| pages::windows::shared(ui, k, &mut self.app));
            }
            _ => self.build(ui),
        }
    }

    /// A background job runs off the frame and wakes the window as it steps.
    fn is_busy(&self) -> bool {
        self.app.job.as_ref().is_some_and(Job::is_running)
    }
}

/// A group's heading, on the items' text column; the first sits closer to the search above it.
fn group_heading(ui: &mut Ui, k: Kit, group: Group, first: bool) {
    let top = if first { M.space.xs } else { M.space.lg };
    Text::new(group.heading())
        .id_salt(group.heading())
        .style(&k.t.heading())
        .margin((rail::INSET, top, 0.0, M.space.xs))
        .show(ui);
}

/// The stock accent, then each seed of [`SEEDS`], as the shown scheme solves it; the one in use
/// is ringed in the ink.
fn accent_row(ui: &mut Ui, k: Kit, look: &mut Appearance) {
    let scheme = ui.color_scheme();
    let current = look.seed();
    let mut picked = current;
    Panel::hstack()
        .id_salt("accents")
        .size((Sizing::FILL, Sizing::HUG))
        .justify(Justify::SpaceBetween)
        .show(ui, |ui| {
            let stock = (None, Palette::for_scheme(scheme).accent);
            let seeds = SEEDS
                .iter()
                .zip(look.seed_accents(scheme))
                .map(|(&seed, &accent)| (Some(seed), accent));
            for (idx, (seed, accent)) in [stock].into_iter().chain(seeds).enumerate() {
                let ring = if seed == current {
                    Stroke::new(k.t.ink, M.stroke.line)
                } else {
                    Stroke::NONE
                };
                let clicked = Block::new()
                    .id(accent_id(idx))
                    .size((Sizing::fixed(SWATCH), Sizing::fixed(SWATCH)))
                    .sense(Sense::CLICK)
                    .focusable(true)
                    .chrome(Chrome::rounded(accent, Corners::all(SWATCH / 2.0)).with_border(ring))
                    .show(ui)
                    .clicked();
                if clicked {
                    picked = seed;
                }
            }
        });
    look.set_seed(picked);
}

/// The id of the accent swatch at `idx`: the stock accent, then each seed of [`SEEDS`].
fn accent_id(idx: usize) -> WidgetId {
    WidgetId::from_hash(("showcase::rail::accent", idx))
}

/// The UI-scale stepper: `−  100%  +`. The readout only reads: a scrubbable value would
/// re-lay-out under the pointer mid-drag.
fn ui_scale_row(ui: &mut Ui, k: Kit) {
    let scale = ui.user_scale();
    let step = ui.theme().menu_bar.title.clone();
    let step = ButtonTheme {
        selected: step.clone(),
        unselected: step,
        defaults: ui.theme().menu_bar.defaults,
    };
    let mut next = scale;
    Panel::hstack()
        .id_salt("ui scale")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(M.space.xs)
        .child_align(Align::v(VAlign::Center))
        .show(ui, |ui| {
            Text::new("UI scale")
                .style(&k.t.caption())
                .margin((rail::INSET, 0.0, 0.0, 0.0))
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui);
            if Button::new().style(&step).label("−").show(ui).clicked() {
                next = scale.stepped_down();
            }
            // Fixed width, mono: the readout's width changes between 90% and 100%.
            Text::new(fmt!(ui, "{}%", scale.percent()))
                .family(FontFamily::MONO)
                .font_size(M.text.caption)
                .size((Sizing::fixed(34.0), Sizing::HUG))
                .text_align(Align::CENTER)
                .show(ui);
            let up = Button::new().style(&step).label("+").show(ui);
            let clicked = up.clicked();
            if clicked {
                next = scale.stepped_up();
            }
            Tooltip::on(&up, "UI scale — Ctrl +/− · Ctrl 0 resets").show(ui);
        });
    ui.set_user_scale(next);
}

/// The debug overlays and vsync, behind one button, each switch mirroring its key.
fn diagnostics(ui: &mut Ui) {
    let trigger = Button::new()
        .id_salt("diagnostics")
        .label("Diagnostics ▾")
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui);
    let mut overlay = ui.debug_overlay();
    let mut vsync_on = ui.vsync() == Vsync::On;
    PopupTrigger::on(&trigger)
        .padding(M.space.md)
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
        });
    ui.set_vsync(if vsync_on { Vsync::On } else { Vsync::Off });
    ui.set_debug_overlay(overlay);
}

/// ⌘Q / Ctrl+Q quits (palantir drops winit's default macOS menu, and with it the native ⌘Q). F8
/// mirrors the inspector button; F9 / F10 / F12 the diagnostics switches; Ctrl+`-` / `=` / `0`
/// the UI-scale stepper.
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
        || ui.shortcut_pressed(Shortcut::new(
            ShortcutMods::CTRL | ShortcutMods::SHIFT,
            Key::Char('+'),
        ));
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

    use palantir::glam::UVec2;
    use palantir::{
        AgentWindow, ColorScheme, Harness, Key, Locator, Preference, Reduction, Scroll, Shortcut,
        ShortcutMods, UiPreferences, WidgetId,
    };

    use crate::appearance::{SEEDS, accent_for};
    use crate::shell::{LIST, PAGES, SEARCH, Shell, accent_id};

    /// Every page's source is a file of the repository, so the header names no file that is gone.
    #[test]
    fn the_sources_name_real_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the crate sits in the repository");
        for page in PAGES {
            assert!(root.join(page.source).is_file(), "{}", page.source);
        }
    }

    /// A query finds a page by its label or its blurb, ASCII case ignored and other letters as
    /// they are: "WINDOW" finds App & windows by its label and Shapes by "windowed rect" in its
    /// blurb; "béz" finds Strokes' "béziers"; the empty query finds every page.
    #[test]
    fn a_query_finds_pages_by_label_or_blurb() {
        let found = |query: &str| -> Vec<&str> {
            PAGES
                .iter()
                .filter(|page| page.matches(query))
                .map(|page| page.label)
                .collect()
        };
        assert_eq!(found("").len(), PAGES.len());
        assert_eq!(found("WINDOW"), ["Shapes", "App & windows"]);
        assert_eq!(found("grad"), ["Gradients"]);
        assert_eq!(found("béz"), ["Strokes"]);
        assert_eq!(found("BÉZ"), Vec::<&str>::new(), "only ASCII letters fold");
        assert_eq!(found("no such page"), Vec::<&str>::new());
    }

    fn label(shell: &Shell) -> &'static str {
        PAGES[shell.active].label
    }

    /// Ctrl+K starts a search; typing narrows the pages Alt+↑ and Alt+↓ step through, which stop
    /// at either end; Enter opens the first found; Escape empties the search.
    #[test]
    fn the_keyboard_searches_and_steps_through_the_pages() {
        let mut h = Harness::builder(UVec2::new(1280, 800)).mono().build();
        let mut shell = Shell::open(0);
        let settle = |h: &mut Harness, shell: &mut Shell| {
            h.session(shell).settle().expect("the frames settle");
        };
        settle(&mut h, &mut shell);
        let alt = |key| Shortcut::new(ShortcutMods::ALT, key);

        h.key(alt(Key::ArrowDown));
        settle(&mut h, &mut shell);
        assert_eq!(label(&shell), "Colors");
        h.key(alt(Key::ArrowUp));
        h.key(alt(Key::ArrowUp));
        settle(&mut h, &mut shell);
        assert_eq!(label(&shell), "Controls", "nothing before the first page");

        h.key(Shortcut::ctrl('K'));
        settle(&mut h, &mut shell);
        assert_eq!(h.ui().focus(), Some(WidgetId::from_hash(SEARCH)));
        h.type_text("window");
        h.key(Key::Enter);
        settle(&mut h, &mut shell);
        assert_eq!(label(&shell), "Shapes", "Enter opens the first page found");
        assert_eq!(
            h.ui().focus(),
            Some(PAGES[13].item_id()),
            "and focuses its item"
        );
        h.key(alt(Key::ArrowDown));
        settle(&mut h, &mut shell);
        assert_eq!(
            label(&shell),
            "App & windows",
            "the next found, past the rest"
        );
        h.key(alt(Key::ArrowDown));
        settle(&mut h, &mut shell);
        assert_eq!(
            label(&shell),
            "App & windows",
            "nothing past the last found"
        );

        h.key(Shortcut::ctrl('K'));
        settle(&mut h, &mut shell);
        assert_eq!(
            h.ui().focus(),
            Some(WidgetId::from_hash(SEARCH)),
            "Ctrl+K from the list"
        );
        assert_eq!(shell.query, "", "a new search");
        h.type_text("grad");
        settle(&mut h, &mut shell);
        h.key(Key::Escape);
        settle(&mut h, &mut shell);
        assert_eq!(shell.query, "");
    }

    /// A click on an accent swatch asks for the frame that builds the theme, with no other input
    /// to wake one: the ring the theme draws takes the seed as the shown scheme solves it.
    /// Each preference row sets only its own preference, and the arrows step within one track.
    #[test]
    fn a_pressed_segment_sets_its_rows_preference() {
        let mut h = Harness::builder(UVec2::new(1280, 800)).mono().build();
        let mut shell = Shell::open(0);
        h.session(&mut shell).settle().expect("the frames settle");
        let prefs = |h: &Harness| h.ui().preferences();
        assert_eq!(
            prefs(&h),
            UiPreferences::default(),
            "each follows the system"
        );

        h.click("Dark");
        h.session(&mut shell).settle().expect("the frames settle");
        // "Less" is on the motion row, then the transparency row.
        h.click(Locator::from("Less").nth(1));
        h.session(&mut shell).settle().expect("the frames settle");
        assert_eq!(
            prefs(&h),
            UiPreferences {
                scheme: Preference::Fixed(ColorScheme::Dark),
                motion: Preference::System,
                transparency: Preference::Fixed(Reduction::Reduced),
            }
        );

        // The motion row's "Full", then one arrow right, which moves the pick to its "Less".
        h.click(Locator::from("Full").nth(0));
        h.session(&mut shell).settle().expect("the frames settle");
        assert_eq!(
            h.ui().preferences().motion,
            Preference::Fixed(Reduction::Full)
        );
        h.key(Key::ArrowRight);
        h.session(&mut shell).settle().expect("the frames settle");
        assert_eq!(
            prefs(&h),
            UiPreferences {
                scheme: Preference::Fixed(ColorScheme::Dark),
                motion: Preference::Fixed(Reduction::Reduced),
                transparency: Preference::Fixed(Reduction::Reduced),
            }
        );
    }

    #[test]
    fn a_picked_accent_reaches_the_theme() {
        let mut h = Harness::builder(UVec2::new(1280, 800)).mono().build();
        let mut shell = Shell::open(0);
        h.session(&mut shell).settle().expect("the frames settle");
        h.click(accent_id(1));
        let report = h.frame_app(&mut shell);
        assert!(
            report.repaint_requested,
            "the edit asks for the frame that builds its theme"
        );
        h.session(&mut shell).settle().expect("the frames settle");
        let scheme = h.ui().color_scheme();
        assert_eq!(shell.app.appearance.seed(), Some(SEEDS[0]));
        assert_eq!(
            h.ui().theme().focus_ring.color,
            accent_for(SEEDS[0], scheme)
        );
    }

    /// A page opened from the keyboard far down the list scrolls its item into view: on a short
    /// window the last item starts out of view, and after Alt+↓ steps onto it the list has moved
    /// so the whole 28 px item shows.
    #[test]
    fn a_page_opened_from_the_keyboard_scrolls_into_view() {
        let mut h = Harness::builder(UVec2::new(1280, 480)).mono().build();
        let last = PAGES.len() - 1;
        let mut shell = Shell::open(last - 1);
        h.session(&mut shell).settle().expect("the frames settle");
        let item = PAGES[last].item_id();
        let list = WidgetId::from_hash(LIST);
        let shown = |h: &mut Harness| h.ui().response_for(item).rect.map(|rect| rect.size.h);
        assert_ne!(
            shown(&mut h),
            Some(28.0),
            "the last item starts out of full view"
        );
        let before = Scroll::offset(h.ui(), list).y;
        h.key(Shortcut::new(ShortcutMods::ALT, Key::ArrowDown));
        h.session(&mut shell).settle().expect("the frames settle");
        assert_eq!(label(&shell), "App & windows");
        assert!(Scroll::offset(h.ui(), list).y > before, "the list scrolled");
        assert_eq!(shown(&mut h), Some(28.0), "the whole item shows");
    }
}
