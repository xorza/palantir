use crate::Ui;
use crate::input::InputEvent;
use crate::layout::types::display::Display;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::support::testing::{ui_at, under_outer};
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::{Scroll, ScrollState};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 600);

fn surface_display() -> Display {
    Display::from_physical(SURFACE, 1.0)
}

/// Wrap the scroll under a `Panel::vstack` root so its `Sizing::Fixed`
/// is honored. The root expands to surface; the panel's `vstack` slot
/// then hands the scroll exactly its declared size.
fn build(ui: &mut crate::ui::Ui, viewport_h: f32, content_h: f32) {
    Panel::vstack().with_id("root").show(ui, |ui| {
        Scroll::vertical()
            .with_id("scroll")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(viewport_h)))
            .show(ui, |ui| {
                Frame::new()
                    .with_id("content")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(content_h)))
                    .show(ui);
            });
    });
}

fn read_state(ui: &mut crate::ui::Ui) -> ScrollState {
    *ui.state
        .get_or_insert_with::<ScrollState, _>(WidgetId::from_hash("scroll"), Default::default)
}

#[test]
fn scroll_state_records_viewport_and_content_after_arrange() {
    let mut ui = ui_at(SURFACE);
    build(&mut ui, 200.0, 800.0);
    ui.end_frame();

    let row = read_state(&mut ui);
    assert_eq!(row.viewport.h, 200.0);
    assert_eq!(row.content.h, 800.0);
    assert_eq!(row.offset, Vec2::ZERO, "no wheel input → offset stays at 0");
}

#[test]
fn wheel_delta_advances_offset_clamped_to_max() {
    let mut ui = ui_at(SURFACE);
    build(&mut ui, 200.0, 800.0);
    ui.end_frame();

    // Pointer over scroll viewport (root vstack starts at (0,0); scroll is
    // the only child; viewport is the top 200px of the surface).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 50.0)));

    ui.begin_frame(surface_display());
    build(&mut ui, 200.0, 800.0);
    ui.end_frame();

    assert_eq!(
        read_state(&mut ui).offset.y,
        50.0,
        "wheel delta accumulates into offset",
    );

    // Huge wheel push → clamps to (content - viewport) = 600.
    ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 9_999.0)));
    ui.begin_frame(surface_display());
    build(&mut ui, 200.0, 800.0);
    ui.end_frame();

    assert_eq!(
        read_state(&mut ui).offset.y,
        600.0,
        "offset clamps to content - viewport",
    );
}

#[test]
fn non_overflowing_content_keeps_offset_at_zero() {
    let mut ui = ui_at(SURFACE);
    build(&mut ui, 300.0, 100.0);
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 500.0)));

    ui.begin_frame(surface_display());
    build(&mut ui, 300.0, 100.0);
    ui.end_frame();

    assert_eq!(
        read_state(&mut ui).offset,
        Vec2::ZERO,
        "wheel input over non-overflowing content has nowhere to go",
    );
}

#[test]
fn horizontal_scroll_pans_only_x() {
    let mut ui = ui_at(SURFACE);
    let build_h = |ui: &mut crate::ui::Ui| {
        Panel::vstack().with_id("root").show(ui, |ui| {
            Scroll::horizontal()
                .with_id("hscroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(40.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .with_id("hcontent")
                        .size((Sizing::Fixed(800.0), Sizing::Fixed(40.0)))
                        .show(ui);
                });
        });
    };
    build_h(&mut ui);
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0)));
    // Touchpad / wheel deltas come in on both axes — verify only X
    // makes it into the offset for a horizontal scroll.
    ui.on_input(InputEvent::Scroll(Vec2::new(75.0, 200.0)));

    ui.begin_frame(surface_display());
    build_h(&mut ui);
    ui.end_frame();

    let id = WidgetId::from_hash("hscroll");
    let row = *ui
        .state
        .get_or_insert_with::<ScrollState, _>(id, Default::default);
    assert_eq!(row.offset, Vec2::new(75.0, 0.0));
}

#[test]
fn both_axis_scroll_pans_both_axes() {
    let mut ui = ui_at(SURFACE);
    let build_xy = |ui: &mut crate::ui::Ui| {
        Panel::vstack().with_id("root").show(ui, |ui| {
            Scroll::both()
                .with_id("xy")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .with_id("xy-content")
                        .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
                        .show(ui);
                });
        });
    };
    build_xy(&mut ui);
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::Scroll(Vec2::new(40.0, 60.0)));

    ui.begin_frame(surface_display());
    build_xy(&mut ui);
    ui.end_frame();

    let id = WidgetId::from_hash("xy");
    let row = *ui
        .state
        .get_or_insert_with::<ScrollState, _>(id, Default::default);
    assert_eq!(row.offset, Vec2::new(40.0, 60.0));
    assert_eq!(
        row.content,
        crate::primitives::size::Size::new(800.0, 800.0)
    );
    assert_eq!(
        row.viewport,
        crate::primitives::size::Size::new(200.0, 200.0)
    );
}

// --- Measure-side: scroll_content captured by the layout pass ---------------
// Pin the contract between the `Scroll(axes)` arm of `measure_dispatch` and
// `LayoutResult.scroll_content`: content extent lands there, the viewport's
// own desired stays at zero on the panned axes (so `resolve_desired` falls
// through to the user's `Sizing`).

#[test]
fn scroll_v_records_content_height_and_yields_panned_axis_to_self_sizing() {
    let mut ui = Ui::new();
    let scroll_node = under_outer(&mut ui, UVec2::new(400, 600), |ui| {
        Scroll::vertical()
            .with_id("scroll")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .gap(4.0)
            .show(ui, |ui| {
                // Three rows of 28h with 4px gap → 28*3 + 4*2 = 92.
                for i in 0..3u32 {
                    Frame::new()
                        .with_id(("row", i))
                        .size((Sizing::Fixed(180.0), Sizing::Fixed(28.0)))
                        .show(ui);
                }
            })
            .node
    });

    let rect = ui.layout_engine.result.rect[scroll_node.index()];
    let content = ui.layout_engine.result.scroll_content[scroll_node.index()];
    assert_eq!(
        rect.size.h, 200.0,
        "viewport honors Fixed h, ignores content"
    );
    assert_eq!(content.h, 92.0, "stack(Y) sum + (n-1)·gap");
    assert_eq!(content.w, 180.0, "stack(Y) cross = max child width");
}

#[test]
fn scroll_h_records_content_width_and_yields_panned_axis_to_self_sizing() {
    let mut ui = Ui::new();
    let scroll_node = under_outer(&mut ui, UVec2::new(800, 200), |ui| {
        Scroll::horizontal()
            .with_id("scroll")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(60.0)))
            .gap(8.0)
            .show(ui, |ui| {
                // Two cols of 60w with 8 gap → 60*2 + 8 = 128.
                for i in 0..2u32 {
                    Frame::new()
                        .with_id(("col", i))
                        .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                        .show(ui);
                }
            })
            .node
    });

    let rect = ui.layout_engine.result.rect[scroll_node.index()];
    let content = ui.layout_engine.result.scroll_content[scroll_node.index()];
    assert_eq!(rect.size.w, 200.0);
    assert_eq!(content.w, 128.0);
    assert_eq!(content.h, 40.0);
}

#[test]
fn scroll_xy_records_max_per_axis() {
    // ZStack-flavored: children overlap at (0,0) inside the viewport.
    // Content extent = max child per axis (not sum).
    let mut ui = Ui::new();
    let scroll_node = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Scroll::both()
            .with_id("scroll")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .show(ui, |ui| {
                Frame::new()
                    .with_id("wide")
                    .size((Sizing::Fixed(300.0), Sizing::Fixed(60.0)))
                    .show(ui);
                Frame::new()
                    .with_id("tall")
                    .size((Sizing::Fixed(80.0), Sizing::Fixed(250.0)))
                    .show(ui);
            })
            .node
    });

    let content = ui.layout_engine.result.scroll_content[scroll_node.index()];
    assert_eq!(content.w, 300.0, "max child width");
    assert_eq!(content.h, 250.0, "max child height");
}

#[test]
fn scroll_with_no_children_records_zero_content() {
    let mut ui = Ui::new();
    let scroll_node = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Scroll::vertical()
            .with_id("empty")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .show(ui, |_| {})
            .node
    });

    let content = ui.layout_engine.result.scroll_content[scroll_node.index()];
    assert_eq!(content, Size::ZERO);
}

#[test]
fn scroll_content_survives_measure_cache_hit() {
    // Two frames with identical input: first frame populates
    // `scroll_content` from the live measure; second frame's measure
    // cache short-circuits an ancestor and must restore `scroll_content`
    // verbatim from the snapshot. Pins that the cache plumbing carries
    // the column rather than re-deriving it.
    let surface = UVec2::new(400, 600);
    let display = Display::from_physical(surface, 1.0);
    let build = |ui: &mut Ui| {
        Panel::vstack().with_id("root").show(ui, |ui| {
            Scroll::vertical()
                .with_id("scroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .gap(4.0)
                .show(ui, |ui| {
                    for i in 0..3u32 {
                        Frame::new()
                            .with_id(("row", i))
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(28.0)))
                            .show(ui);
                    }
                });
        });
    };

    let mut ui = ui_at(surface);
    build(&mut ui);
    ui.end_frame();
    let scroll_id = WidgetId::from_hash("scroll");
    let after_first = *ui
        .state
        .get_or_insert_with::<ScrollState, _>(scroll_id, Default::default);
    assert_eq!(after_first.content.h, 92.0);

    ui.begin_frame(display);
    build(&mut ui);
    ui.end_frame();
    let after_second = *ui
        .state
        .get_or_insert_with::<ScrollState, _>(scroll_id, Default::default);
    assert_eq!(
        after_second.content, after_first.content,
        "scroll_content survives a measure-cache hit"
    );
    assert_eq!(after_second.viewport, after_first.viewport);
}
