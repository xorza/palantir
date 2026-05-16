use crate::TextStyle;
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::{Layer, NodeId};
use crate::layout::types::display::Display;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect};
use crate::ui::FrameStamp;
use crate::ui::damage::Damage;
use crate::ui::frame_report::RenderPlan;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(200, 200);

fn measure_calls(ui: &Ui) -> u64 {
    ui.text.measure_calls()
}

fn blue_frame(ui: &mut Ui, salt: &'static str) -> NodeId {
    Frame::new()
        .id_salt(salt)
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.2, 0.4, 0.8).into(),
            ..Default::default()
        })
        .show(ui)
        .node(ui)
}

/// Two `.id_salt("dup")` calls in one frame would silently corrupt
/// every per-id store. Instead of panicking, `SeenIds::record`
/// disambiguates the second one (same path as auto-id collisions),
/// `Forest` pairs both colliding nodes via `Forest.collisions`, and
/// the encoder emits a magenta `DrawRect` at each colliding node's
/// arranged rect after the regular paint walk.
#[test]
fn duplicate_explicit_widget_id_disambiguates_and_flags() {
    let mut ui = Ui::for_test();
    let button_node = std::cell::Cell::new(NodeId(0));
    ui.run_at(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let a = Button::new().id_salt("dup").show(ui);
            Button::new().id_salt("dup").show(ui);
            button_node.set(a.node(ui));
        });
    });
    // One collision pair should be recorded, survives until the next
    // `pre_record` so the encoder can read it.
    assert_eq!(
        ui.forest.collisions.len(),
        1,
        "expected exactly one explicit collision recorded",
    );
    let button_rect = ui.layout[Layer::Main].rect[button_node.get().index()];
    // Drive the encoder and check the emitted quads. The two overlay
    // quads should be stroked, magenta-ish, and rect-equal to the two
    // colliding buttons' arranged rects.
    // Share Ui's frame arena so any mesh/polyline bytes pushed at
    // record time are visible at compose / upload — the Host wiring
    // for real apps.
    let mut frontend = crate::renderer::frontend::Frontend::for_test();
    let buffer = frontend.build(
        &ui,
        RenderPlan::Full {
            clear: ui.theme.window_clear,
        },
    );
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

/// Cross-layer collision: `.id_salt("dup")` in Main and another with
/// the same key inside a `Ui::layer(Popup, ...)` body. `SeenIds.curr`
/// is shared across layers, so the second occurrence is detected as a
/// collision. Each `CollisionRecord` endpoint carries its own `Layer`,
/// so the encoder paints each overlay at the correct per-layer rect.
#[test]
fn cross_layer_explicit_widget_id_collision_resolves_per_layer() {
    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(200, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Button::new().id_salt("dup").show(ui);
        });
        ui.layer(Layer::Popup, glam::Vec2::ZERO, None, |ui| {
            Button::new().id_salt("dup").show(ui);
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
    let main_rect = ui.layout[Layer::Main].rect[pair.first.node.index()];
    let popup_rect = ui.layout[Layer::Popup].rect[pair.second.node.index()];
    // Share Ui's frame arena so any mesh/polyline bytes pushed at
    // record time are visible at compose / upload — the Host wiring
    // for real apps.
    let mut frontend = crate::renderer::frontend::Frontend::for_test();
    let buffer = frontend.build(
        &ui,
        RenderPlan::Full {
            clear: ui.theme.window_clear,
        },
    );
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

/// Pin: the encoder-direct overlay path leaves `Layer::Debug` empty
/// (no sink node recorded) — guards against silent regression back to
/// the prior "sink in Debug" approach.
#[test]
fn collisions_do_not_record_into_debug_layer() {
    let mut ui = Ui::for_test();
    assert!(
        !ui.debug_overlay.frame_stats,
        "test relies on frame_stats off — Debug should otherwise stay empty",
    );
    ui.run_at(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new().id_salt("dup").show(ui);
            Button::new().id_salt("dup").show(ui);
        });
    });
    assert!(
        !ui.forest.collisions.is_empty(),
        "collision should have been recorded",
    );
    assert_eq!(
        ui.forest.tree(Layer::Debug).records.len(),
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
    ui.run_at(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            chip(ui);
            chip(ui);
            chip(ui);
        });
    });
    // Synthetic viewport root + 1 panel + 3 chips = 5 distinct ids, no panic.
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 5);
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
    ui.run_at(SURFACE, |ui| {
        let probe_resp = std::cell::RefCell::new(None);
        Panel::vstack().auto_id().show(ui, |ui| {
            *probe_resp.borrow_mut() = Some(Frame::new().id_salt(id_salt).size(40.0).show(ui));
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
    ui.run_at(SURFACE, |_| {});

    // Empty UI on the first frame: damage is `None` (skip). Force `Full`
    // to exercise encode/compose and assert the buffers come out empty.
    // No mesh/polyline bytes recorded → a private frontend arena works.
    let mut frontend = crate::renderer::frontend::Frontend::for_test();
    let buffer = frontend.build(
        &ui,
        RenderPlan::Full {
            clear: ui.theme.window_clear,
        },
    );
    assert!(buffer.quads.is_empty());
    assert!(buffer.texts.is_empty());
    assert!(buffer.groups.is_empty());

    // Synthetic viewport root: even an empty user record produces one node.
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 1);
    assert!(ui.damage_engine.prev.is_empty());
    assert!(ui.damage_engine.dirty.is_empty());
    assert!(ui.damage_region().is_empty());
    assert_eq!(
        Damage::new(ui.display.logical_rect(), ui.damage_region()),
        Damage::None,
    );
}

/// Pin: an empty frame followed by a populated frame works (the
/// recorder retains no per-frame state across frames).
#[test]
fn empty_then_populated_frame() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(100, 100), |_| {});
    ui.run_at_acked(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |_| {});
    });
    // Synthetic viewport root + user Panel = 2 records.
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 2);
    // Root Panel is non-painting (no chrome, no shapes) so prev stays
    // empty — only painting widgets are tracked.
    assert!(ui.damage_engine.prev.is_empty());
}

/// Pin: `Ui::frame` panics if `display.scale_factor` is below `EPS`.
#[test]
#[should_panic(expected = "Display::scale_factor must be ≥ EPSILON")]
fn frame_rejects_zero_scale_factor() {
    let mut ui = Ui::for_test();
    let _ = ui.frame(
        FrameStamp::new(
            Display::from_physical(UVec2::new(800, 600), 0.0),
            Duration::ZERO,
        ),
        &mut (),
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

/// Pin: after the first frame, painting widgets land in `prev` with
/// their arranged rect and authoring hash; non-painting widgets (Panel
/// without chrome) don't.
#[test]
fn prev_frame_captures_painting_nodes() {
    let mut ui = Ui::for_test();
    let mut frame_node = None;
    ui.run_at(SURFACE, |ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            frame_node = Some(blue_frame(ui, "a"));
        });
    });
    let frame_node = frame_node.unwrap();
    let prev = &ui.damage_engine.prev;
    let snap = prev[&WidgetId::from_hash("a")];

    assert!(!prev.contains_key(&WidgetId::from_hash("root")));
    assert_eq!(snap.rect, ui.layout[Layer::Main].rect[frame_node.index()]);
    assert_eq!(
        snap.hash,
        ui.forest.tree(Layer::Main).rollups.node[frame_node.index()],
    );
}

#[test]
fn prev_frame_drops_disappeared_widgets() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(SURFACE, |ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Button::new().id_salt("gone").label("X").show(ui);
        });
    });
    assert!(
        ui.damage_engine
            .prev
            .contains_key(&WidgetId::from_hash("gone"))
    );

    ui.run_at_acked(SURFACE, |ui| {
        Panel::hstack().id_salt("root").show(ui, |_| {});
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
                .id_salt("a")
                .size(50.0)
                .background(Background {
                    fill: fill.into(),
                    ..Default::default()
                })
                .show(ui);
        }
    };
    ui.run_at_acked(SURFACE, paint(Color::rgb(0.2, 0.4, 0.8)));
    let h1 = ui.damage_engine.prev[&WidgetId::from_hash("a")].hash;

    ui.run_at_acked(SURFACE, paint(Color::rgb(0.9, 0.4, 0.8)));
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
            Text::new("the quick brown fox").id_salt("hello").show(ui);
        });
    };
    let wrapped: Build = |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::Fixed(60.0), Sizing::Hug))
            .show(ui, |ui| {
                Text::new("the quick brown fox jumps over the lazy dog")
                    .id_salt("wrapped")
                    .style(TextStyle::default().with_font_size(16.0))
                    .wrapping()
                    .show(ui);
            });
    };
    let grid_intrinsic: Build = |ui| {
        Grid::new()
            .id_salt("g")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .cols(std::rc::Rc::from([Track::hug(), Track::fill()]))
            .show(ui, |ui| {
                Text::new("label")
                    .id_salt("hug-col-text")
                    .grid_cell((0, 0))
                    .show(ui);
                Text::new("the quick brown fox jumps over the lazy dog")
                    .id_salt("fill-col-text")
                    .wrapping()
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
        ui.run_at_acked(UVec2::new(400, 200), build);
        let after_first = measure_calls(&ui);
        assert!(
            after_first > 0,
            "{label}: first frame should drive at least one measure call",
        );
        ui.run_at_acked(UVec2::new(400, 200), build);
        let after_second = measure_calls(&ui);
        assert_eq!(
            after_second,
            after_first,
            "{label}: second identical frame must reuse cached MeasureResult \
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
                Text::new(content).id_salt("changing").show(ui);
            });
        }
    };
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(400, 200), render("first"));
    let before = measure_calls(&ui);
    ui.run_at_acked(UVec2::new(400, 200), render("second"));
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
    ui.run_at_acked(UVec2::new(400, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("hello").id_salt("transient").show(ui);
        });
    });
    let wid = WidgetId::from_hash("transient");
    assert!(
        ui.text.has_reuse_entry(wid, 0),
        "text widget should populate text_reuse on first render",
    );

    ui.run_at_acked(UVec2::new(400, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |_| {});
    });
    assert!(
        !ui.text.has_reuse_entry(wid, 0),
        "removed widget's reuse entry must be swept",
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
                .size((Sizing::Fixed(slot_w), Sizing::Hug))
                .show(ui, |ui| {
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .id_salt("p")
                        .style(TextStyle::default().with_font_size(16.0))
                        .wrapping()
                        .show(ui);
                });
        }
    };

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(400, 200), render(60.0));
    let after_first = measure_calls(&ui);
    assert!(
        after_first >= 2,
        "first frame should measure both unbounded and wrap (got {after_first})",
    );
    ui.run_at_acked(UVec2::new(400, 200), render(80.0));
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

    ui.run_at_acked(UVec2::new(100, 100), |ui| {
        Frame::new().id_salt("a").show(ui);
        Frame::new().id_salt("b").show(ui);
        *ui.state_mut::<u32>(id_a) = 11;
        *ui.state_mut::<u32>(id_b) = 22;
    });
    ui.run_at_acked(UVec2::new(100, 100), |ui| {
        Frame::new().id_salt("a").show(ui);
        // Reading state during recording so the row is touched while
        // its widget is still seen.
        assert_eq!(*ui.state_mut::<u32>(id_a), 11);
    });
    ui.run_at_acked(UVec2::new(100, 100), |ui| {
        Frame::new().id_salt("b").show(ui);
        assert_eq!(
            *ui.state_mut::<u32>(id_b),
            0,
            "B was unrecorded last frame; its row should have been swept",
        );
    });
}

/// `Ui::frame` re-records when the frame contained input that could
/// plausibly drive a state mutation (action input), and runs the build
/// closure exactly once otherwise. Action coverage has to be exact:
/// false positives waste CPU silently, false negatives leave the
/// popup-dismissal class of bugs unfixed.
#[test]
fn frame_pass_count_matches_action_trigger() {
    use crate::input::InputEvent;
    use crate::input::keyboard::{Key, Modifiers};
    use crate::input::pointer::PointerButton;
    use glam::Vec2;
    use std::cell::Cell;

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
            "click",
            |ui| {
                ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0)));
                ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
                ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
            },
            2,
        ),
        (
            "keydown",
            |ui| {
                ui.on_input(InputEvent::KeyDown {
                    key: Key::Enter,
                    repeat: false,
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
        ui.run_at_acked(UVec2::new(100, 100), |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
        });
        prime(&mut ui);

        let count = Cell::new(0u32);
        let frame_id_before = ui.frame_id;
        let _ = ui.frame(FrameStamp::new(display, Duration::ZERO), &mut (), |ui| {
            count.set(count.get() + 1);
            Panel::vstack().id_salt("root").show(ui, |_| {});
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
            ui.frame_id,
            frame_id_before + 1,
            "{label}: frame_id must bump exactly once per frame (passes: {expected})",
        );
    }
}

/// `Ui::frame` plumbs `now`, `dt`, and the repaint-requested flag
/// end-to-end: per-call `now` lands in `Ui::time`, the derived `dt`
/// clamps to `MAX_DT`, `repaint_requested` resets at the top of every
/// call, and a flag set during recording surfaces on `FrameOutput`.
#[test]
fn frame_plumbs_now_dt_and_repaint_request() {
    const MAX_DT: f32 = Ui::MAX_DT;
    let display = Display::from_physical(UVec2::new(100, 100), 1.0);

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(100, 100), |ui| {
        Panel::vstack().id_salt("root").show(ui, |_| {});
    });

    // Frame A: idle, no repaint request, now = 16ms.
    let repaint = ui
        .frame(
            FrameStamp::new(display, Duration::from_millis(16)),
            &mut (),
            |ui| {
                Panel::vstack().id_salt("root").show(ui, |_| {});
            },
        )
        .repaint_requested();
    assert!(
        !repaint,
        "no animate-not-settled flag set — must stay false"
    );
    assert_eq!(ui.time, Duration::from_millis(16));
    assert!(
        (ui.dt - 0.016).abs() < 1e-6,
        "Ui::dt should be (now - prev) in seconds; got {}",
        ui.dt,
    );

    // Frame B: simulate an unsettled animation tick by setting the
    // internal flag during recording. The flag must reach `FrameOutput`.
    let repaint = ui
        .frame(
            FrameStamp::new(display, Duration::from_millis(32)),
            &mut (),
            |ui| {
                Panel::vstack().id_salt("root").show(ui, |_| {});
                ui.repaint_requested = true;
            },
        )
        .repaint_requested();
    assert!(
        repaint,
        "repaint_requested set during recording must surface on FrameOutput",
    );
    assert_eq!(ui.time, Duration::from_millis(32));
    assert!(
        (ui.dt - 0.016).abs() < 1e-6,
        "Ui::dt should be next-frame delta; got {}",
        ui.dt,
    );

    // Frame C: oversized gap (5s) clamps dt to MAX_DT; `time` still
    // tracks true clock so animation math doesn't teleport.
    let _ = ui.frame(
        FrameStamp::new(display, Duration::from_millis(5_032)),
        &mut (),
        |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
        },
    );
    assert_eq!(ui.time, Duration::from_millis(5_032));
    assert!(
        (ui.dt - MAX_DT).abs() < 1e-6,
        "Ui::dt should clamp at MAX_DT; got {}",
        ui.dt,
    );

    // Frame D: prior frame's repaint_requested must NOT leak — resets
    // at the top of every `frame` regardless of pass count.
    let repaint = ui
        .frame(
            FrameStamp::new(display, Duration::from_millis(5_048)),
            &mut (),
            |ui| {
                Panel::vstack().id_salt("root").show(ui, |_| {});
            },
        )
        .repaint_requested();
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
    ui.debug_overlay.frame_stats = true;
    let display = Display::from_physical(SURFACE, 1.0);

    // Warm-up frame at t = 0. `fps_ema` stays zero (no prior `time` to
    // diff against), but the Debug layer should already carry the
    // readout.
    ui.frame(FrameStamp::new(display, Duration::ZERO), &mut (), |ui| {
        Frame::new().id_salt("body").size(50.0).show(ui);
    });
    ui.frame_state.mark_submitted();
    assert_eq!(ui.fps_ema, 0.0);
    assert!(
        !ui.forest.tree(Layer::Debug).records.is_empty(),
        "Debug layer must carry the frame_stats readout",
    );

    // Second frame at t = 16ms. Main scene is unchanged; only the
    // Debug-layer readout dirties → expect `Partial`, not `Full`,
    // and not `None` either. `fps_ema` picks up its first instantaneous
    // reading (~62.5).
    let report = ui.frame(
        FrameStamp::new(display, Duration::from_millis(16)),
        &mut (),
        |ui| {
            Frame::new().id_salt("body").size(50.0).show(ui);
        },
    );
    ui.frame_state.mark_submitted();
    assert!(
        matches!(report.plan, Some(RenderPlan::Partial { .. })),
        "frame_stats should produce Partial damage on a static scene; got {:?}",
        report.plan,
    );
    assert!(
        ui.fps_ema > 0.0,
        "fps_ema must update after the second frame; got {}",
        ui.fps_ema,
    );

    // Disabling the flag mid-stream evicts the Debug-layer node next
    // frame.
    ui.debug_overlay.frame_stats = false;
    ui.frame(
        FrameStamp::new(display, Duration::from_millis(32)),
        &mut (),
        |ui| {
            Frame::new().id_salt("body").size(50.0).show(ui);
        },
    );
    assert!(
        ui.forest.tree(Layer::Debug).records.is_empty(),
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
    let report = ui.frame(
        FrameStamp::new(display, Duration::from_secs_f32(0.0)),
        &mut (),
        |ui| {
            ui.request_repaint_after(Duration::from_secs_f32(0.5));
            ui.request_repaint_after(Duration::from_secs_f32(1.5));
        },
    );
    // Earliest deadline wins the report slot.
    assert_eq!(
        report.repaint_after(),
        Some(Duration::from_secs_f32(0.5)),
        "FrameReport must surface the earliest pending wake",
    );
    // Both entries are still queued (neither has fired).
    assert_eq!(
        ui.repaint_wakes.len(),
        2,
        "both distinct deadlines stay queued"
    );

    // Run a frame at the first deadline. The earliest entry drains;
    // the second survives.
    let report = ui.frame(
        FrameStamp::new(display, Duration::from_secs_f32(0.5)),
        &mut (),
        |_| {},
    );
    assert_eq!(
        report.repaint_after(),
        Some(Duration::from_secs_f32(1.5)),
        "second deadline survives the first frame's drain",
    );
    assert_eq!(ui.repaint_wakes.len(), 1);

    // Run a frame at the second deadline. Queue empties.
    let report = ui.frame(
        FrameStamp::new(display, Duration::from_secs_f32(1.5)),
        &mut (),
        |_| {},
    );
    assert_eq!(report.repaint_after(), None);
    assert!(ui.repaint_wakes.is_empty());
}

/// Re-requesting an already-queued deadline within the same frame
/// is a no-op — the queue is sorted + dedup'd. Near-duplicates within
/// `REPAINT_COALESCE_DT` (1/120 s) collapse onto the later wake to
/// minimize host wake-ups; entries spaced beyond the window stay
/// distinct.
#[test]
fn request_repaint_after_dedups_within_frame() {
    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);
    ui.frame(
        FrameStamp::new(display, Duration::from_secs_f32(0.0)),
        &mut (),
        |ui| {
            for _ in 0..10 {
                ui.request_repaint_after(Duration::from_secs_f32(0.5));
            }
            ui.request_repaint_after(Duration::from_secs_f32(0.5));
        },
    );
    assert_eq!(
        ui.repaint_wakes.len(),
        1,
        "exact duplicate deadlines collapse to one entry",
    );

    // Near-duplicates within the 1/120 s window collapse onto the
    // later deadline (prefer the longer wait); deadlines spaced
    // beyond the window stay distinct.
    let mut ui = Ui::for_test();
    ui.frame(
        FrameStamp::new(display, Duration::from_secs_f32(0.0)),
        &mut (),
        |ui| {
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
        },
    );
    let deadlines: Vec<Duration> = ui.repaint_wakes.iter().map(|w| w.deadline).collect();
    assert_eq!(
        deadlines,
        vec![
            Duration::from_secs_f32(0.512),
            Duration::from_secs_f32(0.600),
        ],
        "near-duplicate wakes collapse onto the later deadline",
    );
}

/// Entries with `deadline <= now` drain at the top of the next
/// frame; entries strictly past `now` survive.
#[test]
fn request_repaint_after_drains_fired_entries() {
    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);
    ui.frame(
        FrameStamp::new(display, Duration::from_secs_f32(0.0)),
        &mut (),
        |ui| {
            ui.request_repaint_after(Duration::from_secs_f32(0.5));
            ui.request_repaint_after(Duration::from_secs_f32(1.0));
            ui.request_repaint_after(Duration::from_secs_f32(2.0));
        },
    );
    assert_eq!(ui.repaint_wakes.len(), 3);

    // Frame at t=1.0 drains entries at 0.5 and 1.0; 2.0 survives.
    let report = ui.frame(
        FrameStamp::new(display, Duration::from_secs_f32(1.0)),
        &mut (),
        |_| {},
    );
    assert_eq!(ui.repaint_wakes.len(), 1);
    assert_eq!(report.repaint_after(), Some(Duration::from_secs_f32(2.0)));
}

#[test]
fn app_state_round_trip_across_frame() {
    struct App {
        count: u32,
    }
    let mut ui = Ui::for_test();
    let mut app = App { count: 0 };
    ui.frame(
        FrameStamp::new(Display::default(), Duration::ZERO),
        &mut app,
        |ui| {
            ui.app::<App>().count += 1;
            ui.app::<App>().count += 1;
        },
    );
    assert_eq!(app.count, 2);
}

#[test]
#[should_panic(expected = "no app state installed")]
fn app_without_install_panics() {
    let _ = Ui::for_test().app::<u32>();
}

#[test]
#[should_panic(expected = "type mismatch")]
fn app_type_mismatch_panics() {
    let mut ui = Ui::for_test();
    let mut a: u32 = 7;
    ui.frame(
        FrameStamp::new(Display::default(), Duration::ZERO),
        &mut a,
        |ui| {
            let _ = ui.app::<i64>();
        },
    );
}

/// Anim-only fast path: when the only wake fired is a paint-anim
/// quantum boundary (no input, no `request_repaint`, no real wake),
/// `Ui::frame` skips record + post-record and emits
/// `FrameProcessing::PaintOnly`.
#[test]
fn paint_only_fast_path_fires_on_anim_quantum_boundary() {
    use crate::animation::paint::PaintAnim;
    use crate::primitives::brush::Brush;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    use crate::shape::Shape;
    use crate::ui::frame_report::FrameProcessing;

    let half = Duration::from_millis(500);

    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Frame::new().id_salt("blinker").size(20.0).show(ui);
            ui.add_shape_animated(
                Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 4.0, 12.0)),
                    radius: Corners::ZERO,
                    fill: Brush::Solid(Color::rgb(1.0, 0.0, 0.0)),
                    stroke: Stroke::default(),
                },
                PaintAnim::BlinkOpacity {
                    half_period: half,
                    started_at: Duration::ZERO,
                },
            );
        });
    }

    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);

    // Frame 0: record. Full path; schedules anim wake at `half`.
    let r0 = ui.frame(FrameStamp::new(display, Duration::ZERO), &mut (), |ui| {
        body(ui, half)
    });
    ui.frame_state.mark_submitted();
    assert_eq!(r0.processing(), FrameProcessing::SingleLayout);
    assert_eq!(r0.repaint_after(), Some(half));

    // Frame 1 at the blink boundary: only anim wake fires → fast path.
    let r1 = ui.frame(FrameStamp::new(display, half), &mut (), |ui| body(ui, half));
    assert_eq!(r1.processing(), FrameProcessing::PaintOnly);

    // PaintOnly must emit a Partial damage plan covering the anim's
    // tight rect — not Full (defeats the point) and not None (the
    // blink phase actually flipped). Pin both invariants.
    match r1.plan {
        Some(crate::ui::frame_report::RenderPlan::Partial { region, .. }) => {
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
    ui.frame_state.mark_submitted();

    // Bug regression: PaintOnly skips post_record, but must still
    // re-fold the retained paint_anims so the *next* blink boundary
    // is queued. Without this fold the caret stops blinking until
    // input forces a FullRecord (mouse-move regression).
    assert_eq!(r1.repaint_after(), Some(half + half));
    let r2 = ui.frame(FrameStamp::new(display, half + half), &mut (), |ui| {
        body(ui, half)
    });
    assert_eq!(r2.processing(), FrameProcessing::PaintOnly);
}

/// `request_repaint` co-firing with an anim wake produces the
/// `REAL | ANIM` mix, so the classifier picks Full.
#[test]
fn paint_only_skipped_when_widget_requested_repaint() {
    use crate::animation::paint::PaintAnim;
    use crate::primitives::brush::Brush;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    use crate::shape::Shape;
    use crate::ui::frame_report::FrameProcessing;

    let half = Duration::from_millis(500);

    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Frame::new().id_salt("blinker").size(20.0).show(ui);
            ui.add_shape_animated(
                Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 4.0, 12.0)),
                    radius: Corners::ZERO,
                    fill: Brush::Solid(Color::rgb(1.0, 0.0, 0.0)),
                    stroke: Stroke::default(),
                },
                PaintAnim::BlinkOpacity {
                    half_period: half,
                    started_at: Duration::ZERO,
                },
            );
        });
    }

    let mut ui = Ui::for_test();
    let display = Display::from_physical(SURFACE, 1.0);

    // Frame 0: record + `request_repaint`. Next frame must be Full.
    let r0 = ui.frame(FrameStamp::new(display, Duration::ZERO), &mut (), |ui| {
        body(ui, half);
        ui.request_repaint();
    });
    ui.frame_state.mark_submitted();
    assert!(r0.repaint_requested());

    let r1 = ui.frame(FrameStamp::new(display, half), &mut (), |ui| body(ui, half));
    assert_eq!(r1.processing(), FrameProcessing::SingleLayout);
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
    use crate::animation::paint::PaintAnim;
    use crate::input::InputEvent;
    use crate::input::keyboard::Key;
    use crate::input::policy::InputPolicy;
    use crate::primitives::brush::Brush;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    use crate::shape::Shape;
    use crate::ui::frame_report::FrameProcessing;
    use glam::Vec2;

    let display = Display::from_physical(UVec2::new(100, 100), 1.0);
    let half = Duration::from_millis(500);

    // Body declares an inert Frame *and* an anim shape so the next
    // frame's wake fires `ANIM`. Pointer-over-inert hits no Sense
    // entry, so OnDelta sees `requests_repaint = false`.
    fn body(ui: &mut Ui, half: Duration) {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Frame::new().id_salt("inert").size(80.0).show(ui);
            ui.add_shape_animated(
                Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 4.0, 12.0)),
                    radius: Corners::ZERO,
                    fill: Brush::Solid(Color::rgb(1.0, 0.0, 0.0)),
                    stroke: Stroke::default(),
                },
                PaintAnim::BlinkOpacity {
                    half_period: half,
                    started_at: Duration::ZERO,
                },
            );
        });
    }

    // --- OnDelta: inert pointer move keeps the PaintOnly fast path.
    {
        let mut ui = Ui::for_test();
        ui.input_policy = InputPolicy::OnDelta;
        let r0 = ui.frame(FrameStamp::new(display, Duration::ZERO), &mut (), |ui| {
            body(ui, half)
        });
        ui.frame_state.mark_submitted();
        assert_eq!(r0.processing(), FrameProcessing::SingleLayout);

        ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
        assert!(
            ui.input.had_input_since_last_frame,
            "had_input set after any event (precondition)",
        );
        assert!(
            !ui.input.repaint_requested_since_last_frame,
            "inert pointer move must not flip repaint_requested",
        );

        let r1 = ui.frame(FrameStamp::new(display, half), &mut (), |ui| body(ui, half));
        assert_eq!(
            r1.processing(),
            FrameProcessing::PaintOnly,
            "OnDelta + inert pointer move + anim wake → PaintOnly",
        );

        // PaintOnly path must have drained input sticky bits and queues.
        assert!(!ui.input.had_input_since_last_frame);
        assert!(!ui.input.repaint_requested_since_last_frame);
    }

    // --- Always: same inert move upgrades the frame to SingleLayout.
    {
        let mut ui = Ui::for_test();
        ui.input_policy = InputPolicy::Always;
        let _ = ui.frame(FrameStamp::new(display, Duration::ZERO), &mut (), |ui| {
            body(ui, half)
        });
        ui.frame_state.mark_submitted();

        ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
        let r1 = ui.frame(FrameStamp::new(display, half), &mut (), |ui| body(ui, half));
        assert_eq!(
            r1.processing(),
            FrameProcessing::SingleLayout,
            "Always + any input forces SingleLayout",
        );
    }

    // --- OnDelta: action input still records. KeyDown now wakes
    // only with focus or a chord subscriber, so prime focus first.
    {
        use crate::primitives::widget_id::WidgetId;
        let mut ui = Ui::for_test();
        ui.input_policy = InputPolicy::OnDelta;
        let _ = ui.frame(FrameStamp::new(display, Duration::ZERO), &mut (), |ui| {
            body(ui, half)
        });
        ui.frame_state.mark_submitted();
        ui.input.focused = Some(WidgetId::from_hash("editor"));

        ui.on_input(InputEvent::KeyDown {
            key: Key::Enter,
            repeat: false,
        });
        assert!(
            ui.input.repaint_requested_since_last_frame,
            "KeyDown with focus held must flip repaint_requested",
        );
        let r1 = ui.frame(FrameStamp::new(display, half), &mut (), |ui| body(ui, half));
        assert_ne!(
            r1.processing(),
            FrameProcessing::PaintOnly,
            "OnDelta must not pick PaintOnly on action input",
        );
    }
}
