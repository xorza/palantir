use super::*;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::frame_report::FramePaint;
use crate::widgets::button::Button;
use std::cell::RefCell;
use std::time::Duration;

/// Every widget module this tree records; with [`EXCLUDED`] it must cover the
/// public widget surface ([`covered_and_excluded_account_for_every_public_widget`]).
const COVERED: &[&str] = &[
    "block",
    "button",
    "checkbox",
    "combo_box",
    "drag_value",
    "expander",
    "grid",
    "panel",
    "popup",
    "progress_bar",
    "radio",
    "scroll",
    "separator",
    "slider",
    "splitter",
    "switch",
    "text",
    "text_edit",
    "tooltip",
];

/// Widget modules deliberately absent, each with the reason.
const EXCLUDED: &[(&str, &str)] = &[
    (
        "color_field",
        "builds a CPU texture and registers it with the image registry, which the \
             deviceless CPU/alloc harnesses never drain — the fixture would hold the \
             texels and measure a first-frame build the steady state never repeats",
    ),
    (
        "color_strip",
        "same as color_field — one registered texture per bar",
    ),
    (
        "color_picker",
        "arranges color_field and color_strip, so it inherits their reason",
    ),
    (
        "color_button",
        "records only a chip until a click opens its picker, and then that picker",
    ),
    (
        "color_swatch",
        "a rect and a checker; joining alone would move every existing bench number \
             for coverage its own tests already give",
    ),
    (
        "spinner",
        "animates — a PaintAnimation wakes the host every frame, so `frame/cached_*` \
             could never settle to no damage",
    ),
    (
        "modal",
        "records nothing until an interaction the benches never deliver",
    ),
    (
        "context_menu",
        "same as modal — nothing is recorded until a right-click",
    ),
    (
        "gpu_view",
        "needs a `wgpu::Device` the deviceless CPU/alloc harnesses don't have",
    ),
    (
        "close_handle",
        "the close request an overlay hands its body, not a widget — `popup` \
             covers what hands it out",
    ),
    (
        "tabs",
        "shows one page at a time — putting any of the fixture's cards behind a \
             strip would stop the workload recording them, and a page of its own \
             retargets every series the frozen structure keeps comparable",
    ),
    (
        "dock",
        "a whole-window pane tree that takes the space it is handed, so it cannot \
             sit inside the designed screen — `tests/alloc/fixtures/dock.rs` measures a \
             steady-state dock frame against a surface of its own",
    ),
    (
        "drag_num",
        "a value binding, not a widget — `slider` and `drag_value` cover \
             both of its variants",
    ),
];

const SOURCES: &[&str] = &[
    include_str!("mod.rs"),
    include_str!("chrome.rs"),
    include_str!("forms.rs"),
    include_str!("lists.rs"),
    include_str!("panes.rs"),
    include_str!("specimen.rs"),
    include_str!("stat_strip.rs"),
    include_str!("tokens.rs"),
];

/// Matches a widget module's `use` path; the trailing `::` keeps `text` from matching `text_edit`.
fn records(module: &str) -> bool {
    let path = format!("crate::widgets::{module}::");
    SOURCES.iter().any(|src| src.contains(&path))
}

#[test]
fn every_covered_widget_is_actually_recorded() {
    for module in COVERED {
        assert!(
            records(module),
            "`{module}` is listed as covered but nothing in the fixture imports \
                 `crate::widgets::{module}::` — drop it from COVERED or record one",
        );
    }
}

#[test]
fn every_excluded_widget_is_actually_absent() {
    for (module, reason) in EXCLUDED {
        assert!(
            !records(module),
            "`{module}` is excluded ({reason}) but the fixture imports it — \
                 move it to COVERED",
        );
    }
}

/// The two lists together must name every widget `lib.rs` exports via
/// `pub use widgets::<module>::`; [`NOT_WIDGETS`] carries the rest.
#[test]
fn covered_and_excluded_account_for_every_public_widget() {
    /// Exported from `widgets::` but not a widget: the theme.
    const NOT_WIDGETS: &[&str] = &["theme"];

    let mut classified: Vec<&str> = COVERED.to_vec();
    classified.extend(EXCLUDED.iter().map(|(m, _)| *m));

    let mut public: Vec<&str> = include_str!("../../lib.rs")
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub use widgets::"))
        .filter_map(|rest| rest.split("::").next())
        .filter(|module| !NOT_WIDGETS.contains(module))
        .collect();
    public.sort_unstable();
    public.dedup();

    assert!(
        !public.is_empty(),
        "parsed no widget exports from lib.rs — the `pub use widgets::` shape changed \
             and this test is now vacuous",
    );

    for module in &public {
        assert!(
            classified.contains(module),
            "`{module}` is publicly exported but neither covered by the fixture nor \
                 listed in EXCLUDED with a reason",
        );
    }
    for module in &classified {
        assert!(
            public.contains(module),
            "`{module}` is listed here but no longer publicly exported — drop it",
        );
    }
}

/// Nothing this tree records on an overlay layer may capture input aimed at a
/// host beside it: `Popup::show` records a full-surface eater and the status
/// bar's toast is recorded every frame, so routing it through `Popup` once
/// left the window unclickable. `click_on` fails on the eater's rect.
#[test]
fn overlays_never_capture_input_aimed_at_a_host_beside_the_fixture() {
    let nav = WidgetId::from_hash("frame_fixture::tests::host-nav");
    // `RefCell` keeps the closure `Copy`.
    let state = RefCell::new(FrameFixture::default());
    let scene = |ui: &mut Ui| {
        Panel::hstack()
            .id_salt("host")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Button::new()
                    .id(nav)
                    .label("nav")
                    .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                    .show(ui);
                state.borrow_mut().render(2, ui);
            });
    };

    let mut h = UiHarness::new(glam::UVec2::new(1280, 800));
    h.prime(2, scene);

    h.click_on(nav);
    assert!(
        h.response_in(nav, scene).left.clicked(),
        "a host widget beside the fixture must stay clickable",
    );
}

/// The `frame/partial_*` arms model one counter changing so damage collapses to
/// the footer Text. Pinned so a fixture edit that reflows siblings (`Full`) or
/// hides the change (`Skip`) fails `cargo test`; swept across sizes since the
/// card column once overflowed a normal window over the status bar.
#[test]
fn footer_counter_alone_yields_partial_damage() {
    for (px, scale) in [
        (glam::UVec2::new(1280, 800), 6usize),
        (glam::UVec2::new(2560, 1600), 6),
        (glam::UVec2::new(3840, 4800), 32),
    ] {
        let mut h = UiHarness::with_text(px).scale(2.0);
        let mut state = FrameFixture::default();
        let mut paint = FramePaint::Full;
        // Two frames settle the caches and the popup's anchor, which reads last frame's status-bar rect.
        for i in 0..5u64 {
            state.tick = state.tick.wrapping_add(1);
            paint = h
                .at(Duration::from_millis(i * 16))
                .frame(|ui| state.render(scale, ui))
                .paint();
        }
        assert_eq!(
            paint,
            FramePaint::Partial,
            "tick-only change must damage just the footer counter at {px:?} @2x, scale {scale}",
        );
    }
}
