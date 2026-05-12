use crate::TextStyle;
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::{Layer, NodeId};
use crate::forest::widget_id::WidgetId;
use crate::layout::types::display::Display;
use crate::primitives::{color::Color, rect::Rect};
use crate::support::testing::{new_ui_text, run_at, run_at_acked, ui_at};
use crate::ui::damage::Damage;
use crate::widgets::theme::Background;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(200, 200);

fn measure_calls(ui: &Ui) -> u64 {
    crate::support::internals::text_shaper_measure_calls(&ui.text)
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
        .node
}

#[test]
#[should_panic(expected = "WidgetId collision")]
fn duplicate_widget_id_panics() {
    // Two `Button::new().id_salt("dup")` calls in one frame produce the same
    // `WidgetId`, which would silently corrupt every per-id store. `Ui::node`
    // enforces uniqueness with a release `assert!`.
    let mut ui = Ui::new();
    run_at(&mut ui, UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new().id_salt("dup").show(ui);
            Button::new().id_salt("dup").show(ui);
        });
    });
}

/// Auto-generated ids (call-site hash) silently disambiguate when the same
/// site fires more than once per frame — the "loop / closure helper" case.
#[test]
fn auto_id_collisions_disambiguate() {
    fn chip(ui: &mut Ui) {
        Frame::new().auto_id().show(ui);
    }
    let mut ui = Ui::new();
    run_at(&mut ui, UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            chip(ui);
            chip(ui);
            chip(ui);
        });
    });
    // 1 panel + 3 chips = 4 distinct ids, no panic.
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 4);
}

/// Pin: an empty frame drives the full pipeline without panicking and
/// produces no draw commands.
#[test]
fn empty_ui_drives_a_frame_safely() {
    use crate::renderer::frontend::Frontend;

    let mut ui = Ui::new();
    run_at(&mut ui, SURFACE, |_| {});

    // Empty UI on the first frame: damage is `None` (skip). Force `Full`
    // to exercise encode/compose and assert the buffers come out empty.
    let mut frontend = Frontend::default();
    let buffer = frontend.build(&ui, Damage::Full);
    assert!(buffer.quads.is_empty());
    assert!(buffer.texts.is_empty());
    assert!(buffer.groups.is_empty());

    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 0);
    assert!(ui.damage_engine.prev.is_empty());
    assert!(ui.damage_engine.dirty.is_empty());
    assert!(ui.damage_engine.region.is_empty());
    assert_eq!(ui.damage_engine.filter(ui.display.logical_rect()), None);
}

/// Pin: an empty frame followed by a populated frame works (the
/// recorder retains no per-frame state across frames).
#[test]
fn empty_then_populated_frame() {
    let mut ui = Ui::new();
    run_at_acked(&mut ui, UVec2::new(100, 100), |_| {});
    run_at_acked(&mut ui, UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |_| {});
    });
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 1);
    // Root Panel is non-painting (no chrome, no shapes) so prev stays
    // empty — only painting widgets are tracked.
    assert!(ui.damage_engine.prev.is_empty());
}

/// Pin: `Ui::frame` panics if `display.scale_factor` is below `EPS`.
#[test]
#[should_panic(expected = "Display::scale_factor must be ≥ EPSILON")]
fn frame_rejects_zero_scale_factor() {
    let mut ui = Ui::new();
    let _ = ui.frame(
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
    let ui = Ui::new();
    assert!(ui.damage_engine.prev.is_empty());
}

/// Pin: after the first frame, painting widgets land in `prev` with
/// their arranged rect and authoring hash; non-painting widgets (Panel
/// without chrome) don't.
#[test]
fn prev_frame_captures_painting_nodes() {
    let mut ui = Ui::new();
    let mut frame_node = None;
    run_at(&mut ui, SURFACE, |ui| {
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
    let mut ui = Ui::new();
    run_at_acked(&mut ui, SURFACE, |ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Button::new().id_salt("gone").label("X").show(ui);
        });
    });
    assert!(
        ui.damage_engine
            .prev
            .contains_key(&WidgetId::from_hash("gone"))
    );

    run_at_acked(&mut ui, SURFACE, |ui| {
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
    let mut ui = Ui::new();
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
    run_at_acked(&mut ui, SURFACE, paint(Color::rgb(0.2, 0.4, 0.8)));
    let h1 = ui.damage_engine.prev[&WidgetId::from_hash("a")].hash;

    run_at_acked(&mut ui, SURFACE, paint(Color::rgb(0.9, 0.4, 0.8)));
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
        let mut ui = new_ui_text();
        run_at_acked(&mut ui, UVec2::new(400, 200), build);
        let after_first = measure_calls(&ui);
        assert!(
            after_first > 0,
            "{label}: first frame should drive at least one measure call",
        );
        run_at_acked(&mut ui, UVec2::new(400, 200), build);
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
    let mut ui = new_ui_text();
    run_at_acked(&mut ui, UVec2::new(400, 200), render("first"));
    let before = measure_calls(&ui);
    run_at_acked(&mut ui, UVec2::new(400, 200), render("second"));
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

    let mut ui = new_ui_text();
    run_at_acked(&mut ui, UVec2::new(400, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("hello").id_salt("transient").show(ui);
        });
    });
    let wid = WidgetId::from_hash("transient");
    assert!(
        crate::support::internals::text_shaper_has_reuse_entry(&ui.text, wid, 0),
        "text widget should populate text_reuse on first render",
    );

    run_at_acked(&mut ui, UVec2::new(400, 200), |ui| {
        Panel::vstack().auto_id().show(ui, |_| {});
    });
    assert!(
        !crate::support::internals::text_shaper_has_reuse_entry(&ui.text, wid, 0),
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

    let mut ui = new_ui_text();
    run_at_acked(&mut ui, UVec2::new(400, 200), render(60.0));
    let after_first = measure_calls(&ui);
    assert!(
        after_first >= 2,
        "first frame should measure both unbounded and wrap (got {after_first})",
    );
    run_at_acked(&mut ui, UVec2::new(400, 200), render(80.0));
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
    let mut ui = ui_at(UVec2::new(100, 100));
    let id_a = WidgetId::from_hash("a");
    let id_b = WidgetId::from_hash("b");

    run_at_acked(&mut ui, UVec2::new(100, 100), |ui| {
        Frame::new().id_salt("a").show(ui);
        Frame::new().id_salt("b").show(ui);
        *ui.state_mut::<u32>(id_a) = 11;
        *ui.state_mut::<u32>(id_b) = 22;
    });
    run_at_acked(&mut ui, UVec2::new(100, 100), |ui| {
        Frame::new().id_salt("a").show(ui);
        // Reading state during recording so the row is touched while
        // its widget is still seen.
        assert_eq!(*ui.state_mut::<u32>(id_a), 11);
    });
    run_at_acked(&mut ui, UVec2::new(100, 100), |ui| {
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
    use crate::input::keyboard::{Key, Modifiers};
    use crate::input::{InputEvent, PointerButton};
    use glam::Vec2;
    use std::cell::Cell;

    let display = Display::from_physical(UVec2::new(100, 100), 1.0);
    type Prime = fn(&mut Ui);
    let cases: &[(&str, Prime, usize)] = &[
        ("idle", |_ui| {}, 1),
        (
            "hover only",
            |ui| ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0))),
            1,
        ),
        (
            "modifiers only",
            |ui| ui.on_input(InputEvent::ModifiersChanged(Modifiers::NONE)),
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
                })
            },
            2,
        ),
        (
            "scroll",
            |ui| ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 10.0))),
            2,
        ),
    ];

    for (label, prime, expected) in cases {
        let mut ui = Ui::new();
        // Baseline frame so the under-test `frame` diffs against a real
        // prior recording, not the never-painted initial state.
        run_at_acked(&mut ui, UVec2::new(100, 100), |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
        });
        prime(&mut ui);

        let count = Cell::new(0u32);
        let frame_id_before = ui.frame_id;
        let _ = ui.frame(display, Duration::ZERO, |ui| {
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

    let mut ui = Ui::new();
    run_at_acked(&mut ui, UVec2::new(100, 100), |ui| {
        Panel::vstack().id_salt("root").show(ui, |_| {});
    });

    // Frame A: idle, no repaint request, now = 16ms.
    let repaint = ui
        .frame(display, Duration::from_millis(16), |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
        })
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
        .frame(display, Duration::from_millis(32), |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
            ui.repaint_requested = true;
        })
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
    let _ = ui.frame(display, Duration::from_millis(5_032), |ui| {
        Panel::vstack().id_salt("root").show(ui, |_| {});
    });
    assert_eq!(ui.time, Duration::from_millis(5_032));
    assert!(
        (ui.dt - MAX_DT).abs() < 1e-6,
        "Ui::dt should clamp at MAX_DT; got {}",
        ui.dt,
    );

    // Frame D: prior frame's repaint_requested must NOT leak — resets
    // at the top of every `frame` regardless of pass count.
    let repaint = ui
        .frame(display, Duration::from_millis(5_048), |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
        })
        .repaint_requested();
    assert!(
        !repaint,
        "repaint_requested must reset at the top of frame()",
    );
}
