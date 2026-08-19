//! What one recorded frame leaves behind, and what the next one reads.

use crate::Ui;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect};
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::frame::FrameRuntime;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::{COLD, SURFACE, blue_frame};
use crate::widgets::response::ResponseSnapshot;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::{IVec2, UVec2, Vec2};
use std::cell::{Cell, RefCell};
use std::time::Duration;

/// Cascade runs in `post_record` (after each pass's measure+arrange),
/// not in `finalize_frame`. Means a `request_relayout` re-record can
/// read pass A's arranged rect via `response_for(id).rect` — the
/// invariant `ContextMenu::show` relies on to clamp its anchor in
/// the same frame as the first open, and the general API contract
/// for any widget that needs its own size mid-frame.
#[test]
fn cascade_visible_to_relayout_pass() {
    let pass = Cell::new(0u32);
    let pass_a_rect = Cell::new(None::<Rect>);
    let pass_b_rect = Cell::new(None::<Rect>);
    let id_salt = "cascade-relayout-probe";

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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

#[test]
fn prev_frame_empty_before_first_frame() {
    let h = UiHarness::new(SURFACE);
    assert!(h.ui.damage_engine.prev.is_empty());
}

/// Pin the row invariant: after the first frame, widgets with paint
/// rows land in `prev` — painting widgets with their arranged rect and
/// authoring hash, and chromeless parents via their child-marker rows
/// (paint-order tracking), whose all-zero screens union to no paint
/// extent. A rowless node (childless Panel without chrome) stays out.
#[test]
fn prev_frame_captures_nodes_with_rows() {
    let mut h = UiHarness::new(SURFACE);
    let mut frame_node = None;
    h.frame(|ui| {
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
    let prev = &h.ui.damage_engine.prev;
    let snap = &prev[&WidgetId::from_hash("a")];

    assert!(prev.contains_key(&WidgetId::from_hash("root")));
    assert!(!prev.contains_key(&WidgetId::from_hash("empty")));
    assert_eq!(
        h.ui.damage_engine
            .prev_paint_rect(WidgetId::from_hash("root")),
        None,
    );
    assert_eq!(
        h.ui.damage_engine
            .prev_paint_rect(WidgetId::from_hash("a"))
            .unwrap(),
        h.ui.layout[Layer::Main].rect[frame_node.idx()],
    );
    assert_eq!(
        snap.hash,
        h.ui.forest.trees[Layer::Main].rollups.node[frame_node.idx()],
    );
}

#[test]
fn prev_frame_drops_disappeared_widgets() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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
        h.ui.damage_engine
            .prev
            .contains_key(&WidgetId::from_hash("gone"))
    );

    h.frame(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    assert!(
        !h.ui
            .damage_engine
            .prev
            .contains_key(&WidgetId::from_hash("gone"))
    );
}

#[test]
fn prev_frame_updates_on_authoring_change() {
    let mut h = UiHarness::new(SURFACE);
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
    h.frame(paint(Color::rgb(0.2, 0.4, 0.8)));
    let h1 = h.ui.damage_engine.prev[&WidgetId::from_hash("a")].hash;

    h.frame(paint(Color::rgb(0.9, 0.4, 0.8)));
    let h2 = h.ui.damage_engine.prev[&WidgetId::from_hash("a")].hash;
    assert_ne!(h1, h2);
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

    fn build_target(ui: &mut Ui) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .sense(Sense::CLICK)
            .focusable(true)
            .show(ui, |_| {});
    }

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
        let mut h = UiHarness::new(UVec2::new(100, 100));
        // Baseline frame so the under-test `frame` diffs against a real
        // prior recording, not the never-painted initial state.
        h.frame(build_target);
        prime(&mut h.ui);

        let count = Cell::new(0u32);
        let render_frame_before = h.ui.frame_runtime.render_frame_id;
        let _ = h.frame(|ui| {
            count.set(count.get() + 1);
            build_target(ui);
        });
        assert_eq!(
            count.get() as usize,
            *expected,
            "{label}: expected {expected} build invocation(s), got {}",
            count.get(),
        );
        // The render frame id must bump exactly once per `frame`
        // regardless of pass count — pass B's anim ticks must see the same
        // id as pass A's so the integrator doesn't double-advance.
        assert_eq!(
            h.ui.frame_runtime.render_frame_id,
            render_frame_before + 1,
            "{label}: render_frame_id must bump once per frame (passes: {expected})",
        );
    }
}

/// A routed action requests pass B, but its edge is visible only in pass A.
/// This lets application code handle a widget action inline without replaying
/// the effect.
#[test]
fn action_effect_runs_once_across_record_replay() {
    let surface = UVec2::new(100, 100);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Button::new()
            .id(WidgetId::from_hash("action"))
            .label("Run")
            .size((100.0, 100.0))
            .show(ui)
            .left
            .clicked()
    };

    h.frame(|ui| {
        let _ = build(ui);
    });
    h.press_at(Vec2::new(10.0, 10.0));
    h.release();

    let mut passes = 0;
    let mut effects = 0;
    let _ = h.at(Duration::from_millis(16)).frame(|ui| {
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

    let mut h = UiHarness::new(UVec2::new(100, 100));
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });

    // Frame A: idle, no repaint request, now = 16ms.
    let repaint = h
        .at(Duration::from_millis(16))
        .frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
        })
        .repaint_requested;
    assert!(
        !repaint,
        "no animate-not-settled flag set — must stay false"
    );
    assert_eq!(h.ui.frame_runtime.time, Duration::from_millis(16));
    assert!(
        (h.ui.frame_runtime.dt - 0.016).abs() < 1e-6,
        "FrameRuntime::dt should be (now - prev) in seconds; got {}",
        h.ui.frame_runtime.dt,
    );

    // Frame B: simulate an unsettled animation tick by setting the
    // internal flag during recording. The flag must reach `FrameOutput`.
    let repaint = h
        .at(Duration::from_millis(32))
        .frame(|ui| {
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
    assert_eq!(h.ui.frame_runtime.time, Duration::from_millis(32));
    assert!(
        (h.ui.frame_runtime.dt - 0.016).abs() < 1e-6,
        "FrameRuntime::dt should be next-frame delta; got {}",
        h.ui.frame_runtime.dt,
    );

    // Frame C: oversized gap (5s) clamps dt to MAX_DT; `time` still
    // tracks true clock so animation math doesn't teleport.
    let _ = h.at(Duration::from_millis(5_032)).frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    assert_eq!(h.ui.frame_runtime.time, Duration::from_millis(5_032));
    assert!(
        (h.ui.frame_runtime.dt - MAX_DT).abs() < 1e-6,
        "FrameRuntime::dt should clamp at MAX_DT; got {}",
        h.ui.frame_runtime.dt,
    );

    // Frame D: prior frame's repaint_requested must NOT leak — resets
    // at the top of every `frame` regardless of pass count.
    let repaint = h
        .at(Duration::from_millis(5_048))
        .frame(|ui| {
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

/// A relayout request forces a second record pass, exactly as pending
/// action input does. `frame_value` still records both — skipping pass B
/// would leave an empty tree — but hands back pass A's value, because
/// pass A is the one that observes one-frame edges.
#[test]
fn frame_value_records_both_relayout_passes_and_returns_the_first() {
    let mut h = UiHarness::new(COLD);
    let mut calls = 0_u32;

    let captured = h.frame_value(|ui| {
        calls += 1;
        if calls == 1 {
            ui.request_relayout();
        }
        calls
    });

    assert_eq!(calls, 2, "relayout runs exactly two record passes");
    assert_eq!(captured, 1, "capture returns the input-observing pass");
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

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| build(ui, 50.0));
    assert!(
        h.ui.frame_runtime.cascade_ran(),
        "first frame runs the cascade"
    );

    h.frame(|ui| build(ui, 50.0));
    assert!(
        !h.ui.frame_runtime.cascade_ran(),
        "unchanged frame skips the cascade"
    );

    h.frame(|ui| build(ui, 80.0));
    assert!(
        h.ui.frame_runtime.cascade_ran(),
        "authoring change re-runs the cascade"
    );

    h.frame(|ui| build(ui, 80.0));
    assert!(
        !h.ui.frame_runtime.cascade_ran(),
        "settles back to skipping"
    );

    h.resize(UVec2::new(SURFACE.x + 1, SURFACE.y));
    h.frame(|ui| build(ui, 80.0));
    assert!(
        h.ui.frame_runtime.cascade_ran(),
        "exact-surface change re-runs the cascade"
    );
}

/// O5 stage-0 completeness for the *authoring* cascade inputs. The
/// fingerprint trusts `subtree_hash` to capture everything the cascade
/// reads (transforms, clip / disabled / focusable, visibility, chrome,
/// shapes); if a future input stops being folded in, a frame toggling
/// it would wrongly skip the cascade and paint stale. One arm per
/// attribute class — each toggles a single attribute and asserts the
/// skip is busted. Scroll offset and zoom are authored transforms and
/// are pinned separately by
/// `widgets::scroll::tests::cascade_skip_busts_on_scroll_offset_change`.
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
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| base(ui));
        assert!(
            h.ui.frame_runtime.cascade_ran(),
            "{label}: first frame runs the cascade"
        );
        h.frame(|ui| base(ui));
        assert!(
            !h.ui.frame_runtime.cascade_ran(),
            "{label}: unchanged frame skips the cascade"
        );
        h.frame(|ui| changed(ui));
        assert!(
            h.ui.frame_runtime.cascade_ran(),
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

    let mut h = UiHarness::new(SURFACE);
    let open = WindowToken(7);
    let close = WindowToken(3);

    h.frame(|ui| {
        ui.open_window(open, WindowConfig::new("inspector"));
        ui.close_window(close);
    });

    // Filed during record, still pending after the frame returned —
    // nothing in the frame pipeline clears them.
    assert_eq!(h.ui.window_requests.commands.opens.len(), 1);
    assert_eq!(h.ui.window_requests.commands.opens[0].token, open);
    assert_eq!(
        h.ui.window_requests.commands.opens[0].config.title,
        "inspector"
    );
    assert_eq!(h.ui.window_requests.commands.closes, vec![close]);

    // A quiet frame (no new requests) must not drop the still-undrained
    // queue — the host might not have ticked between these two frames.
    h.frame(|_| {});
    assert_eq!(
        h.ui.window_requests.commands.opens.len(),
        1,
        "queue must outlive a quiet frame"
    );
    assert_eq!(h.ui.window_requests.commands.closes, vec![close]);

    // The host drains by `append`/`drain`-ing the vecs; emulate that and
    // confirm a third frame leaves them empty (no re-queue).
    h.ui.window_requests.commands.opens.clear();
    h.ui.window_requests.commands.closes.clear();
    h.frame(|_| {});
    assert!(h.ui.window_requests.commands.opens.is_empty());
    assert!(h.ui.window_requests.commands.closes.is_empty());

    // `window_open` polls the host-refreshed live set (here set directly,
    // as the host would before each frame) — not the pending queues.
    assert!(!h.ui.window_open(open), "empty live set ⇒ nothing open");
    h.ui.resources.windows.set_live(open, true);
    assert!(h.ui.window_open(open));
    assert!(!h.ui.window_open(close), "only `open` is live");

    h.ui.window_frame.position = Some(IVec2::new(-120, 48));
    h.ui.window_frame.maximized = true;
    let geometry = h.ui.window_geometry();
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
    let mut h = UiHarness::new(SURFACE);

    // No close pending: the flag is false and keep_open never fires.
    h.frame(|ui| {
        assert!(
            !ui.close_requested(),
            "no close pending ⇒ close_requested() false"
        );
    });
    assert!(!h.ui.window_requests.close_vetoed);

    // Host signals a close; an app that vetoes keeps the window open.
    h.ui.window_frame.close_requested = true;
    h.ui.window_requests.close_vetoed = false;
    h.frame(|ui| {
        assert!(
            ui.close_requested(),
            "host signalled close ⇒ close_requested() true"
        );
        ui.keep_open();
    });
    assert!(
        h.ui.window_requests.close_vetoed,
        "keep_open must set the veto the host reads"
    );
    let should_close = h.ui.window_frame.close_requested && !h.ui.window_requests.close_vetoed;
    assert!(
        !should_close,
        "a vetoed request must NOT resolve to a close"
    );

    // Same signal, app ignores it: resolves to a real close. (The host
    // resets the veto before every draw.)
    h.ui.window_requests.close_vetoed = false;
    h.frame(|ui| {
        assert!(ui.close_requested());
    });
    assert!(!h.ui.window_requests.close_vetoed, "untouched ⇒ no veto");
    let should_close = h.ui.window_frame.close_requested && !h.ui.window_requests.close_vetoed;
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
        ui.layer(layer).at(Vec2::new(10.0, 10.0)).show(|ui| {
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
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| base(ui));
        h.frame(|ui| base(ui));
        assert!(
            !h.ui.frame_runtime.cascade_ran(),
            "{label}: unchanged frame skips the cascade"
        );
        h.frame(|ui| changed(ui));
        assert!(
            h.ui.frame_runtime.cascade_ran(),
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
    let mut h = UiHarness::new(SURFACE);
    let run = |h: &mut UiHarness, disabled: bool| {
        let mut resp = None;
        h.frame(|ui| {
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
    run(&mut h, false);
    h.move_to(Vec2::new(10.0, 10.0));
    let enabled = run(&mut h, false);
    assert!(enabled.hovered, "sanity: pointer hovers the button");
    assert!(!enabled.disabled);
    // Disable frame: stale cascade still routes the hover; the read
    // must mask it.
    let disabled = run(&mut h, true);
    assert!(disabled.disabled, "ancestor-disabled ORs in lag-free");
    assert!(
        !disabled.hovered,
        "interactions must mask on the disable frame"
    );

    use crate::primitives::color::ColorF16;
    use crate::scene::shapes::paint::ShapeBrush;

    let self_id = WidgetId::from_hash("self-disabled");
    let disabled_fill = Color::rgb(0.8, 0.1, 0.2);
    let mut style = h.ui.theme.button.clone();
    style.looks.disabled.background = Background::fill(disabled_fill);
    let response = h.frame_value(|ui| {
        Button::new()
            .id(self_id)
            .label("disabled")
            .style(&style)
            .disabled(true)
            .show(ui)
            .snapshot()
    });
    assert!(
        response.disabled,
        "a self-disabled widget reports disabled on its own first frame, \
         before the cascade has seen it",
    );
    let endpoint = h.ui.cascade.by_id[&self_id];
    let chrome = h.ui.forest.trees[endpoint.layer]
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

/// Record passes replay (cold-start warmup, double-layout pass B), so
/// one logical `open_window` call reaches the queue two or three times
/// per frame — dedup by token, last config wins.
#[test]
fn open_window_dedups_by_token_within_a_frame() {
    use crate::window::{WindowConfig, WindowToken};
    let mut h = UiHarness::new(SURFACE);
    let cfg = WindowConfig::new;
    h.ui.open_window(WindowToken(7), cfg("first"));
    h.ui.open_window(WindowToken(7), cfg("second"));
    h.ui.open_window(WindowToken(8), cfg("other"));
    assert_eq!(h.ui.window_requests.commands.opens.len(), 2);
    assert_eq!(h.ui.window_requests.commands.opens[0].token, WindowToken(7));
    assert_eq!(
        h.ui.window_requests.commands.opens[0].config.title,
        "second"
    );
    assert_eq!(h.ui.window_requests.commands.opens[1].token, WindowToken(8));
}

/// The theme accessors' sharing contract, which is the whole point of
/// storing it behind an `Rc`: reads hand back a handle so the widgets
/// that need a bundle across a `&mut Ui` reborrow pay a refcount bump
/// rather than copying one; writes are copy-on-write, so a live handle
/// keeps the values it was taken with.
///
/// The `ptr_eq` assertion is the load-bearing one. If `Ui::theme` ever
/// went back to returning `&Theme`, every `ui.theme().clone()` call site
/// in the crate would still compile — and silently deep-copy ~9 KB of
/// bundles per widget per frame instead.
#[test]
fn theme_reads_share_and_writes_copy_on_write() {
    use crate::widgets::theme::Theme;
    use std::rc::Rc;

    let mut h = UiHarness::new(SURFACE);
    let clear = Color::rgb(0.25, 0.5, 0.75);
    h.ui.theme_mut().window_clear = clear;

    let handle = h.ui.theme().clone();
    assert!(
        Rc::ptr_eq(&handle, h.ui.theme()),
        "a theme read must hand back the same allocation, not a copy",
    );

    // Write with the handle alive: the `Ui` moves, the handle does not.
    let recolored = Color::rgb(0.1, 0.2, 0.3);
    h.ui.theme_mut().window_clear = recolored;
    assert_eq!(h.ui.theme().window_clear, recolored);
    assert_eq!(
        handle.window_clear, clear,
        "an outstanding handle must keep the values it was taken with",
    );
    assert!(
        !Rc::ptr_eq(&handle, h.ui.theme()),
        "the copy-on-write split must give the `Ui` a fresh allocation",
    );

    // With the handle dropped, the next write mutates in place.
    drop(handle);
    let before = Rc::as_ptr(h.ui.theme());
    h.ui.theme_mut().window_clear = clear;
    assert_eq!(
        Rc::as_ptr(h.ui.theme()),
        before,
        "an unshared theme must be written in place, with no copy",
    );

    // `set_theme` takes the handle, so swapping whole themes is a move.
    let swapped: Rc<Theme> = Rc::new(Theme::default());
    let swapped_ptr = Rc::as_ptr(&swapped);
    h.ui.set_theme(swapped);
    assert_eq!(Rc::as_ptr(h.ui.theme()), swapped_ptr);
}
