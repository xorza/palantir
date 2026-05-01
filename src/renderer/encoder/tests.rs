use super::{RenderCmd, encode};
use crate::Ui;
use crate::element::Element;
use crate::input::{InputEvent, PointerButton};
use crate::primitives::{Color, Rect, Sense, Sizing, TranslateScale};
use crate::widgets::{Frame, Panel, Styled};
use glam::Vec2;

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
    let ui = Ui::new();
    encode(&ui.tree, ui.layout_result(), &mut cmds);
    assert!(cmds.is_empty());
}

#[test]
fn frame_with_fill_emits_one_draw_rect() {
    let mut ui = Ui::new();
    ui.begin_frame();
    Panel::hstack().show(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(1.0, 0.0, 0.0))
            .show(ui);
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));

    let mut cmds = Vec::new();
    encode(&ui.tree, ui.layout_result(), &mut cmds);

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
    ui.begin_frame();
    Panel::hstack().show(&mut ui, |ui| {
        Frame::with_id("invisible").size(50.0).show(ui);
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));

    let mut cmds = Vec::new();
    encode(&ui.tree, ui.layout_result(), &mut cmds);

    let draw_rects = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::DrawRect { .. }))
        .count();
    assert_eq!(draw_rects, 0);
}

#[test]
fn clip_emits_balanced_push_pop() {
    let mut ui = Ui::new();
    ui.begin_frame();
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
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));

    let mut cmds = Vec::new();
    encode(&ui.tree, ui.layout_result(), &mut cmds);

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
    ui.begin_frame();
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
    ui.layout(Rect::new(0.0, 0.0, 400.0, 400.0));
    ui.end_frame();

    let mut cmds = Vec::new();
    encode(&ui.tree, ui.layout_result(), &mut cmds);
    let drawn = screen_rects_by_fill(&cmds);

    // Visible node: encoder emits exactly one DrawRect with its fill, and the
    // resulting screen rect matches the hit index's rect for the same id.
    let v_id = crate::primitives::WidgetId::from_hash("V");
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
    let d_id = crate::primitives::WidgetId::from_hash("D");
    let d_screen = drawn
        .iter()
        .find(|(c, _)| *c == d_color)
        .map(|(_, r)| *r)
        .expect("disabled node should still paint");
    let d_hit = ui.response_for(d_id).rect.expect("disabled node has rect");
    assert_eq!(d_screen, d_hit, "encoder vs hit-index rect for D");

    // Hidden node: encoder skips, hit index still tracks the slot.
    let h_id = crate::primitives::WidgetId::from_hash("H");
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
    ui.begin_frame();
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
    ui.begin_frame();
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
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));

    let mut cmds = Vec::new();
    encode(&ui.tree, ui.layout_result(), &mut cmds);
    let (pushes, pops) = count_clip_pairs(&cmds);
    assert_eq!(pushes, 2);
    assert_eq!(pops, 2);
}
