use super::{RenderCmd, encode};
use crate::element::Element;
use crate::primitives::{Color, Rect, Sizing};
use crate::widgets::{Frame, HStack, ZStack};
use crate::{Ui, layout};

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
    encode(&ui.tree, &mut cmds);
    assert!(cmds.is_empty());
}

#[test]
fn frame_with_fill_emits_one_draw_rect() {
    let mut ui = Ui::new();
    ui.begin_frame();
    HStack::new().show(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(1.0, 0.0, 0.0))
            .show(ui);
    });
    let root = ui.root();
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 200.0));

    let mut cmds = Vec::new();
    encode(&ui.tree, &mut cmds);

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
    HStack::new().show(&mut ui, |ui| {
        Frame::with_id("invisible").size(50.0).show(ui);
    });
    let root = ui.root();
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 200.0));

    let mut cmds = Vec::new();
    encode(&ui.tree, &mut cmds);

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
    HStack::new().show(&mut ui, |ui| {
        ZStack::with_id("clip")
            .size(50.0)
            .clip(true)
            .show(ui, |ui| {
                Frame::with_id("inner")
                    .size(40.0)
                    .fill(Color::rgb(0.5, 0.5, 0.5))
                    .show(ui);
            });
    });
    let root = ui.root();
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 200.0));

    let mut cmds = Vec::new();
    encode(&ui.tree, &mut cmds);

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

#[test]
fn nested_clips_each_emit_their_own_pair() {
    let mut ui = Ui::new();
    ui.begin_frame();
    HStack::new().show(&mut ui, |ui| {
        ZStack::with_id("outer")
            .size(Sizing::Fixed(100.0))
            .clip(true)
            .show(ui, |ui| {
                ZStack::with_id("inner")
                    .size(Sizing::Fixed(50.0))
                    .clip(true)
                    .show(ui, |_| {});
            });
    });
    let root = ui.root();
    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 200.0, 200.0));

    let mut cmds = Vec::new();
    encode(&ui.tree, &mut cmds);
    let (pushes, pops) = count_clip_pairs(&cmds);
    assert_eq!(pushes, 2);
    assert_eq!(pops, 2);
}
