use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Color, Rect, Sense, Sizing};
use crate::shape::Shape;
use crate::widgets::{Button, Frame, Panel, Styled};

#[test]
fn clip_flag_is_recorded_on_panel_node() {
    // Default is `overflow: visible` — panels do not clip unless asked.
    // Explicit `.clip(true)` opts in. Pin both directions so a future
    // default change is loud.
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut default_panel = None;
    let mut opt_in = None;
    Panel::hstack().show(&mut ui, |ui| {
        default_panel = Some(
            Panel::zstack_with_id("default")
                .size(50.0)
                .show(ui, |_| {})
                .node,
        );
        opt_in = Some(
            Panel::zstack_with_id("opt-in")
                .size(50.0)
                .clip(true)
                .show(ui, |_| {})
                .node,
        );
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));

    assert!(!ui.tree.paint(default_panel.unwrap()).attrs.is_clip());
    assert!(ui.tree.paint(opt_in.unwrap()).attrs.is_clip());
}

#[test]
fn frame_paints_a_single_rounded_rect() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut frame_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        frame_node = Some(
            Frame::with_id("decoration")
                .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .radius(6.0)
                .show(ui)
                .node,
        );
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 200.0, 100.0));

    let shapes = ui.tree.shapes_of(frame_node.unwrap());
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], Shape::RoundedRect { .. }));

    // Default sense is None — frame is not a hit-test target.
    let r = ui.rect(frame_node.unwrap());
    assert_eq!(r.size.w, 80.0);
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn panel_hugs_largest_child_and_layers_them() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut panel_node = None;
    let mut a_node = None;
    let mut b_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        panel_node = Some(
            Panel::zstack_with_id("card")
                .padding(10.0)
                .fill(Color::rgb(0.1, 0.1, 0.15))
                .radius(8.0)
                .show(ui, |ui| {
                    a_node = Some(
                        Button::with_id("a")
                            .size((Sizing::Fixed(80.0), Sizing::Fixed(30.0)))
                            .show(ui)
                            .node,
                    );
                    b_node = Some(
                        Button::with_id("b")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(50.0)))
                            .show(ui)
                            .node,
                    );
                })
                .node,
        );
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 400.0, 200.0));

    // Panel hugs to (max(80, 60) + 2*10, max(30, 50) + 2*10) = (100, 70).
    let panel = ui.rect(panel_node.unwrap());
    assert_eq!(panel.size.w, 100.0);
    assert_eq!(panel.size.h, 70.0);

    // Both children laid out at panel's inner top-left (10, 10), at their own size.
    let a = ui.rect(a_node.unwrap());
    let b = ui.rect(b_node.unwrap());
    assert_eq!((a.min.x, a.min.y), (10.0, 10.0));
    assert_eq!((b.min.x, b.min.y), (10.0, 10.0));
    assert_eq!((a.size.w, a.size.h), (80.0, 30.0));
    assert_eq!((b.size.w, b.size.h), (60.0, 50.0));

    // Panel paints its bg shape; first shape on the panel node is the rect.
    let shapes = ui.tree.shapes_of(panel_node.unwrap());
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], Shape::RoundedRect { .. }));
}

#[test]
fn panel_with_fill_child_grows_to_panel_inner() {
    // Panel with Fixed size + Fill child: child fills panel's inner rect.
    // (Root is an HStack so the panel's Fixed size is honored — root would
    // otherwise expand to surface.)
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut child_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack_with_id("p")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(100.0)))
            .padding(10.0)
            .show(ui, |ui| {
                child_node = Some(
                    Frame::with_id("filler")
                        .size((Sizing::FILL, Sizing::FILL))
                        .fill(Color::rgb(0.5, 0.5, 0.5))
                        .show(ui)
                        .node,
                );
            });
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 400.0, 400.0));

    let child = ui.rect(child_node.unwrap());
    // Panel = 200×100; inner (after padding 10) = 180×80, child fills it at (10, 10).
    assert_eq!(child.min.x, 10.0);
    assert_eq!(child.min.y, 10.0);
    assert_eq!(child.size.w, 180.0);
    assert_eq!(child.size.h, 80.0);
}

#[test]
fn zstack_layers_children_without_painting_background() {
    // Like Panel but with no fill/stroke/radius — pure layered layout.
    // Wrapped in HStack so the ZStack's Hug-to-children size is honored
    // (root would otherwise expand to surface).
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut zstack_node = None;
    let mut bg_node = None;
    let mut fg_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        zstack_node = Some(
            Panel::zstack_with_id("layered")
                .show(ui, |ui| {
                    bg_node = Some(
                        Frame::with_id("bg")
                            .size((Sizing::Fixed(120.0), Sizing::Fixed(80.0)))
                            .fill(Color::rgb(0.1, 0.1, 0.2))
                            .show(ui)
                            .node,
                    );
                    fg_node = Some(
                        Button::with_id("fg")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(30.0)))
                            .show(ui)
                            .node,
                    );
                })
                .node,
        );
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 400.0, 200.0));

    let z = zstack_node.unwrap();
    // ZStack itself paints nothing.
    assert!(ui.tree.shapes_of(z).is_empty());

    // ZStack hugs to max(child sizes) = (120, 80).
    let zr = ui.rect(z);
    assert_eq!(zr.size.w, 120.0);
    assert_eq!(zr.size.h, 80.0);

    // Both children placed at ZStack's top-left (no padding), at their own size.
    let bg = ui.rect(bg_node.unwrap());
    let fg = ui.rect(fg_node.unwrap());
    assert_eq!((bg.min.x, bg.min.y), (0.0, 0.0));
    assert_eq!((fg.min.x, fg.min.y), (0.0, 0.0));
    assert_eq!((bg.size.w, bg.size.h), (120.0, 80.0));
    assert_eq!((fg.size.w, fg.size.h), (60.0, 30.0));
}

#[test]
fn disabled_panel_suppresses_clicks_on_descendants() {
    use crate::input::{InputEvent, PointerButton};
    use glam::Vec2;

    let mut ui = Ui::new();
    ui.begin_frame();
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack_with_id("locked")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
            .padding(20.0)
            .fill(Color::rgb(0.2, 0.2, 0.2))
            .disabled(true)
            .show(ui, |ui| {
                Button::with_id("inside")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                    .show(ui);
            });
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 400.0, 200.0));
    ui.end_frame();

    // Click on the button inside the disabled panel.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame();
    let mut clicked = false;
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack_with_id("locked")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
            .padding(20.0)
            .fill(Color::rgb(0.2, 0.2, 0.2))
            .disabled(true)
            .show(ui, |ui| {
                clicked = Button::with_id("inside")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .clicked();
            });
    });
    assert!(!clicked, "button inside disabled panel should not click");
}

#[test]
fn collapsed_child_consumes_no_space_in_hstack() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = Panel::hstack()
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::with_id("a").size(40.0).show(ui);
            Frame::with_id("gone").size(40.0).collapsed().show(ui);
            Frame::with_id("b").size(40.0).show(ui);
        })
        .node;
    ui.layout(Rect::new(0.0, 0.0, 400.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.rect(kids[0]);
    let gone = ui.rect(kids[1]);
    let b = ui.rect(kids[2]);

    assert_eq!(a.min.x, 0.0);
    assert_eq!(a.size.w, 40.0);
    assert_eq!(gone.size.w, 0.0);
    assert_eq!(gone.size.h, 0.0);
    // Only one gap between the two visible siblings: 40 + 10 = 50.
    assert_eq!(b.min.x, 50.0);
    assert_eq!(b.size.w, 40.0);
}

#[test]
fn collapsed_does_not_consume_fill_weight() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = Panel::hstack()
        .show(&mut ui, |ui| {
            Frame::with_id("a")
                .size((Sizing::Fill(1.0), Sizing::Hug))
                .show(ui);
            Frame::with_id("gone")
                .size((Sizing::Fill(3.0), Sizing::Hug))
                .collapsed()
                .show(ui);
            Frame::with_id("b")
                .size((Sizing::Fill(1.0), Sizing::Hug))
                .show(ui);
        })
        .node;
    ui.layout(Rect::new(0.0, 0.0, 400.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.rect(kids[0]);
    let b = ui.rect(kids[2]);
    // Collapsed sibling's weight (3.0) is dropped — remaining two fills split 50/50.
    assert_eq!(a.size.w, 200.0);
    assert_eq!(b.size.w, 200.0);
    assert_eq!(b.min.x, 200.0);
}

#[test]
fn hidden_keeps_slot_but_emits_no_draws() {
    use crate::renderer::{RenderCmd, encode};
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = Panel::hstack()
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::with_id("a")
                .size(40.0)
                .fill(Color::rgb(1.0, 0.0, 0.0))
                .show(ui);
            Frame::with_id("hid")
                .size(40.0)
                .fill(Color::rgb(0.0, 1.0, 0.0))
                .hidden()
                .show(ui);
            Frame::with_id("b")
                .size(40.0)
                .fill(Color::rgb(0.0, 0.0, 1.0))
                .show(ui);
        })
        .node;
    ui.layout(Rect::new(0.0, 0.0, 400.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let hid = ui.rect(kids[1]);
    let b = ui.rect(kids[2]);
    // Hidden node still occupies its slot.
    assert_eq!(hid.size.w, 40.0);
    // ...so b's offset includes hidden's width + both gaps.
    assert_eq!(b.min.x, 40.0 + 10.0 + 40.0 + 10.0);

    // ...but emits no DrawRect.
    ui.end_frame();
    let mut cmds = Vec::new();
    encode(&ui.tree, ui.layout_result(), ui.cascades(), 1.0, &mut cmds);
    let draws = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::DrawRect { .. }))
        .count();
    assert_eq!(draws, 2, "only the two Visible frames should paint");
}

#[test]
fn hidden_button_does_not_click() {
    use crate::input::{InputEvent, PointerButton};
    use glam::Vec2;

    let mut ui = Ui::new();
    ui.begin_frame();
    Panel::hstack().show(&mut ui, |ui| {
        Button::with_id("invisible")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .hidden()
            .show(ui);
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 400.0, 200.0));
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame();
    let mut clicked = false;
    Panel::hstack().show(&mut ui, |ui| {
        clicked = Button::with_id("invisible")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .hidden()
            .show(ui)
            .clicked();
    });
    assert!(!clicked, "hidden button should not receive clicks");
}

#[test]
fn hstack_child_align_y_centers_all_children_by_default() {
    use crate::primitives::{Align, VAlign};
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = Panel::hstack()
        .size((Sizing::FILL, Sizing::Fixed(100.0)))
        .child_align(Align::v(VAlign::Center))
        .show(&mut ui, |ui| {
            Frame::with_id("a")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .show(ui);
            Frame::with_id("b")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .show(ui);
        })
        .node;
    ui.layout(Rect::new(0.0, 0.0, 200.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.rect(kids[0]);
    let b = ui.rect(kids[1]);
    // Cross axis = 100, child = 20 tall → centered at (100-20)/2 = 40.
    assert_eq!(a.min.y, 40.0);
    assert_eq!(b.min.y, 40.0);
    assert_eq!(a.size.h, 20.0);
    assert_eq!(b.size.h, 20.0);
}

#[test]
fn child_align_self_overrides_parent_default() {
    use crate::primitives::{Align, VAlign};
    let mut ui = Ui::new();
    ui.begin_frame();
    let root = Panel::hstack()
        .size((Sizing::FILL, Sizing::Fixed(100.0)))
        .child_align(Align::v(VAlign::Center))
        .show(&mut ui, |ui| {
            Frame::with_id("centered")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .show(ui);
            // Explicit Bottom on the child wins over the parent's default.
            Frame::with_id("bottom")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .align(Align::v(VAlign::Bottom))
                .show(ui);
        })
        .node;
    ui.layout(Rect::new(0.0, 0.0, 200.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let centered = ui.rect(kids[0]);
    let bottom = ui.rect(kids[1]);
    assert_eq!(centered.min.y, 40.0);
    assert_eq!(bottom.min.y, 80.0);
}

#[test]
fn zstack_centers_child_when_align_center() {
    use crate::primitives::Align;
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut child_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack_with_id("box")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(100.0)))
            .show(ui, |ui| {
                child_node = Some(
                    Frame::with_id("c")
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                        .align(Align::CENTER)
                        .fill(Color::rgb(0.5, 0.5, 0.5))
                        .show(ui)
                        .node,
                );
            });
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 400.0, 400.0));

    let r = ui.rect(child_node.unwrap());
    // ZStack inner = 200×100, child = 40×20 → centered at (80, 40).
    assert_eq!((r.min.x, r.min.y), (80.0, 40.0));
    assert_eq!((r.size.w, r.size.h), (40.0, 20.0));
}

#[test]
fn zstack_aligns_independently_per_axis() {
    use crate::primitives::{Align, HAlign, VAlign};
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut child_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack_with_id("box")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(100.0)))
            .show(ui, |ui| {
                child_node = Some(
                    Frame::with_id("c")
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                        .align(Align::new(HAlign::Right, VAlign::Center))
                        .fill(Color::rgb(0.5, 0.5, 0.5))
                        .show(ui)
                        .node,
                );
            });
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 400.0, 400.0));

    let r = ui.rect(child_node.unwrap());
    // x: End → 200-40 = 160. y: Center → (100-20)/2 = 40.
    assert_eq!((r.min.x, r.min.y), (160.0, 40.0));
}

#[test]
fn canvas_places_children_at_absolute_positions_and_hugs_bbox() {
    use glam::Vec2;
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut canvas_node = None;
    let mut a_node = None;
    let mut b_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        canvas_node = Some(
            Panel::canvas_with_id("c")
                .show(ui, |ui| {
                    a_node = Some(
                        Frame::with_id("a")
                            .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                            .position(Vec2::new(10.0, 5.0))
                            .show(ui)
                            .node,
                    );
                    b_node = Some(
                        Frame::with_id("b")
                            .size((Sizing::Fixed(30.0), Sizing::Fixed(60.0)))
                            .position(Vec2::new(80.0, 40.0))
                            .show(ui)
                            .node,
                    );
                })
                .node,
        );
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 400.0, 400.0));

    let c = ui.rect(canvas_node.unwrap());
    // Hugs bbox: max(10+40, 80+30)=110, max(5+20, 40+60)=100.
    assert_eq!(c.size.w, 110.0);
    assert_eq!(c.size.h, 100.0);

    let a = ui.rect(a_node.unwrap());
    let b = ui.rect(b_node.unwrap());
    assert_eq!((a.min.x, a.min.y), (10.0, 5.0));
    assert_eq!((a.size.w, a.size.h), (40.0, 20.0));
    assert_eq!((b.min.x, b.min.y), (80.0, 40.0));
    assert_eq!((b.size.w, b.size.h), (30.0, 60.0));
}

#[test]
fn frame_with_sense_click_is_clickable() {
    use crate::input::{InputEvent, PointerButton};
    use glam::Vec2;

    let mut ui = Ui::new();
    ui.begin_frame();
    Panel::hstack().show(&mut ui, |ui| {
        Frame::with_id("hitbox")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .sense(Sense::CLICK)
            .show(ui);
    });
    let _root = ui.root();
    ui.layout(Rect::new(0.0, 0.0, 200.0, 100.0));
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 25.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame();
    let mut clicked = false;
    Panel::hstack().show(&mut ui, |ui| {
        clicked = Frame::with_id("hitbox")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .sense(Sense::CLICK)
            .show(ui)
            .clicked();
    });
    assert!(clicked);
}

#[test]
fn wrapping_text_grows_height_in_narrow_frame() {
    use crate::shape::TextWrap;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::Text;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame();
    let mut text_node = None;
    Panel::vstack()
        .size((Sizing::Fixed(60.0), Sizing::Hug))
        .show(&mut ui, |ui| {
            text_node = Some(
                Text::new("the quick brown fox jumps over the lazy dog")
                    .size_px(16.0)
                    .wrapping()
                    .show(ui)
                    .node,
            );
        });
    ui.layout(Rect::new(0.0, 0.0, 400.0, 400.0));
    ui.end_frame();

    let node = text_node.unwrap();
    let r = ui.rect(node);
    assert!(
        r.size.h > 32.0,
        "wrapped paragraph should span multiple lines, got h={}",
        r.size.h,
    );
    let shape = ui.tree.shapes_of(node).first().expect("text shape");
    let wrap = match shape {
        Shape::Text { wrap, .. } => *wrap,
        _ => panic!("expected Shape::Text"),
    };
    assert_eq!(wrap, TextWrap::Wrap);
    let shaped = ui
        .layout_result()
        .text_shape(node)
        .expect("layout should have shaped the text");
    assert!(shaped.measured.h > 32.0);
}

#[test]
fn wrapping_text_overflows_intrinsic_min_without_breaking_words() {
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::Text;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame();
    let mut text_node = None;
    Panel::vstack()
        .size((Sizing::Fixed(8.0), Sizing::Hug))
        .show(&mut ui, |ui| {
            text_node = Some(
                Text::new("supercalifragilisticexpialidocious")
                    .size_px(16.0)
                    .wrapping()
                    .show(ui)
                    .node,
            );
        });
    ui.layout(Rect::new(0.0, 0.0, 400.0, 400.0));
    ui.end_frame();

    let r = ui.rect(text_node.unwrap());
    // The single word can't break — its width must overflow the 8 px slot.
    assert!(
        r.size.w > 8.0,
        "an unbreakable word must overflow the slot, got w={}",
        r.size.w,
    );
}
