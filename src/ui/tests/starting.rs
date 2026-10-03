//! The first frames: the warm-up pass, and what an empty `Ui` still does.

use crate::damage::Damage;
use crate::display::Display;
use crate::display::user_scale::UserScale;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::frontend::Frontend;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::layer::Layer;
use crate::ui::tests::support::{SURFACE, cold_ui};
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};
use glam::{UVec2, Vec2};

/// Pin: an empty frame drives the full pipeline without panicking and
/// produces no draw commands.
#[test]
fn empty_ui_drives_a_frame_safely() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|_| {});

    // Empty UI on the first frame: damage is `None` (skip). Force `Full`
    // to exercise encode/compose and assert the buffers come out empty.
    // No mesh/polyline bytes were recorded, so the Ui record store is empty.
    let mut frontend = Frontend::for_test();
    frontend.build(
        h.ui.frame_scene(),
        RenderPlan {
            clear: h.ui.theme.window_clear,
            damage: Damage::Full,
        },
    );
    let buffer = &frontend.buffer;
    assert!(buffer.quads.is_empty());
    assert!(buffer.texts.is_empty());
    assert!(buffer.groups.is_empty());

    // Synthetic viewport root: even an empty user record produces one node.
    assert_eq!(h.ui.forest.trees[Layer::Main].records.len(), 1);
    assert!(h.engines.damage.prev.is_empty());
    assert!(h.engines.damage.counters.dirty().is_empty());
    assert!(h.damage_region().is_empty());
    assert_eq!(Damage::new(h.collapsed_damage()), None);
}

/// Pin: an empty frame followed by a populated frame works (the
/// recorder retains no per-frame state across frames).
#[test]
fn empty_then_populated_frame() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    h.frame(|_| {});
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |_| {});
    });
    // Synthetic viewport root + user Panel = 2 records.
    assert_eq!(h.ui.forest.trees[Layer::Main].records.len(), 2);
    // The user Panel is rowless (no chrome, no shapes, no children) so
    // it gets no prev entry; the viewport root tracks it as a
    // child-marker row — one entry total.
    assert_eq!(h.engines.damage.prev.len(), 1);
}

/// Pin: `Ui::frame` panics if `display.scale_factor()` is below `EPS`.
#[test]
#[should_panic(expected = "Display::scale_factor() must be finite and ≥ EPSILON")]
fn frame_rejects_zero_scale_factor() {
    let mut h = UiHarness::new(UVec2::new(800, 600)).scale(0.0);
    let _ = h.frame(|_| {});
}

/// Pin: `Display::logical_rect` divides physical by `scale_factor()`,
/// which is both factors — so the user scale takes room out of layout.
#[test]
fn display_logical_rect_scales() {
    let d = Display::from_physical(UVec2::new(800, 600), 2.0);
    assert_eq!(d.logical_rect(), Rect::new(0.0, 0.0, 400.0, 300.0));

    let zoomed = Display {
        user_scale: UserScale::new(2.0),
        ..d
    };
    assert_eq!(zoomed.logical_rect(), Rect::new(0.0, 0.0, 200.0, 150.0));
}

/// On a true first frame the user closure runs **twice** — once for the
/// blackout warmup pass, once for the real pass. The second frame runs
/// it once. The existing `double_layout` arm fires when an input action
/// or a `request_relayout` lands; warmup is the only third trigger.
#[test]
fn cold_start_runs_record_closure_twice_on_first_frame() {
    let mut h = cold_ui();
    let mut calls = 0_u32;
    h.frame(|_| calls += 1);
    assert_eq!(calls, 2, "first frame: warmup pass + real pass");

    let snapshot = calls;
    h.frame(|_| calls += 1);
    assert_eq!(
        calls - snapshot,
        1,
        "second frame: single record pass (no warmup, no action)",
    );
}

/// The warmup pass must see an empty `InputState`. A `PointerMoved`
/// delivered before frame 1 must be invisible to widgets recording
/// during warmup, then visible during the real pass.
#[test]
fn cold_start_blacks_out_input_during_warmup_pass() {
    let mut h = cold_ui();
    h.move_to(Vec2::new(40.0, 40.0));

    let observed: std::cell::RefCell<Vec<Option<Vec2>>> = Default::default();
    h.frame(|ui| {
        observed.borrow_mut().push(ui.input.pointer_pos());
    });
    let observed = observed.into_inner();
    assert_eq!(observed.len(), 2, "warmup + real");
    assert_eq!(
        observed[0], None,
        "warmup pass must see InputState::default() — no pointer",
    );
    assert_eq!(
        observed[1],
        Some(Vec2::new(40.0, 40.0)),
        "real pass must see the held pointer_pos that arrived pre-frame",
    );
}

/// Hover routing on frame 1: pointer is over a clickable widget when
/// the window first opens. Before this fix, `Ui::on_input` would
/// hit-test against an empty cascade so `hovered` would stay `None`
/// until the second frame. The warmup builds the cascade and
/// `refresh_pointer_targets` routes the held pointer against it.
#[test]
fn cold_start_routes_held_pointer_against_warmup_cascade() {
    let mut h = cold_ui();
    // Cursor lands inside the future button rect (button is anchored at
    // (0,0) with 60×30 size below). Delivered before any frame ran;
    // cascade is empty so on_input can't resolve a target.
    h.move_to(Vec2::new(20.0, 10.0));
    assert_eq!(h.ui.input.hovered(), None, "pre-frame: no cascade, no hit");

    let button_id = WidgetId::from_hash("btn");
    h.frame(|ui| {
        Button::new()
            .id(button_id)
            .label("hi")
            .size((60.0, 30.0))
            .show(ui);
    });

    assert_eq!(
        h.ui.input.hovered(),
        Some(button_id),
        "warmup builds cascade; refresh_pointer_targets routes held \
         pointer onto the button before the real record pass",
    );
}

/// First frame, no input — assert the contract pinned by the in-engine
/// `assert!(!first_frame || matches!(damage, Damage::Full))`.
#[test]
fn cold_start_first_frame_damage_is_full() {
    let mut h = cold_ui();
    let report = h.frame(|ui| {
        Block::new()
            .auto_id()
            .size(50.0)
            .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
            .show(ui);
    });
    assert!(
        matches!(
            report.plan,
            Some(RenderPlan {
                damage: Damage::Full,
                ..
            })
        ),
        "first frame: prev snapshot empty, every painting node is new ⇒ Full",
    );
}

/// Relayout / repaint requests issued during the blackout pass must
/// not bias the real-pass `double_layout` gate — otherwise a widget
/// whose first record legitimately asks for relayout would force a
/// third record pass on frame 1 (warmup + pass-A + pass-B).
#[test]
fn cold_start_warmup_relayout_does_not_trigger_pass_b() {
    let mut h = cold_ui();
    let mut calls = 0_u32;
    h.frame(|ui| {
        calls += 1;
        if calls == 1 {
            // Simulate a widget whose first-frame measure depends on
            // state that wasn't seeded yet — fires once during warmup,
            // then is satisfied. Without the reset in `frame`,
            // this leaks into the real pass's `double_layout` arm and
            // we'd see calls == 3 below.
            ui.request_relayout();
        }
    });
    assert_eq!(
        calls, 2,
        "warmup pass + real pass; warmup's relayout request must be discarded",
    );
}

/// The warm `UiHarness` constructors mark the recorder as warm by
/// synthesizing a `prev_stamp`. Tests must observe single-record
/// semantics on their first `run_at` so they don't have to reason
/// about the double-call contract for every assertion.
#[test]
fn warm_constructors_skip_the_warmup_pass() {
    let mut h = UiHarness::new(SURFACE);
    let mut calls = 0_u32;
    h.frame(|_| calls += 1);
    assert_eq!(
        calls, 1,
        "the warm constructors seed prev_stamp; frame 1 is single-pass",
    );
}

/// What the warmup pass asks of the input outlives the pass: an app that
/// moves focus once, on its first record, does so during warmup. Each row
/// starts from a focus and makes one request on the first record only.
/// Both passes then record with the requested focus, and the frame ends
/// on it.
#[test]
fn warmup_keeps_focus_requests() {
    let target = WidgetId::from_hash("warmup-focus");
    let cases = [(None, Some(target)), (Some(target), None)];
    for (before, request) in cases {
        let mut h = cold_ui();
        if let Some(id) = before {
            h.set_focus(id);
        }
        let mut records = 0_u32;
        let mut seen = Vec::new();
        h.frame(|ui| {
            if records == 0 {
                match request {
                    Some(id) => ui.set_focus(id),
                    None => ui.clear_focus(),
                }
            }
            records += 1;
            seen.push(ui.focused_id());
            Block::new().id(target).size(10.0).show(ui);
        });
        assert_eq!(records, 2, "warmup + real");
        assert_eq!(seen, [request, request], "{before:?} → {request:?}");
        assert_eq!(h.ui.focused_id(), request, "{before:?} → {request:?}");
    }
}

/// A scope withdrawn during warmup is gone for the visible pass. Focus
/// sits inside `inner`, so with `inner` live the Escape that arrived
/// before frame 1 is granted to it; withdrawn, the grant falls to `root`.
#[test]
fn warmup_keeps_scope_releases() {
    use crate::input::key_class::KeyFilter;
    use crate::input::keyboard::key::Key;
    use crate::primitives::layout::sizing::Sizing;

    let root = WidgetId::from_hash("warmup-root");
    let inner = WidgetId::from_hash("warmup-inner");
    let editor = WidgetId::from_hash("warmup-editor");
    let mut h = cold_ui();
    h.set_focus(editor);
    h.key(Key::Escape);

    let mut records = 0_u32;
    // Whether the root reads the Escape, per pass after warmup.
    let passes = h.frame_passes(|ui| {
        let at_root = Panel::vstack()
            .id(root)
            .input_scope(KeyFilter::ALL)
            .size((Sizing::fixed(60.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                let at_root = ui.escape_pressed();
                Panel::vstack()
                    .id(inner)
                    .input_scope(KeyFilter::ALL)
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui, |ui| {
                        Block::new().id(editor).size(10.0).show(ui);
                        if records == 0 {
                            ui.release_input_scope(inner);
                        }
                    });
                at_root
            })
            .inner;
        records += 1;
        at_root
    });
    assert!(*passes.a(), "the withdrawn scope cannot hold the grant");
}
