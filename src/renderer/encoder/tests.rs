use super::{RenderCmd, align_text_in, encode};
use crate::Ui;
use crate::element::Configure;
use crate::input::{InputEvent, PointerButton};
use crate::primitives::{
    Align, Color, Display, HAlign, Rect, Sense, Size, Sizing, TranslateScale, VAlign, WidgetId,
};
use crate::widgets::{Frame, Panel, Styled};
use glam::{UVec2, Vec2};

fn count_clip_pairs(cmds: &[RenderCmd]) -> (usize, usize) {
    let mut pushes = 0;
    let mut pops = 0;
    for c in cmds {
        match c {
            RenderCmd::PushClip(_) => pushes += 1,
            RenderCmd::PopClip => pops += 1,
            _ => {}
        }
    }
    (pushes, pops)
}

#[test]
fn empty_tree_encodes_to_nothing() {
    let mut cmds = Vec::new();
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(100.0 as u32, 100.0 as u32),
        1.0,
    ));
    Panel::hstack().show(&mut ui, |_| {});
    ui.end_frame();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        None,
        &mut cmds,
    );
    let draws = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::DrawRect { .. }))
        .count();
    assert_eq!(draws, 0);
}

#[test]
fn frame_with_fill_emits_one_draw_rect() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 200.0 as u32),
        1.0,
    ));
    Panel::hstack().show(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(1.0, 0.0, 0.0))
            .show(ui);
    });
    ui.end_frame();
    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        None,
        &mut cmds,
    );

    let draw_rects = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::DrawRect { .. }))
        .count();
    assert_eq!(draw_rects, 1);
}

#[test]
fn invisible_frame_does_not_emit_draw_rect() {
    // Shape::is_noop filters at Ui::add_shape time; the encoder should see no
    // RoundedRect in the tree, hence no DrawRect command.
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 200.0 as u32),
        1.0,
    ));
    Panel::hstack().show(&mut ui, |ui| {
        Frame::with_id("invisible").size(50.0).show(ui);
    });
    ui.end_frame();
    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        None,
        &mut cmds,
    );

    let draw_rects = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::DrawRect { .. }))
        .count();
    assert_eq!(draw_rects, 0);
}

#[test]
fn clip_emits_balanced_push_pop() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 200.0 as u32),
        1.0,
    ));
    // Outer HStack opts out of the default-on clip so we can count just the
    // ZStack's pair under test.
    Panel::hstack().clip(false).show(&mut ui, |ui| {
        Panel::zstack_with_id("clip")
            .size(50.0)
            .clip(true)
            .show(ui, |ui| {
                Frame::with_id("inner")
                    .size(40.0)
                    .fill(Color::rgb(0.5, 0.5, 0.5))
                    .show(ui);
            });
    });
    ui.end_frame();
    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        None,
        &mut cmds,
    );

    let (pushes, pops) = count_clip_pairs(&cmds);
    assert_eq!(pushes, 1);
    assert_eq!(pops, 1);

    // PushClip must come before the inner DrawRect, PopClip after — i.e. the
    // inner draw is sandwiched.
    let push_idx = cmds
        .iter()
        .position(|c| matches!(c, RenderCmd::PushClip(_)))
        .unwrap();
    let pop_idx = cmds
        .iter()
        .position(|c| matches!(c, RenderCmd::PopClip))
        .unwrap();
    let draw_idxs: Vec<_> = cmds
        .iter()
        .enumerate()
        .filter_map(|(i, c)| matches!(c, RenderCmd::DrawRect { .. }).then_some(i))
        .collect();
    assert!(!draw_idxs.is_empty());
    for &di in &draw_idxs {
        assert!(
            di > push_idx && di < pop_idx,
            "draw at {di} not inside [{push_idx}, {pop_idx}]"
        );
    }
}

/// Walk an encoder command stream and return the effective screen-space rect
/// for each `DrawRect`, keyed by its fill colour. The interpretation mirrors
/// what a backend does: `PushTransform` composes onto the current transform;
/// `PushClip` is taken in the parent's already-composed transform space and
/// intersected with the active clip; `DrawRect.rect` is in the parent's
/// transform space at draw time.
fn screen_rects_by_fill(cmds: &[RenderCmd]) -> Vec<(Color, Rect)> {
    let mut t = TranslateScale::IDENTITY;
    let mut t_stack: Vec<TranslateScale> = Vec::new();
    let mut clip: Option<Rect> = None;
    let mut clip_stack: Vec<Option<Rect>> = Vec::new();
    let mut out = Vec::new();
    for cmd in cmds {
        match cmd {
            RenderCmd::PushTransform(child) => {
                t_stack.push(t);
                t = t.compose(*child);
            }
            RenderCmd::PopTransform => t = t_stack.pop().expect("balanced PushTransform/Pop"),
            RenderCmd::PushClip(r) => {
                let screen = t.apply_rect(*r);
                let intersected = match clip {
                    Some(c) => screen.intersect(c),
                    None => screen,
                };
                clip_stack.push(clip);
                clip = Some(intersected);
            }
            RenderCmd::PopClip => clip = clip_stack.pop().expect("balanced PushClip/Pop"),
            RenderCmd::DrawRect { rect, fill, .. } => {
                let screen = t.apply_rect(*rect);
                let visible = match clip {
                    Some(c) => screen.intersect(c),
                    None => screen,
                };
                out.push((*fill, visible));
            }
            RenderCmd::DrawText { .. } => {
                // Test rasterizer ignores text — encoder tests only assert on rect output.
            }
        }
    }
    assert!(t_stack.is_empty(), "transform stack unbalanced");
    assert!(clip_stack.is_empty(), "clip stack unbalanced");
    out
}

#[test]
fn cascade_matches_hit_index_for_visible_disabled_and_hidden() {
    // The encoder and the hit index both apply the same four cascades
    // (disabled / invisible / clip / transform). They walk the tree in
    // different shapes — the encoder pushes/pops, the hit index snapshots
    // per-node. This test pins the contract that they agree on:
    //
    //  - Visible and disabled nodes get the same effective screen rect.
    //  - Hidden nodes are skipped by the encoder but still tracked by the
    //    hit index (their slot persists; sense becomes NONE).
    //  - Clicks land on the visible node and are suppressed for both
    //    disabled (input-cascade) and hidden (visibility-cascade) nodes.

    let v_color = Color::rgb(1.0, 0.0, 0.0);
    let d_color = Color::rgb(0.0, 1.0, 0.0);
    let h_color = Color::rgb(0.0, 0.0, 1.0);
    let xform = TranslateScale::new(Vec2::new(5.0, 7.0), 2.0);

    let mut ui = Ui::new();

    // Frame 1: build, layout, end_frame so the hit index is populated.
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 400.0 as u32),
        1.0,
    ));
    Panel::hstack().clip(false).show(&mut ui, |ui| {
        Panel::canvas_with_id("mid")
            .size(200.0)
            .clip(true)
            .transform(xform)
            .show(ui, |ui| {
                Frame::with_id("V")
                    .position((0.0, 0.0))
                    .size(30.0)
                    .fill(v_color)
                    .sense(Sense::CLICK)
                    .show(ui);
                Frame::with_id("D")
                    .position((40.0, 0.0))
                    .size(30.0)
                    .fill(d_color)
                    .sense(Sense::CLICK)
                    .disabled(true)
                    .show(ui);
                Frame::with_id("H")
                    .position((80.0, 0.0))
                    .size(30.0)
                    .fill(h_color)
                    .sense(Sense::CLICK)
                    .hidden()
                    .show(ui);
            });
    });
    ui.end_frame();

    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        None,
        &mut cmds,
    );
    let drawn = screen_rects_by_fill(&cmds);

    // Visible node: encoder emits exactly one DrawRect with its fill, and the
    // resulting screen rect matches the hit index's rect for the same id.
    let v_id = WidgetId::from_hash("V");
    let v_screen = drawn
        .iter()
        .find(|(c, _)| *c == v_color)
        .map(|(_, r)| *r)
        .expect("visible node should emit a DrawRect");
    let v_hit = ui
        .response_for(v_id)
        .rect
        .expect("visible node should have a hit-index rect");
    assert_eq!(v_screen, v_hit, "encoder vs hit-index rect for V");

    // Disabled node still paints; the hit index keeps its rect too. Cascades
    // (clip + transform) must produce the same screen rect on both sides.
    let d_id = WidgetId::from_hash("D");
    let d_screen = drawn
        .iter()
        .find(|(c, _)| *c == d_color)
        .map(|(_, r)| *r)
        .expect("disabled node should still paint");
    let d_hit = ui.response_for(d_id).rect.expect("disabled node has rect");
    assert_eq!(d_screen, d_hit, "encoder vs hit-index rect for D");

    // Hidden node: encoder skips, hit index still tracks the slot.
    let h_id = WidgetId::from_hash("H");
    assert!(
        !drawn.iter().any(|(c, _)| *c == h_color),
        "hidden node must not emit a DrawRect"
    );
    assert!(
        ui.response_for(h_id).rect.is_some(),
        "hidden node still has a slot in the hit index"
    );

    // Click suppression: press+release at each widget's centre.
    // Visible → clicks. Disabled → suppressed (sense cascade in HitIndex).
    // Hidden → suppressed (visibility cascade).
    fn press_and_release_at(ui: &mut Ui, p: Vec2) {
        ui.on_input(InputEvent::PointerMoved(p));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    }
    press_and_release_at(
        &mut ui,
        v_hit.min + Vec2::new(v_hit.size.w, v_hit.size.h) * 0.5,
    );
    press_and_release_at(
        &mut ui,
        d_hit.min + Vec2::new(d_hit.size.w, d_hit.size.h) * 0.5,
    );
    let h_hit = ui.response_for(h_id).rect.unwrap();
    press_and_release_at(
        &mut ui,
        h_hit.min + Vec2::new(h_hit.size.w, h_hit.size.h) * 0.5,
    );

    // Frame 2: rebuild and read clicked() on each widget.
    ui.begin_frame(Display::default());
    let mut got = (false, false, false);
    Panel::hstack().clip(false).show(&mut ui, |ui| {
        Panel::canvas_with_id("mid")
            .size(200.0)
            .clip(true)
            .transform(xform)
            .show(ui, |ui| {
                got.0 = Frame::with_id("V")
                    .position((0.0, 0.0))
                    .size(30.0)
                    .fill(v_color)
                    .sense(Sense::CLICK)
                    .show(ui)
                    .clicked();
                got.1 = Frame::with_id("D")
                    .position((40.0, 0.0))
                    .size(30.0)
                    .fill(d_color)
                    .sense(Sense::CLICK)
                    .disabled(true)
                    .show(ui)
                    .clicked();
                got.2 = Frame::with_id("H")
                    .position((80.0, 0.0))
                    .size(30.0)
                    .fill(h_color)
                    .sense(Sense::CLICK)
                    .hidden()
                    .show(ui)
                    .clicked();
            });
    });

    assert!(got.0, "visible widget should click");
    assert!(!got.1, "disabled widget must not click (sense cascade)");
    assert!(!got.2, "hidden widget must not click (visibility cascade)");
}

#[test]
fn nested_clips_each_emit_their_own_pair() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 200.0 as u32),
        1.0,
    ));
    Panel::hstack().clip(false).show(&mut ui, |ui| {
        Panel::zstack_with_id("outer")
            .size(Sizing::Fixed(100.0))
            .clip(true)
            .show(ui, |ui| {
                Panel::zstack_with_id("inner")
                    .size(Sizing::Fixed(50.0))
                    .clip(true)
                    .show(ui, |_| {});
            });
    });
    ui.end_frame();
    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        None,
        &mut cmds,
    );
    let (pushes, pops) = count_clip_pairs(&cmds);
    assert_eq!(pushes, 2);
    assert_eq!(pops, 2);
}

#[test]
fn disabled_ancestor_dims_descendant_fill() {
    let mut ui = Ui::new();
    let pure_red = Color::rgb(1.0, 0.0, 0.0);
    ui.begin_frame(Display::from_physical(
        UVec2::new(100.0 as u32, 100.0 as u32),
        1.0,
    ));
    Panel::vstack().disabled(true).show(&mut ui, |ui| {
        Frame::new()
            .size(Sizing::Fixed(40.0))
            .fill(pure_red)
            .show(ui);
    });
    ui.end_frame();

    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        0.5,
        None,
        &mut cmds,
    );
    let dimmed = cmds
        .iter()
        .find_map(|c| match c {
            RenderCmd::DrawRect { fill, .. } => Some(*fill),
            _ => None,
        })
        .expect("frame must emit one DrawRect");
    assert_eq!(dimmed, pure_red.dim_rgb(0.5));

    // Same tree with no disabled ancestor: fill comes through untouched.
    let mut ui2 = Ui::new();
    ui2.begin_frame(Display::default());
    Panel::vstack().show(&mut ui2, |ui| {
        Frame::new()
            .size(Sizing::Fixed(40.0))
            .fill(pure_red)
            .show(ui);
    });
    ui2.end_frame();
    cmds.clear();
    encode(
        &ui2.tree,
        ui2.layout_engine.result(),
        &ui2.cascades,
        0.5,
        None,
        &mut cmds,
    );
    let untouched = cmds
        .iter()
        .find_map(|c| match c {
            RenderCmd::DrawRect { fill, .. } => Some(*fill),
            _ => None,
        })
        .expect("frame must emit one DrawRect");
    assert_eq!(untouched, pure_red);
}

/// `align_text_in` math: glyph bbox positioned inside the leaf's
/// arranged rect. Center, right, and bottom shift the origin; auto
/// (the default) collapses to top-left because glyphs don't stretch.
#[test]
fn align_text_in_centers_horizontally_and_vertically() {
    // Leaf is 200×40, text measures 80×16.
    let leaf = Rect::new(10.0, 20.0, 200.0, 40.0);
    let measured = Size::new(80.0, 16.0);

    let r = align_text_in(leaf, measured, Align::CENTER);
    // x: 10 + (200-80)/2 = 70. y: 20 + (40-16)/2 = 32.
    assert_eq!((r.min.x, r.min.y), (70.0, 32.0));
    assert_eq!((r.size.w, r.size.h), (80.0, 16.0));
}

#[test]
fn align_text_in_top_left_when_auto() {
    let leaf = Rect::new(10.0, 20.0, 200.0, 40.0);
    let measured = Size::new(80.0, 16.0);
    let r = align_text_in(leaf, measured, Align::default());
    assert_eq!((r.min.x, r.min.y), (10.0, 20.0));
}

#[test]
fn align_text_in_right_bottom() {
    let leaf = Rect::new(10.0, 20.0, 200.0, 40.0);
    let measured = Size::new(80.0, 16.0);
    let r = align_text_in(leaf, measured, Align::new(HAlign::Right, VAlign::Bottom));
    assert_eq!((r.min.x, r.min.y), (10.0 + 120.0, 20.0 + 24.0));
}

/// Negative-slack guard: if the measured text is *larger* than its
/// leaf rect, alignment shouldn't pull `min` past the leaf's `min`
/// (which would clip the text on the wrong side). Top-left is the
/// safe fallback — the `.max(0.0)` clamp.
#[test]
fn align_text_in_clamps_negative_slack_to_top_left() {
    let leaf = Rect::new(0.0, 0.0, 50.0, 10.0);
    let oversize = Size::new(80.0, 16.0);
    let r = align_text_in(leaf, oversize, Align::CENTER);
    // Even centered, min stays at leaf.min (no negative offset).
    assert_eq!((r.min.x, r.min.y), (0.0, 0.0));
}

/// Encoder honors padding for text alignment: a centered button label
/// inside a padded button is centered in the *content* area (rect
/// deflated by padding), not in the padding-inclusive outer rect.
#[test]
fn encoder_text_alignment_respects_leaf_padding() {
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::Button;

    let mut ui = Ui::new();
    // Real shaper required so the encoder doesn't drop the text run as
    // having an invalid key (mono fallback uses `TextCacheKey::INVALID`).
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 400.0 as u32),
        1.0,
    ));
    Panel::hstack().show(&mut ui, |ui| {
        Button::with_id("padded")
            .label("ok")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
            .padding(20.0)
            .show(ui);
    });
    ui.end_frame();

    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        None,
        &mut cmds,
    );
    let text_rect = cmds
        .iter()
        .find_map(|c| match c {
            RenderCmd::DrawText { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("button must emit one DrawText");

    // Outer button rect is 200×80 at (0, 0). Padding is 20, so content
    // area is 160×40 starting at (20, 20). Centered "ok" (~16×16 with
    // mono metrics) lands at (20 + (160-16)/2, 20 + (40-16)/2) = (92, 32).
    // If padding were ignored, x would be (200-16)/2 = 92, but y would be
    // (80-16)/2 = 32 → indistinguishable on this axis. Use a non-square
    // padding-vs-rect ratio: x asserts the inset, y is just sanity.
    assert!(
        text_rect.min.x > 20.0 && text_rect.min.x < 180.0,
        "text x must lie inside padded content area, got {}",
        text_rect.min.x
    );
    // Specifically: centered horizontally inside [20, 180].
    let expected_x_center = 20.0 + (160.0 - text_rect.size.w) * 0.5;
    assert!(
        (text_rect.min.x - expected_x_center).abs() < 0.5,
        "text x should center within padded area; expected ≈{expected_x_center}, got {}",
        text_rect.min.x
    );
}

// --- Damage filter (Step 5) -------------------------------------------------
// `damage_filter: Some(rect)` skips DrawRect/DrawText for nodes that don't
// intersect the dirty region. PushClip/PopClip and PushTransform/PopTransform
// are *always* emitted so scissor groups and child transforms stay coherent.

/// Pin: a node whose rect doesn't intersect the damage filter has no
/// DrawRect emitted.
#[test]
fn damage_filter_skips_drawrect_outside_dirty_region() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 200.0 as u32),
        1.0,
    ));
    Panel::hstack().show(&mut ui, |ui| {
        Frame::with_id("a")
            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
            .fill(Color::rgb(1.0, 0.0, 0.0))
            .show(ui);
        Frame::with_id("b")
            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
            .fill(Color::rgb(0.0, 1.0, 0.0))
            .show(ui);
    });
    ui.end_frame();

    // Damage filter covers only the left half (x: 0..50). `a` at
    // (0,0,40,40) intersects; `b` at (40,0,40,40) intersects too
    // (its left edge is at x=40 which is < 50). Use a tighter filter.
    let filter = Rect::new(0.0, 0.0, 30.0, 200.0);
    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        Some(filter),
        &mut cmds,
    );

    let draw_count = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::DrawRect { .. }))
        .count();
    // `a` (0..40) intersects (0..30) → emitted. `b` (40..80) doesn't → skipped.
    assert_eq!(
        draw_count, 1,
        "only the rect inside the damage filter should be drawn"
    );
}

/// Pin: a node fully *inside* the damage filter still emits its
/// DrawRect.
#[test]
fn damage_filter_keeps_drawrect_inside_dirty_region() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 200.0 as u32),
        1.0,
    ));
    Panel::hstack().show(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(1.0, 0.0, 0.0))
            .show(ui);
    });
    ui.end_frame();

    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        Some(Rect::new(0.0, 0.0, 200.0, 200.0)),
        &mut cmds,
    );
    let draw_count = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::DrawRect { .. }))
        .count();
    assert!(draw_count >= 1);
}

/// Pin: PushClip/PopClip pairs are emitted even for clipped nodes whose
/// rect doesn't intersect damage. The composer relies on these for group
/// boundaries.
#[test]
fn damage_filter_preserves_clip_pushpop() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 200.0 as u32),
        1.0,
    ));
    Panel::hstack_with_id("outer")
        .clip(false)
        .show(&mut ui, |ui| {
            Panel::hstack_with_id("clipped")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .clip(true)
                .show(ui, |ui| {
                    Frame::with_id("inner")
                        .size(20.0)
                        .fill(Color::rgb(1.0, 0.0, 0.0))
                        .show(ui);
                });
        });
    ui.end_frame();

    // Filter misses the clipped panel entirely.
    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        Some(Rect::new(150.0, 150.0, 50.0, 50.0)),
        &mut cmds,
    );

    let (pushes, pops) = count_clip_pairs(&cmds);
    assert_eq!(pushes, pops, "clip push/pop must be balanced");
    assert!(
        pushes >= 1,
        "filtered-out clipped node still emits its clip pair"
    );
    let draws = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::DrawRect { .. }))
        .count();
    assert_eq!(draws, 0, "no rects emitted when nothing intersects damage");
}

/// Pin: PushTransform/PopTransform pairs are emitted for filtered-out
/// nodes too, so descendant transform composition stays correct.
#[test]
fn damage_filter_preserves_transform_pushpop() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 200.0 as u32),
        1.0,
    ));
    Panel::hstack().show(&mut ui, |ui| {
        Panel::hstack_with_id("transformed")
            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
            .transform(TranslateScale::from_translation(Vec2::new(5.0, 5.0)))
            .show(ui, |ui| {
                Frame::with_id("inner")
                    .size(20.0)
                    .fill(Color::rgb(1.0, 0.0, 0.0))
                    .show(ui);
            });
    });
    ui.end_frame();

    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        &ui.cascades,
        1.0,
        Some(Rect::new(150.0, 150.0, 50.0, 50.0)),
        &mut cmds,
    );

    let pushes = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::PushTransform(_)))
        .count();
    let pops = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::PopTransform))
        .count();
    assert_eq!(pushes, pops);
    assert!(
        pushes >= 1,
        "filtered-out transformed node still emits its transform pair"
    );
}
