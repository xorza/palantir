use crate::TextStyle;
use crate::Ui;
use crate::display::Display;
use crate::host::shared::HostShared;
use crate::input::InputEvent;
use crate::primitives::background::Background;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect};
use crate::renderer::frontend::Frontend;
use crate::renderer::plan::{RenderKind, RenderPlan};
use crate::scene::damage::Damage;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::tree::node::NodeId;
use crate::text::wrap::TextWrap;
use crate::ui::frame::{FrameRuntime, FrameStamp};
use crate::widgets::ResponseSnapshot;
use crate::widgets::{button::Button, frame::Frame, panel::Panel, text::Text};
use glam::{IVec2, UVec2, Vec2};
use std::cell::{Cell, RefCell};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(200, 200);

fn measure_calls(ui: &Ui) -> u64 {
    ui.resources.text.measure_calls()
}

fn ui_with_shared(shared: &HostShared) -> Ui {
    let mut ui = Ui::new(shared.resources.clone());
    ui.frame_runtime.prev_stamp = Some(FrameStamp::new(ui.display, Duration::ZERO));
    ui
}

fn blue_frame(ui: &mut Ui, salt: &'static str) -> NodeId {
    Frame::new()
        .id(WidgetId::from_hash(salt))
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.2, 0.4, 0.8).into(),
            ..Default::default()
        })
        .show(ui)
        .node()
}

fn add_blink_shape(ui: &mut Ui, half: Duration) {
    use crate::scene::tree::paint_anims::PaintAnim;
    use crate::shape::Shape;

    ui.add_shape_animated(
        Shape::rect(Rect::new(0.0, 0.0, 4.0, 12.0)).fill(Color::rgb(1.0, 0.0, 0.0)),
        PaintAnim::BlinkOpacity {
            half_period: half,
            started_at: Duration::ZERO,
        },
    );
}

/// Two `.id(WidgetId::from_hash("dup"))` calls in one frame would silently corrupt
/// every per-id store. Instead of panicking, `SeenIds::record`
/// disambiguates the second one (same path as auto-id collisions),
/// `Forest` pairs both colliding nodes via `Forest.collisions`, and
/// the encoder emits a magenta `DrawRect` at each colliding node's
/// arranged rect after the regular paint walk.
#[test]
fn duplicate_explicit_widget_id_disambiguates_and_flags() {
    let mut ui = Ui::for_test();
    let button_node = Cell::new(NodeId(0));
    let duplicate_id = WidgetId::from_hash("dup");
    ui.run_at_without_baseline(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let a_node = Button::new().id(duplicate_id).show(ui).node();
            Button::new().id(duplicate_id).show(ui);
            button_node.set(a_node);
        });
    });
    // One collision pair should be recorded, survives until the next
    // `pre_record` so the encoder can read it.
    assert_eq!(
        ui.forest.collisions.len(),
        1,
        "expected exactly one explicit collision recorded",
    );
    assert_eq!(
        ui.cascades.hits.widget_id(),
        [duplicate_id, duplicate_id.with(1)],
        "hit rows must retain both resolved IDs rather than the duplicated raw ID",
    );
    let button_rect = ui.layout[Layer::Main].rect[button_node.get().idx()];
    // Drive the encoder and check the emitted quads. The two overlay
    // quads should be stroked, magenta-ish, and rect-equal to the two
    // colliding buttons' arranged rects.
    // Share Ui's record store so any mesh/polyline bytes pushed at
    // record time are visible at compose / upload — the WindowDriver wiring
    // for real apps.
    let mut frontend = Frontend::for_test();
    frontend.build(
        ui.frame_scene(),
        RenderPlan {
            clear: ui.theme.window_clear,
            kind: RenderKind::Full,
        },
    );
    let buffer = &frontend.buffer;
    let overlay_quads: Vec<_> = buffer
        .quads
        .iter()
        .filter(|q| q.stroke_width > 2.5 && q.stroke_width < 3.5)
        .collect();
    assert_eq!(
        overlay_quads.len(),
        2,
        "expected 2 magenta collision overlay quads in the render buffer",
    );
    // Pin rect math: the first button's arranged rect maps to one
    // of the overlay quads (physical-px == logical at scale=1).
    let matched = overlay_quads.iter().any(|q| {
        (q.rect.min.x - button_rect.min.x).abs() < 1.0
            && (q.rect.min.y - button_rect.min.y).abs() < 1.0
            && (q.rect.size.w - button_rect.size.w).abs() < 1.0
            && (q.rect.size.h - button_rect.size.h).abs() < 1.0
    });
    assert!(
        matched,
        "no overlay quad matched first button's arranged rect {button_rect:?}; overlays: {overlay_quads:?}",
    );
}

/// Cross-layer collision: `.id(WidgetId::from_hash("dup"))` in Main and another with
/// the same key inside a `Ui::layer(Popup, ...)` body. `SeenIds.curr`
/// is shared across layers, so the second occurrence is detected as a
/// collision. Each `CollisionRecord` endpoint carries its own `Layer`,
/// so the encoder paints each overlay at the correct per-layer rect.
#[test]
fn cross_layer_explicit_widget_id_collision_resolves_per_layer() {
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(UVec2::new(200, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Button::new().id(WidgetId::from_hash("dup")).show(ui);
        });
        ui.layer(Layer::Popup, glam::Vec2::ZERO, None, |ui| {
            Button::new().id(WidgetId::from_hash("dup")).show(ui);
        });
    });
    assert_eq!(
        ui.forest.collisions.len(),
        1,
        "expected one collision pair across Main + Popup",
    );
    let pair = ui.forest.collisions[0];
    assert_eq!(
        pair.first.layer,
        Layer::Main,
        "first occurrence should be in Main, got {:?}",
        pair.first.layer,
    );
    assert_eq!(
        pair.second.layer,
        Layer::Popup,
        "second occurrence should be in Popup, got {:?}",
        pair.second.layer,
    );
    // Each endpoint's rect must come from its own layer's `LayerLayout`.
    let main_rect = ui.layout[Layer::Main].rect[pair.first.node.idx()];
    let popup_rect = ui.layout[Layer::Popup].rect[pair.second.node.idx()];
    // Share Ui's record store so any mesh/polyline bytes pushed at
    // record time are visible at compose / upload — the WindowDriver wiring
    // for real apps.
    let mut frontend = Frontend::for_test();
    frontend.build(
        ui.frame_scene(),
        RenderPlan {
            clear: ui.theme.window_clear,
            kind: RenderKind::Full,
        },
    );
    let buffer = &frontend.buffer;
    let overlay_quads: Vec<_> = buffer
        .quads
        .iter()
        .filter(|q| q.stroke_width > 2.5 && q.stroke_width < 3.5)
        .collect();
    assert_eq!(overlay_quads.len(), 2, "expected 2 overlay quads");
    let has_main = overlay_quads
        .iter()
        .any(|q| (q.rect.min - main_rect.min).length() < 1.0);
    let has_popup = overlay_quads
        .iter()
        .any(|q| (q.rect.min - popup_rect.min).length() < 1.0);
    assert!(has_main, "no overlay quad at Main rect {main_rect:?}");
    assert!(has_popup, "no overlay quad at Popup rect {popup_rect:?}");
}

#[test]
fn layout_outputs_stay_isolated_per_layer_across_cache_hits() {
    let mut ui = Ui::for_test_text();
    let main_id = WidgetId::from_hash("layer-output-main");
    let popup_id = WidgetId::from_hash("layer-output-popup");

    let mut record = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("layer-output-main-root"))
            .show(ui, |ui| {
                Button::new()
                    .id(main_id)
                    .label("main layer")
                    .size((40.0, 20.0))
                    .show(ui);
            });
        ui.layer(Layer::Popup, Vec2::new(80.0, 60.0), None, |ui| {
            Button::new()
                .id(popup_id)
                .label("popup layer")
                .size((70.0, 30.0))
                .show(ui);
        });
    };
    let node_for = |ui: &Ui, layer: Layer, id: WidgetId| {
        let index = ui.forest.trees[layer]
            .records
            .widget_id()
            .iter()
            .position(|widget_id| *widget_id == id)
            .unwrap();
        NodeId(index as u32)
    };

    ui.run_at_without_baseline(SURFACE, &mut record);
    let main_node = node_for(&ui, Layer::Main, main_id);
    let popup_node = node_for(&ui, Layer::Popup, popup_id);
    let cold_main = ui.layout[Layer::Main].rect[main_node.idx()];
    let cold_popup = ui.layout[Layer::Popup].rect[popup_node.idx()];
    assert_eq!(cold_main, Rect::new(0.0, 0.0, 40.0, 20.0));
    assert_eq!(cold_popup, Rect::new(80.0, 60.0, 70.0, 30.0));

    let main_span = ui.layout[Layer::Main].text_spans[main_node.idx()];
    let popup_span = ui.layout[Layer::Popup].text_spans[popup_node.idx()];
    assert_eq!(main_span, Span::new(0, 1));
    assert_eq!(popup_span, Span::new(0, 1));
    assert_eq!(ui.layout[Layer::Main].text_shapes.len(), 1);
    assert_eq!(ui.layout[Layer::Popup].text_shapes.len(), 1);
    let cold_main_key = ui.layout[Layer::Main].text_shapes[main_span.start as usize].key;
    let cold_popup_key = ui.layout[Layer::Popup].text_shapes[popup_span.start as usize].key;
    assert_ne!(cold_main_key, cold_popup_key);

    ui.run_at_without_baseline(SURFACE, &mut record);
    assert!(
        !ui.layout_engine.scratch.cache_hits.is_empty(),
        "warm frame must exercise measure-cache restoration",
    );
    let main_node = node_for(&ui, Layer::Main, main_id);
    let popup_node = node_for(&ui, Layer::Popup, popup_id);
    assert_eq!(ui.layout[Layer::Main].rect[main_node.idx()], cold_main);
    assert_eq!(ui.layout[Layer::Popup].rect[popup_node.idx()], cold_popup);
    assert_eq!(
        ui.layout[Layer::Main].text_shapes[main_span.start as usize].key,
        cold_main_key,
    );
    assert_eq!(
        ui.layout[Layer::Popup].text_shapes[popup_span.start as usize].key,
        cold_popup_key,
    );
}

/// Pin: the encoder-direct overlay path leaves `Layer::Debug` empty
/// (no sink node recorded) — guards against silent regression back to
/// the prior "sink in Debug" approach.
#[test]
fn collisions_do_not_record_into_debug_layer() {
    let mut ui = Ui::for_test();
    assert!(
        !ui.resources.diagnostics.overlay.borrow().frame_stats,
        "test relies on frame_stats off — Debug should otherwise stay empty",
    );
    ui.run_at_without_baseline(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new().id(WidgetId::from_hash("dup")).show(ui);
            Button::new().id(WidgetId::from_hash("dup")).show(ui);
        });
    });
    assert!(
        !ui.forest.collisions.is_empty(),
        "collision should have been recorded",
    );
    assert_eq!(
        ui.forest.trees[Layer::Debug].records.len(),
        0,
        "encoder-direct overlay path must not record nodes into Layer::Debug",
    );
}

/// Auto-generated ids (call-site hash) silently disambiguate when the same
/// site fires more than once per frame — the "loop / closure helper" case.
#[test]
fn auto_id_collisions_disambiguate() {
    fn chip(ui: &mut Ui) {
        Frame::new().auto_id().show(ui);
    }
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            chip(ui);
            chip(ui);
            chip(ui);
        });
    });
    // Synthetic viewport root + 1 panel + 3 chips = 5 distinct ids, no panic.
    assert_eq!(ui.forest.trees[Layer::Main].records.len(), 5);
}

/// Cascade runs in `post_record` (after each pass's measure+arrange),
/// not in `finalize_frame`. Means a `request_relayout` re-record can
/// read pass A's arranged rect via `response_for(id).rect` — the
/// invariant `ContextMenu::show` relies on to clamp its anchor in
/// the same frame as the first open, and the general API contract
/// for any widget that needs its own size mid-frame.
#[test]
fn cascade_visible_to_relayout_pass() {
    use std::cell::Cell;
    let pass = Cell::new(0u32);
    let pass_a_rect = Cell::new(None::<Rect>);
    let pass_b_rect = Cell::new(None::<Rect>);
    let id_salt = "cascade-relayout-probe";

    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |ui| {
        let probe_resp: std::cell::RefCell<Option<ResponseSnapshot>> = RefCell::new(None);
        Panel::vstack().auto_id().show(ui, |ui| {
            *probe_resp.borrow_mut() = Some(
                Frame::new()
                    .id(WidgetId::from_hash(id_salt))
                    .size(40.0)
                    .show(ui)
                    .snapshot(),
            );
        });
        let resp = probe_resp.into_inner().unwrap();
        match pass.get() {
            0 => {
                // Pass A: no cascade yet for our frame this run — first
                // ever recording of this widget. Trigger pass B.
                pass_a_rect.set(resp.state.rect);
                ui.request_relayout();
            }
            1 => {
                // Pass B: cascade was rebuilt by pass A's post_record,
                // so response_for now returns pass A's arranged rect.
                pass_b_rect.set(resp.state.rect);
            }
            _ => unreachable!("relayout capped at one retry per frame"),
        }
        pass.set(pass.get() + 1);
    });

    assert_eq!(pass.get(), 2, "expected exactly two record passes");
    assert!(
        pass_a_rect.get().is_none(),
        "pass A sees no cascade entry yet (widget first recorded this frame)",
    );
    let b = pass_b_rect.get().expect("pass B reads pass-A cascade");
    assert_eq!(b.size.w, 40.0);
    assert_eq!(b.size.h, 40.0);
}

/// Pin: an empty frame drives the full pipeline without panicking and
/// produces no draw commands.
#[test]
fn empty_ui_drives_a_frame_safely() {
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |_| {});

    // Empty UI on the first frame: damage is `None` (skip). Force `Full`
    // to exercise encode/compose and assert the buffers come out empty.
    // No mesh/polyline bytes were recorded, so the Ui record store is empty.
    let mut frontend = Frontend::for_test();
    frontend.build(
        ui.frame_scene(),
        RenderPlan {
            clear: ui.theme.window_clear,
            kind: RenderKind::Full,
        },
    );
    let buffer = &frontend.buffer;
    assert!(buffer.quads.is_empty());
    assert!(buffer.texts.is_empty());
    assert!(buffer.groups.is_empty());

    // Synthetic viewport root: even an empty user record produces one node.
    assert_eq!(ui.forest.trees[Layer::Main].records.len(), 1);
    assert!(ui.damage_engine.prev.is_empty());
    assert!(ui.damage_engine.dirty.is_empty());
    assert!(ui.damage_region().rects.is_empty());
    assert_eq!(Damage::new(ui.damage_region()), Damage::Skip,);
}

/// Pin: an empty frame followed by a populated frame works (the
/// recorder retains no per-frame state across frames).
#[test]
fn empty_then_populated_frame() {
    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(100, 100), |_| {});
    ui.run_at(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |_| {});
    });
    // Synthetic viewport root + user Panel = 2 records.
    assert_eq!(ui.forest.trees[Layer::Main].records.len(), 2);
    // The user Panel is rowless (no chrome, no shapes, no children) so
    // it gets no prev entry; the viewport root tracks it as a
    // child-marker row — one entry total.
    assert_eq!(ui.damage_engine.prev.len(), 1);
}

/// Pin: `Ui::frame` panics if `display.scale_factor` is below `EPS`.
#[test]
#[should_panic(expected = "Display::scale_factor must be ≥ EPSILON")]
fn frame_rejects_zero_scale_factor() {
    let mut ui = Ui::for_test();
    let _ = ui.record_test_frame_without_baseline(
        Display::from_physical(UVec2::new(800, 600), 0.0),
        Duration::ZERO,
        |_| {},
    );
}

/// Pin: `Display::logical_rect` divides physical by scale_factor.
#[test]
fn display_logical_rect_scales() {
    let d = Display::from_physical(UVec2::new(800, 600), 2.0);
    assert_eq!(d.logical_rect(), Rect::new(0.0, 0.0, 400.0, 300.0));
}

#[test]
fn prev_frame_empty_before_first_frame() {
    let ui = Ui::for_test();
    assert!(ui.damage_engine.prev.is_empty());
}

/// Pin the row invariant: after the first frame, widgets with paint
/// rows land in `prev` — painting widgets with their arranged rect and
/// authoring hash, and chromeless parents via their child-marker rows
/// (paint-order tracking), whose all-zero screens union to no paint
/// extent. A rowless node (childless Panel without chrome) stays out.
#[test]
fn prev_frame_captures_nodes_with_rows() {
    let mut ui = Ui::for_test();
    let mut frame_node = None;
    ui.run_at_without_baseline(SURFACE, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                frame_node = Some(blue_frame(ui, "a"));
                Panel::hstack()
                    .id(WidgetId::from_hash("empty"))
                    .show(ui, |_| {});
            });
    });
    let frame_node = frame_node.unwrap();
    let prev = &ui.damage_engine.prev;
    let snap = &prev[&WidgetId::from_hash("a")];

    assert!(prev.contains_key(&WidgetId::from_hash("root")));
    assert!(!prev.contains_key(&WidgetId::from_hash("empty")));
    assert_eq!(
        ui.damage_engine
            .prev_paint_rect(WidgetId::from_hash("root")),
        None,
    );
    assert_eq!(
        ui.damage_engine
            .prev_paint_rect(WidgetId::from_hash("a"))
            .unwrap(),
        ui.layout[Layer::Main].rect[frame_node.idx()],
    );
    assert_eq!(
        snap.hash,
        ui.forest.trees[Layer::Main].rollups.node[frame_node.idx()],
    );
}

#[test]
fn prev_frame_drops_disappeared_widgets() {
    let mut ui = Ui::for_test();
    ui.run_at(SURFACE, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("gone"))
                    .label("X")
                    .show(ui);
            });
    });
    assert!(
        ui.damage_engine
            .prev
            .contains_key(&WidgetId::from_hash("gone"))
    );

    ui.run_at(SURFACE, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    assert!(
        !ui.damage_engine
            .prev
            .contains_key(&WidgetId::from_hash("gone"))
    );
}

#[test]
fn prev_frame_updates_on_authoring_change() {
    let mut ui = Ui::for_test();
    let paint = |fill: Color| {
        move |ui: &mut Ui| {
            Frame::new()
                .id(WidgetId::from_hash("a"))
                .size(50.0)
                .background(Background {
                    fill: fill.into(),
                    ..Default::default()
                })
                .show(ui);
        }
    };
    ui.run_at(SURFACE, paint(Color::rgb(0.2, 0.4, 0.8)));
    let h1 = ui.damage_engine.prev[&WidgetId::from_hash("a")].hash;

    ui.run_at(SURFACE, paint(Color::rgb(0.9, 0.4, 0.8)));
    let h2 = ui.damage_engine.prev[&WidgetId::from_hash("a")].hash;
    assert_ne!(h1, h2);
}

/// Per-`WidgetId` text reuse cache: an unchanged Text across frames
/// must hit the cache and skip `TextShaper::measure`. Covers
/// single-line, wrapped, and grid-intrinsic-query paths.
#[test]
fn text_reshape_skipped_when_unchanged() {
    use crate::layout::types::{sizing::Sizing, track::Track};
    use crate::widgets::{grid::Grid, text::Text};

    type Build = fn(&mut Ui);

    let single: Build = |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("the quick brown fox")
                .id(WidgetId::from_hash("hello"))
                .show(ui);
        });
    };
    let wrapped: Build = |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(60.0), Sizing::HUG))
            .show(ui, |ui| {
                Text::new("the quick brown fox jumps over the lazy dog")
                    .id(WidgetId::from_hash("wrapped"))
                    .style(&TextStyle::default().with_font_size(16.0))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
            });
    };
    let grid_intrinsic: Build = |ui| {
        Grid::new()
            .id(WidgetId::from_hash("g"))
            .size((Sizing::fixed(200.0), Sizing::HUG))
            .cols([Track::hug(), Track::fill()])
            .show(ui, |ui| {
                Text::new("label")
                    .id(WidgetId::from_hash("hug-col-text"))
                    .grid_cell((0, 0))
                    .show(ui);
                Text::new("the quick brown fox jumps over the lazy dog")
                    .id(WidgetId::from_hash("fill-col-text"))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .grid_cell((0, 1))
                    .show(ui);
            });
    };

    for (label, build) in [
        ("single-line", single),
        ("wrapped", wrapped),
        ("grid-intrinsic", grid_intrinsic),
    ] {
        let mut ui = Ui::for_test();
        ui.run_at(UVec2::new(400, 200), build);
        let after_first = measure_calls(&ui);
        assert!(
            after_first > 0,
            "{label}: first frame should drive at least one measure call",
        );
        ui.run_at(UVec2::new(400, 200), build);
        let after_second = measure_calls(&ui);
        assert_eq!(
            after_second,
            after_first,
            "{label}: second identical frame must reuse cached TextMeasurement \
             (extra calls: {})",
            after_second - after_first,
        );
    }
}

/// Pin: changing the Text's content invalidates the reuse entry and
/// drives a fresh measure.
#[test]
fn text_reshape_runs_when_content_changes() {
    use crate::widgets::text::Text;

    let render = |content: &'static str| {
        move |ui: &mut Ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Text::new(content)
                    .id(WidgetId::from_hash("changing"))
                    .show(ui);
            });
        }
    };
    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(400, 200), render("first"));
    let before = measure_calls(&ui);
    ui.run_at(UVec2::new(400, 200), render("second"));
    let after = measure_calls(&ui);
    assert!(
        after > before,
        "content change must trigger fresh measure (before={before}, after={after})",
    );
}

/// Pin: when a Text widget disappears from the tree, its `text_reuse`
/// entry is evicted on the same frame.
#[test]
fn text_reuse_evicts_disappeared_widgets() {
    use crate::widgets::text::Text;

    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(400, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("hello")
                .id(WidgetId::from_hash("transient"))
                .show(ui);
        });
    });
    let wid = WidgetId::from_hash("transient");
    assert!(
        ui.layout_engine.text_reuse.has_entry(wid, 0),
        "text widget should populate text_reuse on first render",
    );

    ui.run_at(UVec2::new(400, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |_| {});
    });
    assert!(
        !ui.layout_engine.text_reuse.has_entry(wid, 0),
        "removed widget's reuse entry must be swept",
    );
}

#[test]
fn text_reuse_is_window_local_while_cosmic_buffers_are_shared() {
    use crate::layout::types::sizing::Sizing;
    use crate::text::TextShaper;

    fn text_window(ui: &mut Ui, content: &'static str, width: f32) {
        Panel::vstack()
            .id(WidgetId::from_hash("shared-root"))
            .size((Sizing::fixed(width), Sizing::HUG))
            .show(ui, |ui| {
                Text::new(content)
                    .id(WidgetId::from_hash("shared-text"))
                    .show(ui);
            });
    }

    let shared = HostShared::new(TextShaper::with_bundled_fonts(), None);
    let mut a = ui_with_shared(&shared);
    let mut b = ui_with_shared(&shared);
    let text_id = WidgetId::from_hash("shared-text");

    a.run_at(SURFACE, |ui| text_window(ui, "window A", 120.0));
    let a_key = a.layout[Layer::Main].text_shapes[0].key;
    b.run_at(SURFACE, |ui| text_window(ui, "window B", 120.0));
    let b_key = b.layout[Layer::Main].text_shapes[0].key;

    assert_ne!(a_key, b_key, "different window text needs distinct keys");
    assert!(a.resources.text.has_cosmic_buffer(a_key));
    assert!(a.resources.text.has_cosmic_buffer(b_key));
    assert!(a.layout_engine.text_reuse.has_entry(text_id, 0));
    assert!(b.layout_engine.text_reuse.has_entry(text_id, 0));

    let after_b = a.resources.text.measure_calls();
    a.run_at(SURFACE, |ui| text_window(ui, "window A", 140.0));
    assert_eq!(
        a.resources.text.measure_calls(),
        after_b,
        "window B must not overwrite window A's identity reuse row",
    );

    b.run_at(SURFACE, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("shared-root"))
            .size((Sizing::fixed(120.0), Sizing::HUG))
            .show(ui, |_| {});
    });
    assert!(!b.layout_engine.text_reuse.has_entry(text_id, 0));
    assert!(a.layout_engine.text_reuse.has_entry(text_id, 0));

    let after_b_removal = a.resources.text.measure_calls();
    a.run_at(SURFACE, |ui| text_window(ui, "window A", 160.0));
    assert_eq!(
        a.resources.text.measure_calls(),
        after_b_removal,
        "window B removal must not evict window A's identity reuse row",
    );
    assert!(a.resources.text.has_cosmic_buffer(a_key));
    assert!(a.resources.text.has_cosmic_buffer(b_key));
}

#[test]
fn shared_cache_eviction_restores_idle_windows_paint_only_text() {
    use crate::host::shared::HostShared;
    use crate::layout::types::align::Align;
    use crate::layout::types::sizing::Sizing;
    use crate::renderer::frontend::Frontend;
    use crate::scene::node::Node;
    use crate::scene::tree::paint_anims::PaintAnim;
    use crate::shape::Shape;
    use crate::text::{FontFamily, FontWeight, TextShaper};
    use crate::ui::frame_report::FrameProcessing;

    const HALF: Duration = Duration::from_millis(500);

    fn idle_body(ui: &mut Ui) {
        let mut node = Node::leaf();
        node.size = Some((Sizing::fixed(160.0), Sizing::fixed(30.0)).into());
        ui.widget(node).record(ui, None, |ui| {
            let text = ui.intern("idle interned window text");
            ui.add_shape_animated(
                Shape::Text {
                    local_origin: None,
                    text,
                    color: Color::WHITE,
                    font_size_px: 16.0,
                    line_height_px: 19.2,
                    wrap: TextWrap::SingleLine,
                    align: Align::default(),
                    family: FontFamily::Sans,
                    weight: FontWeight::Regular,
                },
                PaintAnim::BlinkOpacity {
                    half_period: HALF,
                    started_at: HALF,
                },
            );
        });
    }

    let shared = HostShared::new(TextShaper::with_bundled_fonts(), None);
    let mut idle = Ui::new(shared.resources.clone());
    let mut active = Ui::new(shared.resources.clone());
    let display = Display::from_physical(SURFACE, 1.0);

    let idle_first = idle.record_test_frame(display, Duration::ZERO, idle_body);
    assert_eq!(idle_first.repaint_after, Some(HALF));
    let idle_key = idle.layout[Layer::Main].text_shapes[0].key;

    active.run_at(SURFACE, |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("active window one").auto_id().show(ui);
            Text::new("active window two").auto_id().show(ui);
        });
    });
    idle.resources.text.evict_cosmic_buffers(1);
    assert!(
        !idle.resources.text.has_cosmic_buffer(idle_key),
        "newer active-window churn must evict the idle window's key",
    );

    let idle_paint = idle.record_test_frame(display, HALF, |_| {
        panic!("PaintOnly must retain the idle window's prior tree")
    });
    assert_eq!(idle_paint.processing, FrameProcessing::PaintOnly);
    let plan = idle_paint
        .plan
        .expect("the animated text boundary must produce a paint plan");
    assert!(!idle.resources.text.has_cosmic_buffer(idle_key));

    let mut frontend = Frontend::for_test();
    frontend.build(idle.frame_scene(), plan);
    assert!(
        frontend.buffer.texts.iter().any(|run| run.key == idle_key),
        "PaintOnly must emit the retained text run",
    );
    assert!(
        idle.resources.text.has_cosmic_buffer(idle_key),
        "encoder must restore the idle window's evicted interned text",
    );
}

/// Pin: when authoring is unchanged but the wrap target (parent's
/// available width) shifts between frames, the cached *unbounded* shape
/// is preserved — only the *wrap* reshape runs again.
#[test]
fn wrap_target_change_preserves_unbounded_cache() {
    use crate::layout::types::sizing::Sizing;
    use crate::widgets::text::Text;

    let render = |slot_w: f32| {
        move |ui: &mut Ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::fixed(slot_w), Sizing::HUG))
                .show(ui, |ui| {
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .id(WidgetId::from_hash("p"))
                        .style(&TextStyle::default().with_font_size(16.0))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui);
                });
        }
    };

    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(400, 200), render(60.0));
    let after_first = measure_calls(&ui);
    assert!(
        after_first >= 2,
        "first frame should measure both unbounded and wrap (got {after_first})",
    );
    ui.run_at(UVec2::new(400, 200), render(80.0));
    let after_second = measure_calls(&ui);
    let delta = after_second - after_first;
    assert_eq!(
        delta, 1,
        "wrap-target change must reshape only the wrap path, not unbounded \
         (extra calls: {delta})",
    );
}

#[test]
fn state_map_persists_and_evicts_with_recorded_ids() {
    let mut ui = Ui::for_test_at(UVec2::new(100, 100));
    let id_a = WidgetId::from_hash("a");
    let id_b = WidgetId::from_hash("b");

    ui.run_at(UVec2::new(100, 100), |ui| {
        Frame::new().id(WidgetId::from_hash("a")).show(ui);
        Frame::new().id(WidgetId::from_hash("b")).show(ui);
        *ui.state_mut::<u32>(id_a) = 11;
        *ui.state_mut::<u32>(id_b) = 22;
    });
    ui.run_at(UVec2::new(100, 100), |ui| {
        Frame::new().id(WidgetId::from_hash("a")).show(ui);
        // Reading state during recording so the row is touched while
        // its widget is still seen.
        assert_eq!(*ui.state_mut::<u32>(id_a), 11);
    });
    ui.run_at(UVec2::new(100, 100), |ui| {
        Frame::new().id(WidgetId::from_hash("b")).show(ui);
        assert_eq!(
            *ui.state_mut::<u32>(id_b),
            0,
            "B was unrecorded last frame; its row should have been swept",
        );
    });
}

/// `Ui::frame` re-records when the frame contained routed input that could
/// drive a state mutation, and runs the build closure exactly once otherwise.
/// Action coverage has to be exact: false positives waste CPU silently, false
/// negatives leave the popup-dismissal class of bugs unfixed.
#[test]
fn frame_pass_count_matches_action_trigger() {
    use crate::input::InputEvent;
    use crate::input::keyboard::{Key, Modifiers};
    use crate::input::pointer::PointerButton;
    use crate::input::sense::Sense;
    use crate::layout::types::sizing::Sizing;
    use glam::Vec2;
    use std::cell::Cell;

    fn build_target(ui: &mut Ui) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .sense(Sense::CLICK)
            .focusable(true)
            .show(ui, |_| {});
    }

    let display = Display::from_physical(UVec2::new(100, 100), 1.0);
    type Prime = fn(&mut Ui);
    let cases: &[(&str, Prime, usize)] = &[
        ("idle", |_ui| {}, 1),
        (
            "hover only",
            |ui| {
                ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0)));
            },
            1,
        ),
        (
            "modifiers only",
            |ui| {
                ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE));
            },
            1,
        ),
        (
            "routed click",
            |ui| {
                ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0)));
                ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
                ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
            },
            2,
        ),
        (
            "unrouted click",
            |ui| {
                ui.on_input(InputEvent::PointerMoved(Vec2::new(150.0, 150.0)));
                ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
                ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
            },
            1,
        ),
        (
            "unrouted keydown",
            |ui| {
                ui.on_input(InputEvent::KeyDown {
                    key: Key::Enter,
                    repeat: false,
                    physical: Key::Other,
                });
            },
            1,
        ),
        (
            "routed keydown",
            |ui| {
                ui.request_focus(Some(WidgetId::from_hash("root")));
                ui.on_input(InputEvent::KeyDown {
                    key: Key::Enter,
                    repeat: false,
                    physical: Key::Other,
                });
            },
            2,
        ),
        (
            "scroll",
            |ui| {
                ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 10.0)));
            },
            1,
        ),
    ];

    for (label, prime, expected) in cases {
        let mut ui = Ui::for_test();
        // Baseline frame so the under-test `frame` diffs against a real
        // prior recording, not the never-painted initial state.
        ui.run_at(UVec2::new(100, 100), build_target);
        prime(&mut ui);

        let count = Cell::new(0u32);
        let frame_id_before = ui.frame_runtime.frame_id;
        let _ = ui.record_test_frame_without_baseline(display, Duration::ZERO, |ui| {
            count.set(count.get() + 1);
            build_target(ui);
        });
        assert_eq!(
            count.get() as usize,
            *expected,
            "{label}: expected {expected} build invocation(s), got {}",
            count.get(),
        );
        // frame_id must bump exactly once per `frame` regardless of
        // pass count — pass B's anim ticks must see the same id as
        // pass A's so the integrator doesn't double-advance.
        assert_eq!(
            ui.frame_runtime.frame_id,
            frame_id_before + 1,
            "{label}: frame_id must bump exactly once per frame (passes: {expected})",
        );
    }
}

/// A routed action requests pass B, but its edge is visible only in pass A.
/// This lets application code handle a widget action inline without replaying
/// the effect.
#[test]
fn action_effect_runs_once_across_record_replay() {
    use crate::input::pointer::PointerButton;

    let mut ui = Ui::for_test();
    let surface = UVec2::new(100, 100);
    let display = Display::from_physical(surface, 1.0);
    let build = |ui: &mut Ui| {
        Button::new()
            .id(WidgetId::from_hash("action"))
            .label("Run")
            .size((100.0, 100.0))
            .show(ui)
            .left
            .clicked()
    };

    ui.run_at(surface, |ui| {
        let _ = build(ui);
    });
    ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    let mut passes = 0;
    let mut effects = 0;
    let _ = ui.record_test_frame_without_baseline(display, Duration::from_millis(16), |ui| {
        passes += 1;
        if build(ui) {
            effects += 1;
        }
    });

    assert_eq!(passes, 2, "action input must request a replay pass");
    assert_eq!(effects, 1, "the action edge must not replay");
}

/// `Ui::frame` plumbs `now`, `dt`, and the repaint-requested flag
/// end-to-end: per-call `now` lands in the frame runtime, the derived `dt`
/// clamps to `MAX_DT`, `repaint_requested` resets at the top of every
/// call, and a flag set during recording surfaces on `FrameOutput`.
#[test]
fn frame_plumbs_now_dt_and_repaint_request() {
    const MAX_DT: f32 = FrameRuntime::MAX_DT;
    let display = Display::from_physical(UVec2::new(100, 100), 1.0);

    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(100, 100), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });

    // Frame A: idle, no repaint request, now = 16ms.
    let repaint = ui
        .record_test_frame_without_baseline(display, Duration::from_millis(16), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
        })
        .repaint_requested;
    assert!(
        !repaint,
        "no animate-not-settled flag set — must stay false"
    );
    assert_eq!(ui.frame_runtime.time, Duration::from_millis(16));
    assert!(
        (ui.frame_runtime.dt - 0.016).abs() < 1e-6,
        "FrameRuntime::dt should be (now - prev) in seconds; got {}",
        ui.frame_runtime.dt,
    );

    // Frame B: simulate an unsettled animation tick by setting the
    // internal flag during recording. The flag must reach `FrameOutput`.
    let repaint = ui
        .record_test_frame_without_baseline(display, Duration::from_millis(32), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
            ui.frame_runtime.repaint_requested = true;
        })
        .repaint_requested;
    assert!(
        repaint,
        "repaint_requested set during recording must surface on FrameOutput",
    );
    assert_eq!(ui.frame_runtime.time, Duration::from_millis(32));
    assert!(
        (ui.frame_runtime.dt - 0.016).abs() < 1e-6,
        "FrameRuntime::dt should be next-frame delta; got {}",
        ui.frame_runtime.dt,
    );

    // Frame C: oversized gap (5s) clamps dt to MAX_DT; `time` still
    // tracks true clock so animation math doesn't teleport.
    let _ = ui.record_test_frame_without_baseline(display, Duration::from_millis(5_032), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    assert_eq!(ui.frame_runtime.time, Duration::from_millis(5_032));
    assert!(
        (ui.frame_runtime.dt - MAX_DT).abs() < 1e-6,
        "FrameRuntime::dt should clamp at MAX_DT; got {}",
        ui.frame_runtime.dt,
    );

    // Frame D: prior frame's repaint_requested must NOT leak — resets
    // at the top of every `frame` regardless of pass count.
    let repaint = ui
        .record_test_frame_without_baseline(display, Duration::from_millis(5_048), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
        })
        .repaint_requested;
    assert!(
        !repaint,
        "repaint_requested must reset at the top of frame()",
    );
}

/// Pin: enabling `frame_stats` records a Debug-layer text widget,
/// keeps damage `Partial` (not `Full`) on an otherwise-static scene,
/// and updates `fps_ema` once two frames have elapsed.
#[test]
fn frame_stats_overlay_records_partial_damage() {
    let mut ui = Ui::for_test();
    ui.debug_overlay_mut().frame_stats = true;
    let display = Display::from_physical(SURFACE, 1.0);

    // Warm-up frame at t = 0. `fps_ema` stays zero (no prior `time` to
    // diff against), but the Debug layer should already carry the
    // readout.
    ui.record_test_frame(display, Duration::ZERO, |ui| {
        Frame::new()
            .id(WidgetId::from_hash("body"))
            .size(50.0)
            .show(ui);
    });
    assert_eq!(ui.frame_runtime.fps_ema, 0.0);
    assert!(
        !ui.forest.trees[Layer::Debug].records.is_empty(),
        "Debug layer must carry the frame_stats readout",
    );

    // Second frame at t = 16ms. Main scene is unchanged; only the
    // Debug-layer readout dirties → expect `Partial`, not `Full`,
    // and not `None` either. `fps_ema` picks up its first instantaneous
    // reading (~62.5).
    let report = ui.record_test_frame(display, Duration::from_millis(16), |ui| {
        Frame::new()
            .id(WidgetId::from_hash("body"))
            .size(50.0)
            .show(ui);
    });
    assert!(
        matches!(
            report.plan,
            Some(RenderPlan {
                kind: RenderKind::Partial { .. },
                ..
            })
        ),
        "frame_stats should produce Partial damage on a static scene; got {:?}",
        report.plan,
    );
    assert!(
        ui.frame_runtime.fps_ema > 0.0,
        "fps_ema must update after the second frame; got {}",
        ui.frame_runtime.fps_ema,
    );

    // Disabling the flag mid-stream evicts the Debug-layer node next
    // frame.
    ui.debug_overlay_mut().frame_stats = false;
    ui.record_test_frame_without_baseline(display, Duration::from_millis(32), |ui| {
        Frame::new()
            .id(WidgetId::from_hash("body"))
            .size(50.0)
            .show(ui);
    });
    assert!(
        ui.forest.trees[Layer::Debug].records.is_empty(),
        "Debug layer must clear once frame_stats is turned off",
    );
}

/// Multiple distinct deadlines coexist in the queue and surface
/// in ascending order; each fires independently on a frame at or
/// past its deadline.
#[test]
fn request_repaint_after_queues_distinct_deadlines() {
    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);
    let report =
        ui.record_test_frame_without_baseline(display, Duration::from_secs_f32(0.0), |ui| {
            ui.request_repaint_after(Duration::from_secs_f32(0.5));
            ui.request_repaint_after(Duration::from_secs_f32(1.5));
        });
    // Earliest deadline wins the report slot.
    assert_eq!(
        report.repaint_after,
        Some(Duration::from_secs_f32(0.5)),
        "FrameReport must surface the earliest pending wake",
    );
    // Both entries are still queued (neither has fired).
    assert_eq!(
        ui.frame_runtime.repaint_wakes.len(),
        2,
        "both distinct deadlines stay queued"
    );

    // Run a frame at the first deadline. The earliest entry drains;
    // the second survives.
    let report =
        ui.record_test_frame_without_baseline(display, Duration::from_secs_f32(0.5), |_| {});
    assert_eq!(
        report.repaint_after,
        Some(Duration::from_secs_f32(1.5)),
        "second deadline survives the first frame's drain",
    );
    assert_eq!(ui.frame_runtime.repaint_wakes.len(), 1);

    // Run a frame at the second deadline. Queue empties.
    let report =
        ui.record_test_frame_without_baseline(display, Duration::from_secs_f32(1.5), |_| {});
    assert_eq!(report.repaint_after, None);
    assert!(ui.frame_runtime.repaint_wakes.is_empty());
}

/// Re-requesting an already-queued deadline within the same frame
/// is a no-op — the queue is sorted + dedup'd. Near-duplicates within
/// `DEFAULT_REPAINT_COALESCE_DT` (1/120 s, the headless default)
/// collapse onto the later wake to minimize host wake-ups; entries
/// spaced beyond the window stay distinct.
#[test]
fn request_repaint_after_dedups_within_frame() {
    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);
    ui.record_test_frame_without_baseline(display, Duration::from_secs_f32(0.0), |ui| {
        for _ in 0..10 {
            ui.request_repaint_after(Duration::from_secs_f32(0.5));
        }
        ui.request_repaint_after(Duration::from_secs_f32(0.5));
    });
    assert_eq!(
        ui.frame_runtime.repaint_wakes.len(),
        1,
        "exact duplicate deadlines collapse to one entry",
    );

    // Near-duplicates within the 1/120 s window collapse onto the
    // later deadline (prefer the longer wait); deadlines spaced
    // beyond the window stay distinct.
    let mut ui = Ui::for_test();
    ui.record_test_frame_without_baseline(display, Duration::from_secs_f32(0.0), |ui| {
        // Earlier request first; second request lands ~4 ms later
        // (well under 1/120 s ≈ 8.33 ms). Expect the later deadline
        // to win.
        ui.request_repaint_after(Duration::from_secs_f32(0.500));
        ui.request_repaint_after(Duration::from_secs_f32(0.504));
        // Reversed order — later first, then a near-earlier
        // request. Existing later wake should suppress the earlier
        // one (same outcome: only the later survives).
        ui.request_repaint_after(Duration::from_secs_f32(0.512));
        ui.request_repaint_after(Duration::from_secs_f32(0.508));
        // Beyond the window — must stay distinct.
        ui.request_repaint_after(Duration::from_secs_f32(0.600));
    });
    let deadlines: Vec<Duration> = ui
        .frame_runtime
        .repaint_wakes
        .iter()
        .map(|w| w.deadline)
        .collect();
    assert_eq!(
        deadlines,
        vec![
            Duration::from_secs_f32(0.512),
            Duration::from_secs_f32(0.600),
        ],
        "near-duplicate wakes collapse onto the later deadline",
    );
}

/// The coalesce floor tracks `Display::refresh_millihertz`: two wakes
/// 12 ms apart stay distinct at the unknown-rate 120 Hz fallback
/// (≈8.33 ms window) but collapse at 60 Hz (≈16.67 ms window),
/// proving the floor is derived from the display in `schedule_wake`.
#[test]
fn coalesce_floor_follows_refresh_rate() {
    let schedule_pair = |ui: &mut Ui, display: Display| {
        ui.record_test_frame_without_baseline(display, Duration::ZERO, |ui| {
            ui.request_repaint_after(Duration::from_millis(500));
            ui.request_repaint_after(Duration::from_millis(512));
        });
    };

    // Unknown refresh → 120 Hz fallback: 12 ms > 8.33 ms → distinct.
    let mut ui = Ui::for_test();
    schedule_pair(&mut ui, Display::from_physical(SURFACE, 1.0));
    assert_eq!(
        ui.frame_runtime.repaint_wakes.len(),
        2,
        "120 Hz fallback: 12 ms-apart wakes stay distinct",
    );

    // 60 Hz refresh → 16.67 ms window: 12 ms < window → collapse.
    let mut ui = Ui::for_test();
    let display_60 = Display {
        refresh_millihertz: Some(60_000),
        ..Display::from_physical(SURFACE, 1.0)
    };
    schedule_pair(&mut ui, display_60);
    assert_eq!(
        ui.frame_runtime.repaint_wakes.len(),
        1,
        "60 Hz floor: 12 ms-apart wakes collapse",
    );
    assert_eq!(
        ui.frame_runtime.repaint_wakes[0].deadline,
        Duration::from_millis(512),
        "the later deadline survives the collapse",
    );
}

/// Entries with `deadline <= now` drain at the top of the next
/// frame; entries strictly past `now` survive.
#[test]
fn request_repaint_after_drains_fired_entries() {
    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);
    ui.record_test_frame_without_baseline(display, Duration::from_secs_f32(0.0), |ui| {
        ui.request_repaint_after(Duration::from_secs_f32(0.5));
        ui.request_repaint_after(Duration::from_secs_f32(1.0));
        ui.request_repaint_after(Duration::from_secs_f32(2.0));
    });
    assert_eq!(ui.frame_runtime.repaint_wakes.len(), 3);

    // Frame at t=1.0 drains entries at 0.5 and 1.0; 2.0 survives.
    let report =
        ui.record_test_frame_without_baseline(display, Duration::from_secs_f32(1.0), |_| {});
    assert_eq!(ui.frame_runtime.repaint_wakes.len(), 1);
    assert_eq!(report.repaint_after, Some(Duration::from_secs_f32(2.0)));
}

// `app_state_round_trip_across_frame` and `app_without_install_panics`
// were removed when `Ui` lost its `<T>` parameter. App-owned state now
// lives in the caller's frame-builder closure (capture it) — see the
// `app_state` showcase for the canonical pattern.

/// Anim-only fast path: when the only wake fired is a paint-anim
/// quantum boundary (no input, no `request_repaint`, no real wake),
/// `Ui::frame` skips record + post-record and emits
/// `FrameProcessing::PaintOnly`.
#[test]
fn paint_only_fast_path_fires_on_anim_quantum_boundary() {
    use crate::ui::frame_report::FrameProcessing;

    let half = Duration::from_millis(500);

    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Frame::new()
                .id(WidgetId::from_hash("blinker"))
                .size(20.0)
                .show(ui);
            add_blink_shape(ui, half);
        });
    }

    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);

    // Frame 0: record. Full path; schedules anim wake at `half`.
    let r0 = ui.record_test_frame(display, Duration::ZERO, |ui| body(ui, half));
    assert_eq!(r0.processing, FrameProcessing::SingleLayout);
    assert_eq!(r0.repaint_after, Some(half));

    // Frame 1 at the blink boundary: only anim wake fires → fast path.
    let r1 = ui.record_test_frame(display, half, |ui| body(ui, half));
    assert_eq!(r1.processing, FrameProcessing::PaintOnly);

    // PaintOnly must emit a Partial damage plan covering the anim's
    // tight rect — not Full (defeats the point) and not None (the
    // blink phase actually flipped). Pin both invariants.
    match r1.plan {
        Some(RenderPlan {
            kind: RenderKind::Partial { region },
            ..
        }) => {
            let rects: Vec<_> = region.iter_rects().collect();
            assert_eq!(rects.len(), 1, "expected single damage rect, got {rects:?}");
            let r = rects[0];
            assert!(
                r.size.w <= 8.0 && r.size.h <= 16.0,
                "PaintOnly damage should be the anim's tight rect, got {r:?}",
            );
        }
        other => panic!("expected RenderPlan::Partial on PaintOnly, got {other:?}"),
    }
    // Bug regression: PaintOnly skips post_record, but must still
    // re-fold the retained paint_anims so the *next* blink boundary
    // is queued. Without this fold the caret stops blinking until
    // input forces a FullRecord (mouse-move regression).
    assert_eq!(r1.repaint_after, Some(half + half));
    let r2 = ui.record_test_frame(display, half + half, |ui| body(ui, half));
    assert_eq!(r2.processing, FrameProcessing::PaintOnly);

    // A pending OS close request vetoes the fast path: the app can only
    // read `close_requested` (and veto via `keep_open`) during record,
    // so an anim-wake frame escalates to Full while `wants_close` is
    // set — and drops back to PaintOnly once it clears.
    ui.window_frame.close_requested = true;
    let r3 = ui.record_test_frame(display, half * 3, |ui| body(ui, half));
    assert_eq!(r3.processing, FrameProcessing::SingleLayout);
    ui.window_frame.close_requested = false;
    let r4 = ui.record_test_frame(display, half * 4, |ui| body(ui, half));
    assert_eq!(r4.processing, FrameProcessing::PaintOnly);
}

#[test]
fn widget_text_inputs_lower_exact_bytes_and_hashes() {
    use crate::common::hash::hash_str;
    use crate::scene::shapes::record::ShapeRecord;

    let mut ui = Ui::for_test();
    ui.run_at(SURFACE, |ui| {
        let borrowed = String::from("borrowed");
        Text::new(borrowed.as_str())
            .id(WidgetId::from_hash("borrowed"))
            .show(ui);
        Text::new(String::from("owned"))
            .id(WidgetId::from_hash("owned"))
            .show(ui);
        let owned_interned = ui.intern(String::from("owned interned"));
        Text::new(owned_interned)
            .id(WidgetId::from_hash("owned-interned"))
            .show(ui);
        let interned = ui.intern("interned");
        let interned = ui.intern(interned);
        Text::new(interned)
            .id(WidgetId::from_hash("interned"))
            .show(ui);
        let formatted = ui.fmt(format_args!("formatted {}", 7));
        Text::new(formatted)
            .id(WidgetId::from_hash("formatted"))
            .show(ui);
    });

    let payloads = ui.forest.record_store.payloads.borrow();
    let text_bytes = payloads.text_bytes();
    assert_eq!(
        &*text_bytes,
        "borrowedownedowned internedinternedformatted 7"
    );
    let records = &ui.forest.trees[Layer::Main].shapes.records;
    assert_eq!(records.len(), 5);
    for (record, expected) in records.iter().zip([
        "borrowed",
        "owned",
        "owned interned",
        "interned",
        "formatted 7",
    ]) {
        match record {
            ShapeRecord::Text { text, .. } => {
                let resolved = text.resolve(&text_bytes);
                assert_eq!(resolved.text, expected);
                assert_eq!(resolved.hash, hash_str(expected));
            }
            shape => panic!("expected text shape, got {shape:?}"),
        }
    }
}

#[test]
fn retained_arena_text_preserves_bytes_and_hash_across_record_stores() {
    use crate::common::hash::hash_str;
    use crate::scene::shapes::record::ShapeRecord;
    use std::rc::Rc;

    fn assert_recorded_text(ui: &Ui, expected: &str) {
        let payloads = ui.forest.record_store.payloads.borrow();
        let text_bytes = payloads.text_bytes();
        let records = &ui.forest.trees[Layer::Main].shapes.records;
        let [ShapeRecord::Text { text, .. }] = records.as_slice() else {
            panic!("expected one text shape, got {records:?}");
        };
        let resolved = text.resolve(&text_bytes);
        assert_eq!(resolved.text, expected);
        assert_eq!(resolved.hash, hash_str(expected));
    }

    for (original, replacement) in [
        ("first", "other"),
        ("longer retained text", "x"),
        ("x", "longer replacement text"),
    ] {
        let mut ui = Ui::default();
        let mut retained = None;
        let mut pass = 0;

        ui.run_at_without_baseline(SURFACE, |ui| {
            pass += 1;
            if pass == 1 {
                let label = ui.intern(original);
                retained = Some(label.clone());
                Text::new(label)
                    .id(WidgetId::from_hash("retained-arena"))
                    .show(ui);
            } else {
                assert_eq!(pass, 2, "cold first frame must record exactly twice");
                let _replacement = ui.intern(replacement);
                Text::new(retained.as_ref().unwrap().clone())
                    .id(WidgetId::from_hash("retained-arena"))
                    .show(ui);
            }
        });

        assert_eq!(pass, 2);
        assert_recorded_text(&ui, original);
    }

    let mut source = Ui::for_test();
    let mut retained = None;
    source.run_at(SURFACE, |ui| {
        let label = ui.intern("source window");
        retained = Some(label.clone());
        Text::new(label)
            .id(WidgetId::from_hash("source-window"))
            .show(ui);
    });

    let mut destination = Ui::for_test();
    destination.run_at(SURFACE, |ui| {
        let _replacement = ui.intern("destination");
        Text::new(retained.as_ref().unwrap().clone())
            .id(WidgetId::from_hash("cross-window"))
            .show(ui);
    });
    assert_recorded_text(&destination, "source window");

    let mut ui = Ui::for_test();
    let mut retained = None;
    let mut arena_ptrs = Vec::new();
    for content in ["first arena", "second arena", "first arena reused"] {
        ui.run_at(SURFACE, |ui| {
            let label = ui.intern(content);
            arena_ptrs.push(Rc::as_ptr(&label.arena));
            drop(retained.replace(label.clone()));
            Text::new(label)
                .id(WidgetId::from_hash("arena-recycling"))
                .show(ui);
        });
    }
    assert_ne!(arena_ptrs[0], arena_ptrs[1]);
    assert_eq!(
        arena_ptrs[0], arena_ptrs[2],
        "the spare arena must be recycled after its retained handle drops",
    );
}

/// Regression: `Ui::frame` used to clear `record_store` unconditionally
/// at entry, including on `PaintOnly` frames. But on PaintOnly the
/// record pass is skipped, so `tree.shapes` retains last frame's
/// `ShapeRecord`s — which reference record payloads by index
/// (`ShapeBrush::Gradient(id)`, polyline/mesh spans, arena-backed text
/// spans). Clearing left those indices dangling; the encoder then
/// panicked on the first gradient lookup with
/// `index out of bounds: the len is 0 but the index is N`.
/// Fix: clear inside `record_pass` instead (only fires when we're
/// rebuilding shapes). This test pins it with retained gradient and
/// recorded text entries plus an animated shape that forces
/// PaintOnly on frame 1, then re-runs the encoder.
#[test]
fn paint_only_preserves_record_store_for_retained_shapes() {
    use crate::primitives::brush::Brush;
    use crate::primitives::brush::gradient::linear::LinearGradient;
    use crate::ui::frame_report::FrameProcessing;

    let half = Duration::from_millis(500);

    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            // Gradient-filled chrome: `lower::background` interns a
            // `RecordedGradient` into `RecordPayloads::gradients` each record
            // pass, and the resulting `ChromeRow` stores the index.
            Frame::new()
                .id(WidgetId::from_hash("grad_bg"))
                .size(50.0)
                .background(Background {
                    fill: Brush::Linear(LinearGradient::two_stop(
                        0.0,
                        Color::rgb(1.0, 0.0, 0.0),
                        Color::rgb(0.0, 0.0, 1.0),
                    )),
                    ..Default::default()
                })
                .show(ui);
            let label = ui.fmt(format_args!("retained {}", 7));
            Text::new(label)
                .id(WidgetId::from_hash("retained-text"))
                .show(ui);
            // Animated shape, drives the PaintOnly wake on frame 1.
            add_blink_shape(ui, half);
        });
    }

    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);

    // Frame 0: full record. Populates the gradient payloads and stamps
    // `ShapeBrush::Gradient(GradientId(0))` into the chrome row for the frame.
    let r0 = ui.record_test_frame(display, Duration::ZERO, |ui| body(ui, half));
    assert_eq!(r0.processing, FrameProcessing::SingleLayout);
    {
        let payloads = ui.forest.record_store.payloads.borrow();
        assert_eq!(&*payloads.text_bytes(), "retained 7");
    }

    // Frame 1 at the blink boundary: only the anim wake fires →
    // PaintOnly. With the old (buggy) clear, the gradient payloads
    // would be empty here and the encoder below would panic.
    let r1 = ui.record_test_frame(display, half, |ui| body(ui, half));
    assert_eq!(r1.processing, FrameProcessing::PaintOnly);

    // Direct pin: the gradient interned during frame 0's record must
    // still be live for the encoder on a PaintOnly frame.
    assert_eq!(
        ui.forest
            .record_store
            .payloads
            .borrow()
            .gradients
            .records
            .len(),
        1,
        "PaintOnly must preserve gradient payloads so retained \
         ShapeBrush::Gradient indices remain valid",
    );
    {
        let payloads = ui.forest.record_store.payloads.borrow();
        assert_eq!(
            &*payloads.text_bytes(),
            "retained 7",
            "PaintOnly must preserve bytes referenced by retained text",
        );
    }

    // Indirect pin: re-run the encoder against the retained tree
    // + record store. With the bug, this panicked on `gradients[id]`.
    let _ = ui.encode_cmds();
}

#[test]
fn paint_only_reresolves_gradient_after_other_window_evicts_its_row() {
    use crate::primitives::brush::gradient::linear::LinearGradient;
    use crate::primitives::color::ColorU8;

    use crate::primitives::fill_wire::LutRow;

    use crate::renderer::frontend::cmd_buffer::Command;
    use crate::renderer::frontend::encoder;
    use crate::renderer::gradient_atlas::ATLAS_ROWS;
    use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;
    use crate::shape::Shape;
    use crate::text::TextShaper;
    use crate::ui::frame_report::FrameProcessing;
    use std::collections::HashSet;

    fn rows(ui: &Ui, atlas: &SharedGradientAtlas) -> Vec<LutRow> {
        let plan = RenderPlan {
            clear: ui.theme.window_clear,
            kind: RenderKind::Full,
        };
        encoder::test_support::encode(ui.frame_scene(), atlas, plan)
            .iter()
            .filter_map(|command| match command {
                Command::DrawRect(payload) if payload.fill_lut_row != LutRow::FALLBACK => {
                    Some(payload.fill_lut_row)
                }
                _ => None,
            })
            .collect()
    }

    fn window_a(ui: &mut Ui, half: Duration) {
        Panel::hstack().size(20.0).show(ui, |ui| {
            ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(
                LinearGradient::two_stop(0.0, ColorU8::rgb(255, 0, 0), ColorU8::rgb(0, 0, 255)),
            ));
            add_blink_shape(ui, half);
        });
    }

    let shared = HostShared::new(TextShaper::mono(), None);
    let atlas = shared.gradient_atlas.clone();
    let mut a = ui_with_shared(&shared);
    let mut b = ui_with_shared(&shared);
    let half = Duration::from_millis(500);

    a.run_at(SURFACE, |ui| window_a(ui, half));
    let original_row = rows(&a, &atlas)[0];
    atlas.flush_with(|_| ());

    b.run_at_without_baseline(SURFACE, |ui| {
        Panel::hstack().size(20.0).show(ui, |ui| {
            for index in 0..ATLAS_ROWS - 1 {
                ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(
                    LinearGradient::two_stop(
                        0.0,
                        ColorU8::rgb(
                            index as u8,
                            (index >> u8::BITS) as u8,
                            (index >> (u8::BITS * 2)) as u8,
                        ),
                        ColorU8::WHITE,
                    ),
                ));
            }
        });
    });
    let b_rows: HashSet<LutRow> = rows(&b, &atlas).into_iter().collect();
    assert_eq!(b_rows.len(), (ATLAS_ROWS - 1) as usize);
    assert!(b_rows.contains(&original_row));
    atlas.flush_with(|_| ());

    let report = a.record_test_frame(Display::from_physical(SURFACE, 1.0), half, |ui| {
        window_a(ui, half)
    });
    assert_eq!(report.processing, FrameProcessing::PaintOnly);
    let resolved_row = rows(&a, &atlas)[0];
    assert_ne!(
        resolved_row, original_row,
        "PaintOnly must resolve retained gradient content after its old row is reused",
    );
}

/// `request_repaint` co-firing with an anim wake produces the
/// `REAL | ANIM` mix, so the classifier picks Full.
#[test]
fn paint_only_skipped_when_widget_requested_repaint() {
    use crate::ui::frame_report::FrameProcessing;

    let half = Duration::from_millis(500);

    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Frame::new()
                .id(WidgetId::from_hash("blinker"))
                .size(20.0)
                .show(ui);
            add_blink_shape(ui, half);
        });
    }

    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);

    // Frame 0: record + `request_repaint`. Next frame must be Full.
    let r0 = ui.record_test_frame(display, Duration::ZERO, |ui| {
        body(ui, half);
        ui.request_repaint();
    });
    assert!(r0.repaint_requested);

    let r1 = ui.record_test_frame_without_baseline(display, half, |ui| body(ui, half));
    assert_eq!(r1.processing, FrameProcessing::SingleLayout);
}

/// At an anim-only wake boundary, the classifier picks `PaintOnly`.
/// Under `InputPolicy::OnDelta` (default) an inert pointer move
/// since the last frame doesn't disqualify it — `requests_repaint`
/// stayed `false`. Under `InputPolicy::Always` the same input
/// upgrades the frame to `SingleLayout`.
///
/// Action input (click / key / IME) is unconditionally upgraded
/// under both policies because `on_input` returns
/// `requests_repaint = true` for them — exercised in the second
/// half of the test.
#[test]
fn input_policy_routes_paint_only_gate() {
    use crate::input::InputEvent;
    use crate::input::keyboard::Key;
    use crate::input::policy::InputPolicy;
    use crate::ui::frame_report::FrameProcessing;
    use glam::Vec2;

    let display = Display::from_physical(UVec2::new(100, 100), 1.0);
    let half = Duration::from_millis(500);

    // Body declares an inert Frame *and* an anim shape so the next
    // frame's wake fires `ANIM`. Pointer-over-inert hits no Sense
    // entry, so OnDelta sees `requests_repaint = false`.
    fn body(ui: &mut Ui, half: Duration) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("inert"))
                    .size(80.0)
                    .show(ui);
                add_blink_shape(ui, half);
            });
    }

    {
        let mut ui = Ui::for_test();
        ui.input_policy = InputPolicy::OnDelta;
        let r0 = ui.record_test_frame(display, Duration::ZERO, |ui| body(ui, half));
        assert_eq!(r0.processing, FrameProcessing::SingleLayout);

        ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
        assert!(
            ui.input.had_input_since_last_frame,
            "had_input set after any event (precondition)",
        );
        assert!(
            !ui.input.repaint_requested_since_last_frame,
            "inert pointer move must not flip repaint_requested",
        );

        let r1 = ui.record_test_frame(display, half, |ui| body(ui, half));
        assert_eq!(
            r1.processing,
            FrameProcessing::PaintOnly,
            "OnDelta + inert pointer move + anim wake → PaintOnly",
        );

        // PaintOnly path must have drained input sticky bits and queues.
        assert!(!ui.input.had_input_since_last_frame);
        assert!(!ui.input.repaint_requested_since_last_frame);
    }

    {
        let mut ui = Ui::for_test();
        ui.input_policy = InputPolicy::Always;
        let _ = ui.record_test_frame(display, Duration::ZERO, |ui| body(ui, half));

        ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
        let r1 = ui.record_test_frame(display, half, |ui| body(ui, half));
        assert_eq!(
            r1.processing,
            FrameProcessing::SingleLayout,
            "Always + any input forces SingleLayout",
        );
    }

    // only with focus or a chord subscriber, so prime focus first.
    {
        use crate::primitives::widget_id::WidgetId;
        let mut ui = Ui::for_test();
        ui.input_policy = InputPolicy::OnDelta;
        let _ = ui.record_test_frame(display, Duration::ZERO, |ui| body(ui, half));
        ui.input.focused = Some(WidgetId::from_hash("editor"));

        ui.on_input(InputEvent::KeyDown {
            key: Key::Enter,
            repeat: false,
            physical: Key::Other,
        });
        assert!(
            ui.input.repaint_requested_since_last_frame,
            "KeyDown with focus held must flip repaint_requested",
        );
        let r1 = ui.record_test_frame(display, half, |ui| body(ui, half));
        assert_ne!(
            r1.processing,
            FrameProcessing::PaintOnly,
            "OnDelta must not pick PaintOnly on action input",
        );
    }
}

//
// Pin the first-frame behavior added to `Ui::frame`: when the
// recorder has never run before, do a blackout record pass (input
// swapped for `InputState::default()`) to build the cascade, then
// re-route the held `pointer_pos` against it before the user-visible
// pass. Tests below intentionally use `Ui::default()` to exercise true
// cold-start; `Ui::for_test()` pre-marks the recorder warm to keep the
// rest of the test suite on single-record semantics.

const COLD: UVec2 = UVec2::new(200, 200);

fn cold_ui() -> Ui {
    Ui::default()
}

fn cold_frame(ui: &mut Ui, record: impl FnMut(&mut Ui)) {
    let display = Display::from_physical(COLD, 1.0);
    let _ = ui.record_test_frame(display, Duration::ZERO, record);
}

/// On a true first frame the user closure runs **twice** — once for the
/// blackout warmup pass, once for the real pass. The second frame runs
/// it once. The existing `double_layout` arm fires when an input action
/// or a `request_relayout` lands; warmup is the only third trigger.
#[test]
fn cold_start_runs_record_closure_twice_on_first_frame() {
    let mut ui = cold_ui();
    let mut calls = 0_u32;
    cold_frame(&mut ui, |_| calls += 1);
    assert_eq!(calls, 2, "first frame: warmup pass + real pass");

    let snapshot = calls;
    cold_frame(&mut ui, |_| calls += 1);
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
    let mut ui = cold_ui();
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));

    let observed: std::cell::RefCell<Vec<Option<Vec2>>> = Default::default();
    cold_frame(&mut ui, |ui| {
        observed.borrow_mut().push(ui.input.pointer_pos);
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
    let mut ui = cold_ui();
    // Cursor lands inside the future button rect (button is anchored at
    // (0,0) with 60×30 size below). Delivered before any frame ran;
    // cascades is empty so on_input can't resolve a target.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 10.0)));
    assert_eq!(ui.input.hovered, None, "pre-frame: no cascade, no hit");

    let button_id = WidgetId::from_hash("btn");
    cold_frame(&mut ui, |ui| {
        Button::new()
            .id(button_id)
            .label("hi")
            .size((60.0, 30.0))
            .show(ui);
    });

    assert_eq!(
        ui.input.hovered,
        Some(button_id),
        "warmup builds cascade; refresh_pointer_targets routes held \
         pointer onto the button before the real record pass",
    );
}

/// First frame, no input — assert the contract pinned by the in-engine
/// `assert!(!first_frame || matches!(damage, Damage::Full))`.
#[test]
fn cold_start_first_frame_damage_is_full() {
    let mut ui = cold_ui();
    let display = Display::from_physical(COLD, 1.0);
    let report = ui.record_test_frame_without_baseline(display, Duration::ZERO, |ui| {
        Frame::new()
            .auto_id()
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                ..Default::default()
            })
            .show(ui);
    });
    assert!(
        matches!(
            report.plan,
            Some(RenderPlan {
                kind: RenderKind::Full,
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
    let mut ui = cold_ui();
    let mut calls = 0_u32;
    cold_frame(&mut ui, |ui| {
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

/// `Ui::for_test*` constructors mark the recorder as warm by
/// synthesizing a `prev_stamp`. Tests must observe single-record
/// semantics on their first `run_at` so they don't have to reason
/// about the double-call contract for every assertion.
#[test]
fn for_test_constructors_skip_warmup() {
    let mut ui = Ui::for_test();
    let mut calls = 0_u32;
    ui.run_at(COLD, |_| calls += 1);
    assert_eq!(
        calls, 1,
        "for_test() ctor pre-marks warm; first user frame is single-pass",
    );
}

#[test]
fn run_at_value_returns_the_final_relayout_pass() {
    let mut ui = Ui::for_test();
    let mut calls = 0_u32;

    let final_call = ui.run_at_value(COLD, |ui| {
        calls += 1;
        if calls == 1 {
            ui.request_relayout();
        }
        calls
    });

    assert_eq!(calls, 2, "relayout runs exactly two record passes");
    assert_eq!(final_call, 2, "capture returns the final pass's value");
}

/// O5 stage 0: an unchanged frame skips the cascade (its output is
/// provably identical); any cascade-input change — authoring or the
/// exact surface — re-runs it. Pinned via `dbg_cascade_ran`.
#[test]
fn cascade_skip_fires_on_unchanged_reruns_on_change() {
    use crate::layout::types::sizing::Sizing;

    fn build(ui: &mut Ui, w: f32) {
        Frame::new()
            .id(WidgetId::from_hash("f"))
            .size((Sizing::fixed(w), Sizing::fixed(50.0)))
            .show(ui);
    }

    let mut ui = Ui::for_test();
    ui.run_at(SURFACE, |ui| build(ui, 50.0));
    assert!(
        ui.frame_runtime.dbg_cascade_ran,
        "first frame runs the cascade"
    );

    ui.run_at(SURFACE, |ui| build(ui, 50.0));
    assert!(
        !ui.frame_runtime.dbg_cascade_ran,
        "unchanged frame skips the cascade"
    );

    ui.run_at(SURFACE, |ui| build(ui, 80.0));
    assert!(
        ui.frame_runtime.dbg_cascade_ran,
        "authoring change re-runs the cascade"
    );

    ui.run_at(SURFACE, |ui| build(ui, 80.0));
    assert!(
        !ui.frame_runtime.dbg_cascade_ran,
        "settles back to skipping"
    );

    ui.run_at(UVec2::new(SURFACE.x + 1, SURFACE.y), |ui| build(ui, 80.0));
    assert!(
        ui.frame_runtime.dbg_cascade_ran,
        "exact-surface change re-runs the cascade"
    );
}

/// O5 stage-0 completeness for the *authoring* cascade inputs. The
/// fingerprint trusts `subtree_hash` to capture everything the cascade
/// reads (transforms, clip / disabled / focusable, visibility, chrome,
/// shapes); if a future input stops being folded in, a frame toggling
/// it would wrongly skip the cascade and paint stale. One arm per
/// attribute class — each toggles a single attribute and asserts the
/// skip is busted. The scroll offset/zoom class lives in `scroll_states`
/// (not `subtree_hash`) and is folded into the fingerprint explicitly;
/// it's pinned separately by
/// `widgets::tests::scroll::cascade_skip_busts_on_scroll_offset_change`.
#[test]
fn cascade_fingerprint_covers_authoring_input_classes() {
    use crate::layout::types::clip_mode::ClipMode;
    use crate::scene::visibility::Visibility;

    fn probe(ui: &mut Ui, cfg: impl FnOnce(Frame) -> Frame) {
        cfg(Frame::new().id(WidgetId::from_hash("probe")).size(50.0)).show(ui);
    }

    // Settle `base` into the skip, then run `changed` and assert the
    // one-attribute delta re-runs the cascade.
    fn assert_reruns(label: &str, base: impl Fn(&mut Ui), changed: impl Fn(&mut Ui)) {
        let mut ui = Ui::for_test();
        ui.run_at(SURFACE, |ui| base(ui));
        assert!(
            ui.frame_runtime.dbg_cascade_ran,
            "{label}: first frame runs the cascade"
        );
        ui.run_at(SURFACE, |ui| base(ui));
        assert!(
            !ui.frame_runtime.dbg_cascade_ran,
            "{label}: unchanged frame skips the cascade"
        );
        ui.run_at(SURFACE, |ui| changed(ui));
        assert!(
            ui.frame_runtime.dbg_cascade_ran,
            "{label}: toggling it must re-run the cascade — the input is \
             missing from subtree_hash / the cascade fingerprint",
        );
    }

    fn bg(r: f32, g: f32, b: f32) -> Background {
        Background {
            fill: Color::rgb(r, g, b).into(),
            ..Default::default()
        }
    }

    assert_reruns(
        "disabled",
        |ui| probe(ui, |f| f.disabled(false)),
        |ui| probe(ui, |f| f.disabled(true)),
    );
    assert_reruns(
        "focusable",
        |ui| probe(ui, |f| f.focusable(false)),
        |ui| probe(ui, |f| f.focusable(true)),
    );
    assert_reruns(
        "visibility",
        |ui| probe(ui, |f| f.visibility(Visibility::Visible)),
        |ui| probe(ui, |f| f.visibility(Visibility::Hidden)),
    );
    assert_reruns(
        "clip",
        |ui| probe(ui, |f| f.clip(ClipMode::None)),
        |ui| probe(ui, |f| f.clip(ClipMode::Rect)),
    );
    assert_reruns(
        "chrome",
        |ui| probe(ui, |f| f.background(bg(0.2, 0.4, 0.8))),
        |ui| probe(ui, |f| f.background(bg(0.8, 0.2, 0.2))),
    );
}

/// `open_window` / `close_window` enqueue onto the retained scratch the
/// host drains *after* the frame — so the requests must survive the
/// `frame` call that filed them (and a subsequent quiet frame), since
/// the host hasn't had a chance to run yet. Without that, a window
/// opened during record would be silently dropped before the event loop
/// regained `&ActiveEventLoop` to act on it.
#[test]
fn window_requests_queue_and_survive_the_frame() {
    use crate::{WindowConfig, WindowToken};

    let mut ui = Ui::for_test();
    let open = WindowToken(7);
    let close = WindowToken(3);

    ui.run_at_without_baseline(SURFACE, |ui| {
        ui.open_window(open, WindowConfig::new("inspector"));
        ui.close_window(close);
    });

    // Filed during record, still pending after the frame returned —
    // nothing in the frame pipeline clears them.
    assert_eq!(ui.window_requests.commands.opens.len(), 1);
    assert_eq!(ui.window_requests.commands.opens[0].token, open);
    assert_eq!(
        ui.window_requests.commands.opens[0].config.title,
        "inspector"
    );
    assert_eq!(ui.window_requests.commands.closes, vec![close]);

    // A quiet frame (no new requests) must not drop the still-undrained
    // queue — the host might not have ticked between these two frames.
    ui.run_at_without_baseline(SURFACE, |_| {});
    assert_eq!(
        ui.window_requests.commands.opens.len(),
        1,
        "queue must outlive a quiet frame"
    );
    assert_eq!(ui.window_requests.commands.closes, vec![close]);

    // The host drains by `append`/`drain`-ing the vecs; emulate that and
    // confirm a third frame leaves them empty (no re-queue).
    ui.window_requests.commands.opens.clear();
    ui.window_requests.commands.closes.clear();
    ui.run_at_without_baseline(SURFACE, |_| {});
    assert!(ui.window_requests.commands.opens.is_empty());
    assert!(ui.window_requests.commands.closes.is_empty());

    // `window_open` polls the host-refreshed live set (here set directly,
    // as the host would before each frame) — not the pending queues.
    assert!(!ui.window_open(open), "empty live set ⇒ nothing open");
    ui.resources.windows.set_live(open, true);
    assert!(ui.window_open(open));
    assert!(!ui.window_open(close), "only `open` is live");

    ui.window_frame.position = Some(IVec2::new(-120, 48));
    ui.window_frame.maximized = true;
    let geometry = ui.window_geometry();
    assert_eq!(geometry.inner_size, SURFACE);
    assert_eq!(geometry.outer_position, Some(IVec2::new(-120, 48)));
    assert!(geometry.maximized);
}

/// The OS-close veto protocol between the host and app code:
/// [`Ui::close_requested`] reflects the host's per-frame `wants_close`
/// signal, and [`Ui::keep_open`] sets the veto the host reads back to
/// decide whether to actually close. The host's decision rule is
/// `wants_close && !close_vetoed` (the tail of `WinitHost::draw`); pin it
/// here so the two flags can't drift out from under that resolution.
#[test]
fn close_request_veto_protocol() {
    let mut ui = Ui::for_test();

    // No close pending: the flag is false and keep_open never fires.
    ui.run_at_without_baseline(SURFACE, |ui| {
        assert!(
            !ui.close_requested(),
            "no close pending ⇒ close_requested() false"
        );
    });
    assert!(!ui.window_requests.close_vetoed);

    // Host signals a close; an app that vetoes keeps the window open.
    ui.window_frame.close_requested = true;
    ui.window_requests.close_vetoed = false;
    ui.run_at_without_baseline(SURFACE, |ui| {
        assert!(
            ui.close_requested(),
            "host signalled close ⇒ close_requested() true"
        );
        ui.keep_open();
    });
    assert!(
        ui.window_requests.close_vetoed,
        "keep_open must set the veto the host reads"
    );
    let should_close = ui.window_frame.close_requested && !ui.window_requests.close_vetoed;
    assert!(
        !should_close,
        "a vetoed request must NOT resolve to a close"
    );

    // Same signal, app ignores it: resolves to a real close. (The host
    // resets the veto before every draw.)
    ui.window_requests.close_vetoed = false;
    ui.run_at_without_baseline(SURFACE, |ui| {
        assert!(ui.close_requested());
    });
    assert!(!ui.window_requests.close_vetoed, "untouched ⇒ no veto");
    let should_close = ui.window_frame.close_requested && !ui.window_requests.close_vetoed;
    assert!(should_close, "an un-vetoed request must resolve to a close");
}

/// O5 stage-0 completeness for the *identity* cascade inputs: the
/// layer a root subtree lives on and the root's own `WidgetId`.
/// Neither reaches any subtree hash (`compute_rollups` folds only
/// child ids into parents, and roots have no parent), so the
/// fingerprint folds them explicitly. A wrongly matching fingerprint
/// here reuses per-layer cascade columns sized for the previous
/// layer assignment (index OOB in the damage pass) or a `by_id` map
/// still keyed by the dead old root id (inert widget).
#[test]
fn cascade_fingerprint_covers_layer_and_root_identity() {
    fn float(ui: &mut Ui, layer: Layer, key: &str) {
        Frame::new()
            .id(WidgetId::from_hash("anchor"))
            .size(50.0)
            .show(ui);
        ui.layer(layer, Vec2::new(10.0, 10.0), None, |ui| {
            Frame::new()
                .id(WidgetId::from_hash(key))
                .size(20.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8).into(),
                    ..Default::default()
                })
                .show(ui);
        });
    }
    let assert_reruns = |label: &str, base: &dyn Fn(&mut Ui), changed: &dyn Fn(&mut Ui)| {
        let mut ui = Ui::for_test();
        ui.run_at(SURFACE, |ui| base(ui));
        ui.run_at(SURFACE, |ui| base(ui));
        assert!(
            !ui.frame_runtime.dbg_cascade_ran,
            "{label}: unchanged frame skips the cascade"
        );
        ui.run_at(SURFACE, |ui| changed(ui));
        assert!(
            ui.frame_runtime.dbg_cascade_ran,
            "{label}: identity change must re-run the cascade",
        );
    };
    assert_reruns(
        "layer migration",
        &|ui| float(ui, Layer::Popup, "float"),
        &|ui| float(ui, Layer::Tooltip, "float"),
    );
    assert_reruns(
        "root re-key",
        &|ui| float(ui, Layer::Popup, "float"),
        &|ui| float(ui, Layer::Popup, "float2"),
    );
}

/// The interaction half of `response_for` routes against the one-frame
/// -stale cascade, so on the frame a subtree becomes disabled a widget
/// could otherwise observe `hovered`/`clicked` alongside
/// `disabled == true` — a combination the steady-state hit index never
/// produces (disabled entries carry `Sense::NONE`), and one that lets
/// a click land on just-disabled UI.
#[test]
fn freshly_disabled_subtree_masks_stale_interactions() {
    let target = WidgetId::from_hash("target");
    let mut ui = Ui::for_test();
    let run = |ui: &mut Ui, disabled: bool| {
        let mut resp = None;
        ui.run_at(SURFACE, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("wrap"))
                .disabled(disabled)
                .show(ui, |ui| {
                    resp = Some(ui.response_for(target));
                    Button::new().label("hi").id(target).show(ui);
                });
        });
        resp.unwrap()
    };
    run(&mut ui, false);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0)));
    let enabled = run(&mut ui, false);
    assert!(enabled.hovered, "sanity: pointer hovers the button");
    assert!(!enabled.disabled);
    // Disable frame: stale cascade still routes the hover; the read
    // must mask it.
    let disabled = run(&mut ui, true);
    assert!(disabled.disabled, "ancestor-disabled ORs in lag-free");
    assert!(
        !disabled.hovered,
        "interactions must mask on the disable frame"
    );

    use crate::primitives::color::ColorF16;
    use crate::scene::shapes::paint::ShapeBrush;

    let self_id = WidgetId::from_hash("self-disabled");
    let disabled_fill = Color::rgb(0.8, 0.1, 0.2);
    let mut style = ui.theme.button.clone();
    style.looks.disabled.background = Some(Background::fill(disabled_fill));
    let response = ui.run_at_value(SURFACE, |ui| {
        Button::new()
            .id(self_id)
            .label("disabled")
            .style(&style)
            .disabled(true)
            .show(ui)
            .snapshot()
    });
    assert!(
        !response.disabled,
        "eager response must retain the unmerged cascade snapshot",
    );
    let endpoint = ui.cascades.by_id[&self_id];
    let chrome = ui.forest.trees[endpoint.layer]
        .chrome(endpoint.node)
        .expect("disabled button chrome");
    let ShapeBrush::Solid(actual_fill) = chrome.fill else {
        panic!("disabled button must retain its solid test fill");
    };
    assert_eq!(
        actual_fill,
        ColorF16::from(disabled_fill),
        "fresh self-disable must pick disabled visuals before cascade catches up",
    );
}

/// The fps EMA reads the TRUE frame delta — the MAX_DT clamp is for
/// the animation integrator only. Hand-computed: sample 1 at 1 s →
/// inst 1.0 seeds the EMA; sample 2 after a 2 s stall → inst 0.5,
/// EMA = 1.0·0.9 + 0.5·0.1 = 0.95. The clamp would have recorded both
/// stalls as 10 fps samples (EMA 10.0), reporting a HIGHER rate the
/// longer the stall.
#[test]
fn fps_ema_reads_unclamped_frame_delta() {
    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);
    let mut noop = |_: &mut Ui| {};
    ui.record_test_frame(display, Duration::ZERO, &mut noop);
    ui.record_test_frame(display, Duration::from_secs(1), &mut noop);
    assert!(
        (ui.frame_runtime.fps_ema - 1.0).abs() < 1e-6,
        "got {}",
        ui.frame_runtime.fps_ema
    );
    ui.record_test_frame_without_baseline(display, Duration::from_secs(3), &mut noop);
    assert!(
        (ui.frame_runtime.fps_ema - 0.95).abs() < 1e-6,
        "got {}",
        ui.frame_runtime.fps_ema
    );
}

/// Record passes replay (cold-start warmup, double-layout pass B), so
/// one logical `open_window` call reaches the queue two or three times
/// per frame — dedup by token, last config wins.
#[test]
fn open_window_dedups_by_token_within_a_frame() {
    use crate::window::{WindowConfig, WindowToken};
    let mut ui = Ui::for_test();
    let cfg = WindowConfig::new;
    ui.open_window(WindowToken(7), cfg("first"));
    ui.open_window(WindowToken(7), cfg("second"));
    ui.open_window(WindowToken(8), cfg("other"));
    assert_eq!(ui.window_requests.commands.opens.len(), 2);
    assert_eq!(ui.window_requests.commands.opens[0].token, WindowToken(7));
    assert_eq!(ui.window_requests.commands.opens[0].config.title, "second");
    assert_eq!(ui.window_requests.commands.opens[1].token, WindowToken(8));
}
