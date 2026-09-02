use super::*;
use crate::primitives::widget_id::WidgetId;
use crate::ui::frame_report::FramePaint;
use crate::ui::harness::UiHarness;
use crate::widgets::button::Button;
use std::cell::RefCell;
use std::time::Duration;

/// Every widget module this tree records at least once. Paired with
/// [`EXCLUDED`] it must account for the crate's entire public widget
/// surface — that is what
/// [`covered_and_excluded_account_for_every_public_widget`] enforces,
/// and it is the whole point of keeping these as data rather than
/// prose — a name can fall out of a sentence with nothing to catch
/// it, but not out of a list the suite checks.
const COVERED: &[&str] = &[
    "button",
    "checkbox",
    "combo_box",
    "drag_value",
    "frame",
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

/// Widget modules deliberately absent, each with the reason it can't
/// join. A reason is required: "we forgot" and "it doesn't fit the
/// workload" look identical in a diff otherwise, which is how
/// `splitter` went missing.
const EXCLUDED: &[(&str, &str)] = &[
    (
        "spinner",
        "animates — a PaintAnim wakes the host every frame, so `frame/cached_*` \
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
        "value_response",
        "a return type, not a widget — `slider` and `drag_value` cover what \
             produces it",
    ),
    (
        "select_response",
        "a return type, not a widget — `combo_box` covers what produces it",
    ),
    (
        "overlay_response",
        "a return type, not a widget — `popup` covers what produces it",
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
             sit inside the designed screen — `tests/alloc/dock.rs` measures a \
             steady-state dock frame against a surface of its own",
    ),
    (
        "drag_num",
        "a value binding, not a widget — `slider` and `drag_value` cover \
             both of its variants",
    ),
];

/// Every fixture source file of this module, so the checks below read what
/// the fixture actually records rather than what its docs claim.
/// `include_str!` resolves against this file's directory and runs at
/// compile time — no runtime filesystem access, no working-directory
/// assumption.
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

/// Matches the `use` path a widget module is reached by. The trailing
/// `::` is load-bearing: without it `text` would also match
/// `text_edit`, and the two are separately covered.
fn records(module: &str) -> bool {
    let path = format!("crate::widgets::{module}::");
    SOURCES.iter().any(|src| src.contains(&path))
}

/// The covered list has to be *true*: a widget dropped from the tree
/// must fail here rather than leave the claim standing.
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

/// And the exclusions have to be *current*: one that quietly gained a
/// use is a stale reason nobody reread.
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

/// The gate that stops the drift recurring: the two lists together
/// must name every widget the crate publicly exports, so a new one
/// cannot be shipped without someone deciding which side it lands on.
///
/// Reality comes from `lib.rs`'s own `pub use widgets::<module>::`
/// lines — the definition of the public surface, not a second list
/// that could rot beside it. [`NOT_WIDGETS`] carries the exports that
/// live under `widgets::` without being one.
#[test]
fn covered_and_excluded_account_for_every_public_widget() {
    /// Exported from `widgets::` but not widgets: themes, the shared
    /// response types, and the `Widget` trait itself.
    const NOT_WIDGETS: &[&str] = &["theme", "response", "widget"];

    let mut classified: Vec<&str> = COVERED.to_vec();
    classified.extend(EXCLUDED.iter().map(|(m, _)| *m));

    let mut public: Vec<&str> = include_str!("../lib.rs")
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

/// Nothing this tree records on an overlay layer may capture input
/// aimed at a host recorded beside it.
///
/// The fixture is no longer alone on its surface — the showcase
/// carries it as a page, next to a nav rail. `Popup` is a *modal*
/// primitive: every `Popup::show` records a full-surface
/// `Sense::ABSORB_POINTER` eater beneath its body, and the status
/// bar's toast is recorded unconditionally on every frame. Routing
/// that toast through `Popup` therefore left the entire window
/// unclickable — a dead host, not a subtle misplacement — and
/// standalone there was nothing beside it to reveal that.
///
/// `click_on` is the assertion: it refuses to aim at a widget
/// something else covers, so this fails on the eater's rect rather
/// than reporting a click that silently went elsewhere.
#[test]
fn overlays_never_capture_input_aimed_at_a_host_beside_the_fixture() {
    let nav = WidgetId::from_hash("frame_fixture::tests::host-nav");
    // `RefCell` so the scene closure captures by shared ref and stays
    // `Copy` — `prime` and `response_in` each take it by value.
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

/// The `frame/partial_*` arms model an interactive steady state: one
/// counter changes, everything else holds, so damage collapses to the
/// footer Text's arranged rect. Pinned here — not only inside
/// `ui::bench::assert_partial_invariant` — so a fixture edit that lets
/// the counter reflow its siblings (→ `Full`) or hides the change from
/// the tree entirely (→ `Skip`) fails `cargo test` instead of quietly
/// retargeting the bench.
///
/// Swept across viewport sizes because the `Skip` failure mode is a
/// *layout* bug, not a damage bug: before the card column got its page
/// scroll it overflowed on a normal-sized window and painted over the
/// status bar, and an occluded counter damages nothing. The smallest
/// size here is the one that regressed; the largest is the bench's own
/// `CACHED_SIZE` / `BENCH_SCALE` pair.
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
        // Two frames settle the caches and the popup's anchor (it reads
        // last frame's status-bar rect); the rest are steady state.
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
