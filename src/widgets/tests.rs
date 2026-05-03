use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Color, Display, Sense, Sizing};
use crate::renderer::RenderCmdBuffer;
use crate::shape::Shape;
use crate::widgets::{Button, Frame, Panel, Styled};
use glam::UVec2;

#[test]
fn clip_flag_is_recorded_on_panel_node() {
    // Default is `overflow: visible` — panels do not clip unless asked.
    // Explicit `.clip(true)` opts in. Pin both directions so a future
    // default change is loud.
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 200.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    assert!(!ui.tree.paint(default_panel.unwrap()).attrs.is_clip());
    assert!(ui.tree.paint(opt_in.unwrap()).attrs.is_clip());
}

#[test]
fn frame_paints_a_single_rounded_rect() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 100.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let shapes = ui.tree.shapes_of(frame_node.unwrap());
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], Shape::RoundedRect { .. }));

    // Default sense is None — frame is not a hit-test target.
    let r = ui.layout_engine.rect(frame_node.unwrap());
    assert_eq!(r.size.w, 80.0);
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn panel_hugs_largest_child_and_layers_them() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 200.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    // Panel hugs to (max(80, 60) + 2*10, max(30, 50) + 2*10) = (100, 70).
    let panel = ui.layout_engine.rect(panel_node.unwrap());
    assert_eq!(panel.size.w, 100.0);
    assert_eq!(panel.size.h, 70.0);

    // Both children laid out at panel's inner top-left (10, 10), at their own size.
    let a = ui.layout_engine.rect(a_node.unwrap());
    let b = ui.layout_engine.rect(b_node.unwrap());
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
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 400.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let child = ui.layout_engine.rect(child_node.unwrap());
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
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 200.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let z = zstack_node.unwrap();
    // ZStack itself paints nothing.
    assert!(ui.tree.shapes_of(z).is_empty());

    // ZStack hugs to max(child sizes) = (120, 80).
    let zr = ui.layout_engine.rect(z);
    assert_eq!(zr.size.w, 120.0);
    assert_eq!(zr.size.h, 80.0);

    // Both children placed at ZStack's top-left (no padding), at their own size.
    let bg = ui.layout_engine.rect(bg_node.unwrap());
    let fg = ui.layout_engine.rect(fg_node.unwrap());
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
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 200.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    // Click on the button inside the disabled panel.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame(Display::default());
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
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 100.0 as u32),
        1.0,
    ));
    let root = Panel::hstack()
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::with_id("a").size(40.0).show(ui);
            Frame::with_id("gone").size(40.0).collapsed().show(ui);
            Frame::with_id("b").size(40.0).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.layout_engine.rect(kids[0]);
    let gone = ui.layout_engine.rect(kids[1]);
    let b = ui.layout_engine.rect(kids[2]);

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
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 100.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.layout_engine.rect(kids[0]);
    let b = ui.layout_engine.rect(kids[2]);
    // Collapsed sibling's weight (3.0) is dropped — remaining two fills split 50/50.
    assert_eq!(a.size.w, 200.0);
    assert_eq!(b.size.w, 200.0);
    assert_eq!(b.min.x, 200.0);
}

#[test]
fn hidden_keeps_slot_but_emits_no_draws() {
    use crate::renderer::{RenderCmd, encode};
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 100.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let hid = ui.layout_engine.rect(kids[1]);
    let b = ui.layout_engine.rect(kids[2]);
    // Hidden node still occupies its slot.
    assert_eq!(hid.size.w, 40.0);
    // ...so b's offset includes hidden's width + both gaps.
    assert_eq!(b.min.x, 40.0 + 10.0 + 40.0 + 10.0);

    // ...but emits no DrawRect.
    let mut cmds = RenderCmdBuffer::new();
    encode(
        &ui.tree,
        ui.layout_engine.result(),
        ui.cascades.result(),
        None,
        &mut cmds,
    );
    let draws = cmds
        .iter()
        .filter(|c| matches!(c, RenderCmd::DrawRect(_) | RenderCmd::DrawRectStroked(_)))
        .count();
    assert_eq!(draws, 2, "only the two Visible frames should paint");
}

#[test]
fn hidden_button_does_not_click() {
    use crate::input::{InputEvent, PointerButton};
    use glam::Vec2;

    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 200.0 as u32),
        1.0,
    ));
    Panel::hstack().show(&mut ui, |ui| {
        Button::with_id("invisible")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .hidden()
            .show(ui);
    });
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame(Display::default());
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
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 100.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.layout_engine.rect(kids[0]);
    let b = ui.layout_engine.rect(kids[1]);
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
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 100.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let centered = ui.layout_engine.rect(kids[0]);
    let bottom = ui.layout_engine.rect(kids[1]);
    assert_eq!(centered.min.y, 40.0);
    assert_eq!(bottom.min.y, 80.0);
}

#[test]
fn zstack_centers_child_when_align_center() {
    use crate::primitives::Align;
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 400.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let r = ui.layout_engine.rect(child_node.unwrap());
    // ZStack inner = 200×100, child = 40×20 → centered at (80, 40).
    assert_eq!((r.min.x, r.min.y), (80.0, 40.0));
    assert_eq!((r.size.w, r.size.h), (40.0, 20.0));
}

#[test]
fn zstack_aligns_independently_per_axis() {
    use crate::primitives::{Align, HAlign, VAlign};
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 400.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let r = ui.layout_engine.rect(child_node.unwrap());
    // x: End → 200-40 = 160. y: Center → (100-20)/2 = 40.
    assert_eq!((r.min.x, r.min.y), (160.0, 40.0));
}

#[test]
fn canvas_places_children_at_absolute_positions_and_hugs_bbox() {
    use glam::Vec2;
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 400.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let c = ui.layout_engine.rect(canvas_node.unwrap());
    // Hugs bbox: max(10+40, 80+30)=110, max(5+20, 40+60)=100.
    assert_eq!(c.size.w, 110.0);
    assert_eq!(c.size.h, 100.0);

    let a = ui.layout_engine.rect(a_node.unwrap());
    let b = ui.layout_engine.rect(b_node.unwrap());
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
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 100.0 as u32),
        1.0,
    ));
    Panel::hstack().show(&mut ui, |ui| {
        Frame::with_id("hitbox")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .sense(Sense::CLICK)
            .show(ui);
    });
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 25.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    ui.begin_frame(Display::default());
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
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 400.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let node = text_node.unwrap();
    let r = ui.layout_engine.rect(node);
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
        .layout_engine
        .result()
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
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 400.0 as u32),
        1.0,
    ));
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
    ui.end_frame();

    let r = ui.layout_engine.rect(text_node.unwrap());
    // The single word can't break — its width must overflow the 8 px slot.
    assert!(
        r.size.w > 8.0,
        "an unbreakable word must overflow the slot, got w={}",
        r.size.w,
    );
}

/// Pins Option A's known gap: a wrapping `Text` inside a `Grid` `Auto`
/// column gets `available_w = INFINITY` during measure (the WPF trick for
/// unresolved tracks), so it never reshapes and the column ends up at the
/// Pinned by `src/layout/intrinsic.md`: a wrapping `Text` inside a
/// `Grid` `Hug` column constrained by the parent's available width
/// reshapes to fit. The grid column-resolution algorithm runs during measure with
/// the grid's `inner_avail` (200 px here); the wrapping text gets its
/// committed column width before shaping, so the cached shape is
/// multi-line and fits the slot.
#[test]
fn wrapping_text_in_grid_auto_column_wraps_under_constrained_width() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 400.0 as u32),
        1.0,
    ));
    let mut text_node = None;
    Grid::new()
        .cols(Rc::from([Track::hug(), Track::hug()]))
        .rows(Rc::from([Track::hug()]))
        .show(&mut ui, |ui| {
            text_node = Some(
                Text::new("the quick brown fox jumps over the lazy dog")
                    .size_px(16.0)
                    .wrapping()
                    .grid_cell((0, 0))
                    .show(ui)
                    .node,
            );
            Text::new("right column")
                .size_px(16.0)
                .grid_cell((0, 1))
                .show(ui);
        });
    // Surface is 200 px wide — narrower than the text's natural unbroken
    // width (~335 px). Step B's column resolution shrinks the Hug column
    // to fit so the text wraps cleanly inside.
    ui.end_frame();

    let node = text_node.unwrap();
    let shaped = ui
        .layout_engine
        .result()
        .text_shape(node)
        .expect("text was shaped");
    // Multi-line height (a 16 px font wraps to 3 lines at the resolved
    // column width — h ≈ 58 px in practice; assert > 32 to allow for
    // line-height variation).
    assert!(
        shaped.measured.h > 32.0,
        "expected multi-line wrapped height after Step B, got h={}",
        shaped.measured.h,
    );
    // Text fits inside the 200 px surface (column took its share, paragraph
    // wrapped to fit).
    assert!(
        shaped.measured.w <= 200.0,
        "expected text width within the 200 px surface, got w={}",
        shaped.measured.w,
    );
}

/// Step A acceptance: `Ui::intrinsic` returns sane values for a
/// wrapping text leaf inside a Grid `Auto` cell. Pure infrastructure
/// test — nothing in the production layout path consumes intrinsics
/// yet; this just confirms the API + cache + per-driver functions are
/// wired correctly. Steps B/C will flip the `does_not_wrap_today`
/// assertions above by *consuming* what we measure here.
#[test]
fn intrinsic_query_on_wrapping_text_leaf_returns_sensible_values() {
    use crate::layout::{Axis, LenReq};
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 400.0 as u32),
        1.0,
    ));
    let mut text_node = None;
    Grid::new()
        .cols(Rc::from([Track::hug(), Track::hug()]))
        .rows(Rc::from([Track::hug()]))
        .show(&mut ui, |ui| {
            text_node = Some(
                Text::new("the quick brown fox jumps over the lazy dog")
                    .size_px(16.0)
                    .wrapping()
                    .grid_cell((0, 0))
                    .show(ui)
                    .node,
            );
            Text::new("right column")
                .size_px(16.0)
                .grid_cell((0, 1))
                .show(ui);
        });
    ui.end_frame();

    let node = text_node.unwrap();
    // Direct engine query (no Ui wrapper) — drivers do the same in
    // production code paths. Disjoint field borrows on `ui` let this
    // compile without an accessor method.
    let max_w =
        ui.layout_engine
            .intrinsic(&ui.tree, node, Axis::X, LenReq::MaxContent, &mut ui.text);
    let min_w =
        ui.layout_engine
            .intrinsic(&ui.tree, node, Axis::X, LenReq::MinContent, &mut ui.text);
    let max_h =
        ui.layout_engine
            .intrinsic(&ui.tree, node, Axis::Y, LenReq::MaxContent, &mut ui.text);

    // Natural unbroken width is well over 200px (the BUG-card pin
    // observed ~335 px); any value clearly above 200 confirms cosmic
    // returned a real shape.
    assert!(
        max_w > 200.0,
        "max_w should be the natural unbroken width, got {max_w}"
    );
    // Min-content is the longest unbreakable run — for this text "jumps"
    // is one of the longer words (~5 chars × ~10 px ≈ 50 px). Any value
    // between ~30 and ~100 is plausible; assert it's clearly less than
    // the natural width.
    assert!(
        min_w > 0.0 && min_w < max_w,
        "min_w should be positive and < max_w, got {min_w}"
    );
    assert!(
        min_w < 100.0,
        "min_w should be a single-word width, got {min_w}"
    );
    // Single-line height around the 16 px font's line height (~20 px).
    assert!(
        max_h > 0.0 && max_h < 30.0,
        "max_h should be single-line height, got {max_h}"
    );
}

/// Regression: a constrained ZStack (`Sizing::Fill`/`Fixed`) must pass
/// its inner size to children, not `INFINITY`. Without this, Step B's
/// Grid Auto resolution falls back to max-content for any grid nested
/// inside a ZStack — which is exactly the showcase pattern. Pin so the
/// `INF`-as-default doesn't sneak back.
#[test]
fn fill_zstack_passes_finite_avail_so_nested_grid_constrains() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 400.0 as u32),
        1.0,
    ));
    let mut text_node = None;
    Panel::zstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Grid::new()
                .cols(Rc::from([Track::hug(), Track::hug()]))
                .rows(Rc::from([Track::hug()]))
                .show(ui, |ui| {
                    text_node = Some(
                        Text::new("the quick brown fox jumps over the lazy dog")
                            .size_px(16.0)
                            .wrapping()
                            .grid_cell((0, 0))
                            .show(ui)
                            .node,
                    );
                    Text::new("right column")
                        .size_px(16.0)
                        .grid_cell((0, 1))
                        .show(ui);
                });
        });
    ui.end_frame();

    let shaped = ui
        .layout_engine
        .result()
        .text_shape(text_node.unwrap())
        .expect("text was shaped");
    assert!(
        shaped.measured.h > 32.0,
        "ZStack must propagate finite avail to grid → grid constrains hug column → text wraps; got h={}",
        shaped.measured.h,
    );
    assert!(
        shaped.measured.w <= 200.0,
        "wrapped text must fit inside surface; got w={}",
        shaped.measured.w,
    );
}

/// Regression: same as above but for Canvas — also a "child-positioner"
/// layout that historically passed `INFINITY` regardless of its own size.
#[test]
fn fill_canvas_passes_finite_avail_so_nested_grid_constrains() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 400.0 as u32),
        1.0,
    ));
    let mut text_node = None;
    Panel::canvas()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Grid::new()
                .cols(Rc::from([Track::hug(), Track::hug()]))
                .rows(Rc::from([Track::hug()]))
                .show(ui, |ui| {
                    text_node = Some(
                        Text::new("the quick brown fox jumps over the lazy dog")
                            .size_px(16.0)
                            .wrapping()
                            .grid_cell((0, 0))
                            .show(ui)
                            .node,
                    );
                    Text::new("right column")
                        .size_px(16.0)
                        .grid_cell((0, 1))
                        .show(ui);
                });
        });
    ui.end_frame();

    let shaped = ui
        .layout_engine
        .result()
        .text_shape(text_node.unwrap())
        .expect("text was shaped");
    assert!(
        shaped.measured.h > 32.0,
        "Canvas must propagate finite avail; got h={}",
        shaped.measured.h,
    );
    assert!(
        shaped.measured.w <= 200.0,
        "wrapped text must fit inside surface; got w={}",
        shaped.measured.w,
    );
}

/// Pin: a `Hug` ZStack containing a `Fill` child must NOT recursively
/// size to its child. The per-axis fix above keeps the original
/// `INFINITY` behavior on Hug axes precisely to avoid this. If someone
/// "simplifies" the per-axis logic by always passing `inner`, this test
/// catches it.
#[test]
fn hug_zstack_does_not_recursively_size_to_fill_child() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(
        UVec2::new(800.0 as u32, 600.0 as u32),
        1.0,
    ));
    let mut zstack_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        zstack_node = Some(
            Panel::zstack_with_id("hug-z")
                // Default Sizing is Hug × Hug.
                .show(ui, |ui| {
                    // A Fill child inside Hug ZStack: must not blow ZStack up.
                    Frame::with_id("fill-child")
                        .size((Sizing::FILL, Sizing::FILL))
                        .fill(Color::rgb(0.5, 0.5, 0.5))
                        .show(ui);
                    // A real Fixed-size child to give the ZStack content size.
                    Frame::with_id("fixed-child")
                        .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                        .show(ui);
                })
                .node,
        );
    });
    ui.end_frame();

    // Hug ZStack should hug the Fixed child (60 × 40). If the per-axis
    // logic broke, ZStack would stretch to surface size.
    let r = ui.layout_engine.rect(zstack_node.unwrap());
    assert_eq!(r.size.w, 60.0);
    assert_eq!(r.size.h, 40.0);
}

/// Pin: a `Hug` grid with a `Fill` column has the Fill column collapse
/// to 0 at arrange (no leftover available). Step B handles this by
/// leaving Fill cols unresolved during measure → cells in Fill cols get
/// `INFINITY` available width → text shapes at natural (single line),
/// so row heights don't grow weirdly when the window resizes
/// horizontally. The cell rect itself is invisible (slot.w = 0), but the
/// row height stays consistent regardless of available width.
#[test]
fn hug_grid_fill_col_does_not_grow_row_height_on_horizontal_resize() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    fn measure(surface_w: f32) -> f32 {
        let mut ui = Ui::new();
        ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
        ui.begin_frame(Display::from_physical(
            UVec2::new((surface_w) as u32, 400.0 as u32),
            1.0,
        ));
        let mut value_node = None;
        Grid::new()
            // Hug × Hug grid (default sizing) with [Hug, Fill] columns.
            .cols(Rc::from([Track::hug(), Track::fill()]))
            .rows(Rc::from([Track::hug()]))
            .show(&mut ui, |ui| {
                Text::new("Label:").size_px(14.0).grid_cell((0, 0)).show(ui);
                value_node = Some(
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .size_px(14.0)
                        .wrapping()
                        .grid_cell((0, 1))
                        .show(ui)
                        .node,
                );
            });
        ui.end_frame();
        ui.layout_engine
            .result()
            .text_shape(value_node.unwrap())
            .expect("text was shaped")
            .measured
            .h
    }

    let h_wide = measure(2000.0);
    let h_narrow = measure(200.0);
    // Both should be single-line (≈18 px line height for 14 px Inter).
    // The exact value isn't pinned — what matters is wide and narrow
    // produce the same height (no width-driven wrapping during measure).
    assert!(
        h_wide < 24.0,
        "wide-window value should be single-line in Hug grid, got h={h_wide}"
    );
    assert!(
        h_narrow < 24.0,
        "narrow-window value should also be single-line (Fill col gets INF avail in Hug grid), got h={h_narrow}"
    );
    assert!(
        (h_wide - h_narrow).abs() < 0.5,
        "row height must not change with horizontal resize in Hug grid + Fill col; \
         wide={h_wide}, narrow={h_narrow}",
    );
}

/// Pin: a `Fill` grid with a `Fill` column DOES wrap text in the Fill
/// column — measure and arrange agree on the Fill col width (both equal
/// inner_avail's leftover after Hug + Fixed). This is the property-grid
/// pattern: `Sizing::FILL × Hug` grid + Hug label column + Fill value
/// column with wrapping text.
#[test]
fn fill_grid_fill_col_wraps_text_under_constrained_width() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 400.0 as u32),
        1.0,
    ));
    let mut value_node = None;
    // Wrap the grid in a vstack so the FILL grid gets a finite cross-axis
    // width to fill (vstack passes inner.cross to children).
    Panel::vstack().show(&mut ui, |ui| {
        Grid::new()
            .size((Sizing::FILL, Sizing::Hug))
            .cols(Rc::from([Track::hug(), Track::fill()]))
            .rows(Rc::from([Track::hug()]))
            .show(ui, |ui| {
                Text::new("Label:").size_px(14.0).grid_cell((0, 0)).show(ui);
                value_node = Some(
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .size_px(14.0)
                        .wrapping()
                        .grid_cell((0, 1))
                        .show(ui)
                        .node,
                );
            });
    });
    // Surface 200 wide → Fill col gets ~150 (after Hug label col).
    // Natural unbroken width is ~290 → must wrap.
    ui.end_frame();

    let shaped = ui
        .layout_engine
        .result()
        .text_shape(value_node.unwrap())
        .expect("text was shaped");
    assert!(
        shaped.measured.h > 32.0,
        "Fill grid + Fill col should wrap text under constrained width; got h={}",
        shaped.measured.h,
    );
    assert!(
        shaped.measured.w <= 200.0,
        "wrapped text width should fit inside surface; got w={}",
        shaped.measured.w,
    );
}

/// Step C pin: chat-message HStack pattern. Avatar (Fixed) + Message
/// (Fill, wrapping text). Without Step C, message is measured at INF →
/// shapes at natural width → cached shape disagrees with arrange's slot.
/// With Step C, message is re-measured at its resolved Fill share →
/// shapes (and reshapes if narrower than natural) at the slot width.
#[test]
fn hstack_fill_wrap_text_reshapes_at_resolved_share() {
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::Text;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 400.0 as u32),
        1.0,
    ));
    let mut message_node = None;
    // Wrap in a vstack so the Fill HStack gets a finite cross-axis to
    // constrain — and so the HStack's own Fill on main has parent's
    // available to resolve against.
    Panel::vstack().show(&mut ui, |ui| {
        Panel::hstack()
            .size((Sizing::FILL, Sizing::Hug))
            .gap(8.0)
            .show(ui, |ui| {
                Frame::with_id("avatar")
                    .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                    .show(ui);
                message_node = Some(
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .size_px(14.0)
                        .size((Sizing::FILL, Sizing::Hug))
                        .wrapping()
                        .show(ui)
                        .node,
                );
            });
    });
    // Surface 200 wide → message gets ~152 (after avatar + gap).
    // Natural unbroken width is ~290, so message must wrap.
    ui.end_frame();

    let shaped = ui
        .layout_engine
        .result()
        .text_shape(message_node.unwrap())
        .expect("text was shaped");
    assert!(
        shaped.measured.h > 32.0,
        "Fill message should wrap inside its resolved share; got h={}",
        shaped.measured.h,
    );
    // Shape width must be <= the resolved Fill share (200 - avatar - gap = 152).
    assert!(
        shaped.measured.w <= 160.0,
        "wrapped message width should fit within Fill share; got w={}",
        shaped.measured.w,
    );
}

/// Pin: HStack `Fill` child respects `intrinsic_min` floor — when the
/// resolved share is smaller than the longest unbreakable word, the
/// child stays at min-content (overflows) rather than shrinking
/// further. Same rule the leaf reshape branch in `shape_text` follows.
#[test]
fn hstack_fill_wrap_text_floors_at_min_content() {
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::Text;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 400.0 as u32),
        1.0,
    ));
    let mut message_node = None;
    Panel::vstack().show(&mut ui, |ui| {
        Panel::hstack()
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                // Hug Fixed sibling ~consumes 180 of 200 surface, leaving
                // 20 px for Fill — well below the longest word's width.
                Frame::with_id("avatar")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui);
                message_node = Some(
                    Text::new("supercalifragilistic")
                        .size_px(14.0)
                        .size((Sizing::FILL, Sizing::Hug))
                        .wrapping()
                        .show(ui)
                        .node,
                );
            });
    });
    ui.end_frame();

    let shaped = ui
        .layout_engine
        .result()
        .text_shape(message_node.unwrap())
        .expect("text was shaped");
    // The unbreakable word can't shrink below its own width (~110 px in
    // 14 px Inter). Even though the Fill share is ~20 px, message stays
    // at the longer floor and overflows.
    assert!(
        shaped.measured.w > 20.0,
        "min-content floor should keep message wider than the cramped slot; got w={}",
        shaped.measured.w,
    );
}

/// Regression: a VStack section containing a `(Fill, Hug)` Grid with a
/// Hug+Fill column layout and wrapping text in the Fill col must size
/// to the *wrapped* row heights, not the single-line intrinsic.
///
/// Stack pass-1 measures non-Fill children (here, the Hug-on-main grid)
/// at `INF` main + finite cross — that's height-given-width via
/// measure. If pass-1 instead used `intrinsic(MaxContent)` on main, the
/// grid's intrinsic Y collapses to single-line × n_rows (cells'
/// unbounded shape), the stack commits a too-small main, and grid's
/// `resolve_axis` row pass falls into the "cramped" branch — rows
/// collapse to zero. This test pins the showcase property-grid card.
#[test]
fn vstack_section_with_hug_grid_and_fill_col_wrap_does_not_collapse() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 600.0 as u32),
        1.0,
    ));
    let mut grid_node = None;
    Panel::vstack()
        .size((Sizing::FILL, Sizing::Hug))
        .show(&mut ui, |ui| {
            grid_node = Some(
                Grid::with_id("pg")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug(), Track::hug()]))
                    .show(ui, |ui| {
                        Text::new("Title:").size_px(14.0).grid_cell((0, 0)).show(ui);
                        Text::new(
                            "the quick brown fox jumps over the lazy dog \
                             pack my box with five dozen liquor jugs how \
                             vexingly quick daft zebras jump",
                        )
                        .size_px(14.0)
                        .wrapping()
                        .grid_cell((0, 1))
                        .show(ui);
                        Text::new("Tags:").size_px(14.0).grid_cell((1, 0)).show(ui);
                        Text::new("layout, grid, intrinsic, wrapping, css")
                            .size_px(14.0)
                            .wrapping()
                            .grid_cell((1, 1))
                            .show(ui);
                    })
                    .node,
            );
        });
    ui.end_frame();

    // Two rows of 14 px text. Single-line height ≈18 px → 36 px total
    // would mean both rows collapsed to single-line (no wrapping
    // accounted for). Wrapped paragraph in the value col must push
    // row 0 to multiple lines.
    let h = ui.layout_engine.result().rect(grid_node.unwrap()).size.h;
    assert!(
        h > 50.0,
        "grid must size to wrapped row heights, not single-line × 2; got h={h}"
    );
}

/// Regression: a Hug-axis ZStack containing a Hug Grid with wrapping
/// cells in a Fill col must let the grid measure under the constrained
/// cross axis. ZStack passes `INF` on Hug axes via
/// `child_avail_per_axis_hug`; replacing that `INF` with
/// `intrinsic(MaxContent)` would collapse the grid's wrapped Y the same
/// way as the stack regression above.
#[test]
fn hug_zstack_with_nested_grid_wrap_does_not_collapse() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(400.0 as u32, 600.0 as u32),
        1.0,
    ));
    let mut grid_node = None;
    Panel::vstack()
        .size((Sizing::Fixed(400.0), Sizing::Hug))
        .show(&mut ui, |ui| {
            Panel::zstack_with_id("hug-z")
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    grid_node = Some(
                        Grid::with_id("nested-grid")
                            .size((Sizing::FILL, Sizing::Hug))
                            .cols(Rc::from([Track::hug(), Track::fill()]))
                            .rows(Rc::from([Track::hug()]))
                            .show(ui, |ui| {
                                Text::new("Label:").size_px(14.0).grid_cell((0, 0)).show(ui);
                                Text::new(
                                    "the quick brown fox jumps over the lazy dog \
                                     pack my box with five dozen liquor jugs",
                                )
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui);
                            })
                            .node,
                    );
                });
        });
    ui.end_frame();

    let h = ui.layout_engine.result().rect(grid_node.unwrap()).size.h;
    assert!(
        h > 30.0,
        "ZStack must pass `INF` on Hug axes so nested grid measures \
         under the constrained cross and wraps; got h={h}"
    );
}

/// Pin: when a Stack's Fill child clamps to its `MinContent` floor
/// during pass-2 measure (the resolved share is narrower than the
/// longest unbreakable run), `arrange` recomputes leftover from
/// non-Fill `desired` and places the Fill child at `leftover * weight
/// / total_weight`. That arranged size can be **less** than the
/// child's measured size — measure floored at MinContent, arrange did
/// not. The text shape is wide (overflows), the rect is narrow.
///
/// Today's behavior — pinning so a future "tighten arrange to honor
/// measured size on Fill clamp" change is loud rather than silent.
/// Linked from `docs/layout-review.md` "Smaller but worth flagging".
#[test]
fn hstack_fill_clamped_to_min_content_arranges_at_leftover_share() {
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::Text;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(
        UVec2::new(200.0 as u32, 400.0 as u32),
        1.0,
    ));
    let mut message_node = None;
    Panel::vstack().show(&mut ui, |ui| {
        Panel::hstack()
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                // Fixed sibling ~consumes 180 of 200 surface, leaving
                // ~20 px for Fill — well below the longest word's width.
                Frame::with_id("avatar")
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui);
                message_node = Some(
                    Text::new("supercalifragilistic")
                        .size_px(14.0)
                        .size((Sizing::FILL, Sizing::Hug))
                        .wrapping()
                        .show(ui)
                        .node,
                );
            });
    });
    ui.end_frame();

    let shaped_w = ui
        .layout_engine
        .result()
        .text_shape(message_node.unwrap())
        .expect("text was shaped")
        .measured
        .w;
    let rect_w = ui.layout_engine.result().rect(message_node.unwrap()).size.w;

    // Pass-2 measure clamped to MinContent floor: shape ≈ longest-word
    // width (well above the 20 px Fill share).
    assert!(
        shaped_w > 50.0,
        "measure must floor at MinContent; got shaped_w={shaped_w}"
    );
    // Arrange recomputes leftover from non-Fill desired only and
    // places Fill at `leftover * weight / total_weight` ≈ 20 px,
    // ignoring the measured floor.
    assert!(
        rect_w < shaped_w,
        "arrange leftover should fall below measured floor; \
         shaped_w={shaped_w} rect_w={rect_w}"
    );
    assert!(
        rect_w < 30.0,
        "rect should reflect ~20 px leftover share, not the floor; \
         got rect_w={rect_w}"
    );
}

/// Showcase regression: the "two Hug columns" text-layouts section
/// rendered the right-column label on top of the wrapping paragraph.
/// Pin that the two cells' arranged rects don't horizontally overlap
/// — the right cell's `min.x` must sit at or past the left cell's
/// `max.x`.
#[test]
fn two_hug_columns_with_wrapping_text_do_not_overlap() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    let mut left = None;
    let mut right = None;
    Panel::vstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Grid::new()
                .cols(Rc::from([Track::hug(), Track::hug()]))
                .rows(Rc::from([Track::hug()]))
                .show(ui, |ui| {
                    left = Some(
                        Text::new(
                            "The quick brown fox jumps over the lazy dog. Pack my box \
                             with five dozen liquor jugs. How vexingly quick daft zebras jump!",
                        )
                        .size_px(14.0)
                        .wrapping()
                        .grid_cell((0, 0))
                        .show(ui)
                        .node,
                    );
                    right = Some(
                        Text::new("right column")
                            .size_px(14.0)
                            .grid_cell((0, 1))
                            .show(ui)
                            .node,
                    );
                });
        });
    ui.end_frame();

    let layout = ui.layout_engine.result();
    let lr = layout.rect(left.unwrap());
    let rr = layout.rect(right.unwrap());
    assert!(lr.size.w > 0.0, "left column must have a positive width");
    assert!(
        rr.min.x >= lr.max().x - 0.5,
        "right column must start at or past the left column's right edge: \
         left={lr:?}, right={rr:?}",
    );
}

/// Showcase regression — full repro of the broken "text layouts"
/// page: a vstack of sections (each a vstack with a title + body),
/// containing back-to-back grids with wrapping text. Reproduces the
/// case where one section's measure leaks across to another and
/// places cells overlapping. Surface matches the visible bug width.
#[test]
fn text_layouts_two_sections_back_to_back_no_overlap() {
    use crate::primitives::{Stroke, Track};
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
        Pack my box with five dozen liquor jugs. \
        How vexingly quick daft zebras jump!";

    let section = |ui: &mut Ui, id: &'static str, body: &mut dyn FnMut(&mut Ui)| {
        Panel::vstack_with_id(id)
            .size((Sizing::FILL, Sizing::Hug))
            .gap(6.0)
            .padding(8.0)
            .fill(Color::rgb(0.16, 0.18, 0.22))
            .stroke(Stroke {
                width: 1.0,
                color: Color::rgb(0.30, 0.34, 0.42),
            })
            .radius(4.0)
            .show(ui, |ui| {
                Text::with_id(("section-title", id), "title")
                    .size_px(12.0)
                    .show(ui);
                body(ui);
            });
    };

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(UVec2::new(1500, 900), 1.0));

    let mut hug_left = None;
    let mut hug_right = None;
    let mut prop_label = None;
    let mut prop_value = None;

    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            section(ui, "two-hug-columns", &mut |ui| {
                Grid::with_id("two-hug-inner")
                    .cols(Rc::from([Track::hug(), Track::hug()]))
                    .rows(Rc::from([Track::hug()]))
                    .gap_xy(0.0, 16.0)
                    .show(ui, |ui| {
                        hug_left = Some(
                            Text::new(PARAGRAPH)
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        hug_right = Some(
                            Text::new("right column")
                                .size_px(14.0)
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
            });

            section(ui, "property-grid", &mut |ui| {
                Grid::with_id("property-grid-inner")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                    .gap_xy(6.0, 16.0)
                    .show(ui, |ui| {
                        prop_label = Some(
                            Text::new("Title:")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        prop_value = Some(
                            Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
            });
        });
    ui.end_frame();

    let layout = ui.layout_engine.result();
    let l1 = layout.rect(hug_left.unwrap());
    let r1 = layout.rect(hug_right.unwrap());
    let l2 = layout.rect(prop_label.unwrap());
    let r2 = layout.rect(prop_value.unwrap());

    assert!(l1.size.w > 0.0);
    assert!(l2.size.w > 0.0);
    assert!(
        r1.min.x >= l1.max().x - 0.5,
        "two-hug-columns: right cell must start past left cell. left={l1:?}, right={r1:?}",
    );
    assert!(
        r2.min.x >= l2.max().x - 0.5,
        "property-grid: value cell must start past label cell. label={l2:?}, value={r2:?}",
    );
}

/// Render-pass repro: build the property-grid pattern and inspect
/// the emitted `DrawText` commands directly. The visual showcase
/// bug shows label and value columns painted at the SAME x even
/// though their layout rects are correctly side-by-side.
#[test]
fn property_grid_emits_distinct_drawtext_x_positions() {
    use crate::primitives::Track;
    use crate::renderer::RenderCmd;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(UVec2::new(1500, 900), 1.0));
    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Grid::with_id("property-grid-inner")
                .size((Sizing::FILL, Sizing::Hug))
                .cols(Rc::from([Track::hug(), Track::fill()]))
                .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                .gap_xy(6.0, 16.0)
                .show(ui, |ui| {
                    Text::new("Title:").size_px(14.0).grid_cell((0, 0)).show(ui);
                    Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                        .size_px(14.0)
                        .wrapping()
                        .grid_cell((0, 1))
                        .show(ui);
                    Text::new("Description:")
                        .size_px(14.0)
                        .grid_cell((1, 0))
                        .show(ui);
                });
        });
    ui.end_frame();

    let mut cmds = RenderCmdBuffer::new();
    crate::renderer::encode(
        ui.tree(),
        ui.layout_engine.result(),
        ui.cascades.result(),
        None,
        &mut cmds,
    );
    let mut text_xs: Vec<f32> = Vec::new();
    for i in 0..cmds.len() {
        if let RenderCmd::DrawText(payload) = cmds.get(i) {
            text_xs.push(payload.rect.min.x);
        }
    }
    // The two cells in row 0 (Title + Lorem) must emit text at
    // different x positions. Currently the bug renders them at the
    // same x.
    assert!(
        text_xs.len() >= 2,
        "expected at least two DrawText cmds; got {text_xs:?}",
    );
    assert!(
        text_xs[0] != text_xs[1],
        "Title and Lorem texts must paint at different x; got {text_xs:?}",
    );
}

/// Cross-frame measure-cache regression: when the cache hits at a
/// Grid (or any ancestor of a Grid), the grid driver's per-frame
/// `GridHugStore` scratch — populated by `grid::measure` and read
/// by `grid::arrange` — stays at its `reset_for`-zero state because
/// measure was short-circuited. Arrange then computes zero column
/// widths, collapsing every cell to x=0.
///
/// Repro: identical builds across two frames at the same surface
/// → second frame is a cache hit → arrange produces zero-width
/// columns → every cell rect's `min.x` is the grid's `inner.min.x`.
#[test]
fn grid_cells_arranged_correctly_on_cache_hit_frame() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let build = |ui: &mut Ui, capture: &mut Option<(crate::tree::NodeId, crate::tree::NodeId)>| {
        let mut left = None;
        let mut right = None;
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::with_id("g")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug()]))
                    .gap_xy(6.0, 16.0)
                    .show(ui, |ui| {
                        left = Some(
                            Text::new("Title:")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        right = Some(
                            Text::new("value column")
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
            });
        *capture = Some((left.unwrap(), right.unwrap()));
    };

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));

    // Frame 1 (cold).
    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    let mut nodes = None;
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let (l, r) = nodes.unwrap();
    let cold_l = ui.layout_engine.result().rect(l);
    let cold_r = ui.layout_engine.result().rect(r);

    // Frame 2 (warm — cache hit at Panel/Grid).
    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    let mut nodes = None;
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let (l, r) = nodes.unwrap();
    let warm_l = ui.layout_engine.result().rect(l);
    let warm_r = ui.layout_engine.result().rect(r);

    assert_eq!(
        cold_l, warm_l,
        "cache-hit frame must not perturb left-cell rect: cold={cold_l:?} warm={warm_l:?}",
    );
    assert_eq!(
        cold_r, warm_r,
        "cache-hit frame must not perturb right-cell rect: cold={cold_r:?} warm={warm_r:?}",
    );
}

/// Nested grids: outer Grid with an inner Grid in one of its cells.
/// A cache hit at any ancestor must restore hugs for both, each at
/// its current-frame `idx`. Different track counts ensure an
/// out-of-order or dropped grid would surface as a track-count
/// mismatch in restore.
#[test]
fn cache_hit_restores_hugs_for_nested_grids() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let build = |ui: &mut Ui, capture: &mut [Option<crate::tree::NodeId>; 4]| {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::with_id("outer")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug()]))
                    .show(ui, |ui| {
                        capture[0] = Some(
                            Text::new("outer-L")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        Panel::vstack_with_id("inner-host")
                            .grid_cell((0, 1))
                            .show(ui, |ui| {
                                Grid::with_id("inner")
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .cols(Rc::from([Track::hug(), Track::hug(), Track::fill()]))
                                    .rows(Rc::from([Track::hug()]))
                                    .show(ui, |ui| {
                                        capture[1] = Some(
                                            Text::new("a")
                                                .size_px(14.0)
                                                .grid_cell((0, 0))
                                                .show(ui)
                                                .node,
                                        );
                                        capture[2] = Some(
                                            Text::new("bb")
                                                .size_px(14.0)
                                                .grid_cell((0, 1))
                                                .show(ui)
                                                .node,
                                        );
                                        capture[3] = Some(
                                            Text::new("end")
                                                .size_px(14.0)
                                                .grid_cell((0, 2))
                                                .show(ui)
                                                .node,
                                        );
                                    });
                            });
                    });
            });
    };

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));

    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    let mut nodes = [None; 4];
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let cold: Vec<_> = nodes
        .iter()
        .map(|n| ui.layout_engine.result().rect(n.unwrap()))
        .collect();

    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    let mut nodes = [None; 4];
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let warm: Vec<_> = nodes
        .iter()
        .map(|n| ui.layout_engine.result().rect(n.unwrap()))
        .collect();

    assert_eq!(
        cold, warm,
        "cache-hit frame must preserve outer+nested grid cell rects",
    );
}

/// Two sibling Grids inside a vstack: a cache hit at the vstack
/// must restore hug arrays for *both* grids, in pre-order. Catches
/// any regression where the snapshot/restore walk drops a grid or
/// reads them out of order (different track counts → length
/// mismatch + collapse on the wrong grid).
#[test]
fn cache_hit_restores_hugs_for_multiple_sibling_grids() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let build = |ui: &mut Ui, capture: &mut [Option<crate::tree::NodeId>; 4]| {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::with_id("g1")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug()]))
                    .show(ui, |ui| {
                        capture[0] = Some(
                            Text::new("L1:")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        capture[1] = Some(
                            Text::new("v1")
                                .size_px(14.0)
                                .grid_cell((0, 1))
                                .show(ui)
                                .node,
                        );
                    });
                Grid::with_id("g2")
                    .size((Sizing::FILL, Sizing::Hug))
                    .cols(Rc::from([Track::hug(), Track::hug(), Track::fill()]))
                    .rows(Rc::from([Track::hug()]))
                    .show(ui, |ui| {
                        capture[2] = Some(
                            Text::new("Description:")
                                .size_px(14.0)
                                .grid_cell((0, 0))
                                .show(ui)
                                .node,
                        );
                        capture[3] = Some(
                            Text::new("end")
                                .size_px(14.0)
                                .grid_cell((0, 2))
                                .show(ui)
                                .node,
                        );
                    });
            });
    };

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));

    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    let mut nodes = [None; 4];
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let cold: Vec<_> = nodes
        .iter()
        .map(|n| ui.layout_engine.result().rect(n.unwrap()))
        .collect();

    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    let mut nodes = [None; 4];
    build(&mut ui, &mut nodes);
    ui.end_frame();
    let warm: Vec<_> = nodes
        .iter()
        .map(|n| ui.layout_engine.result().rect(n.unwrap()))
        .collect();

    assert_eq!(
        cold, warm,
        "cache-hit frame must preserve all sibling-grid cell rects"
    );
}

/// Diagnostic: full showcase repro. Catches the screenshot bug where
/// two distinct texts emit `DrawText` at the same (x, y).
#[test]
fn text_layouts_full_showcase_drawtext_dump() {
    use crate::primitives::{Stroke, Track};
    use crate::renderer::RenderCmd;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
        Pack my box with five dozen liquor jugs. \
        How vexingly quick daft zebras jump!";

    let section = |ui: &mut Ui, id: &'static str, body: &mut dyn FnMut(&mut Ui)| {
        Panel::vstack_with_id(id)
            .size((Sizing::FILL, Sizing::Hug))
            .gap(6.0)
            .padding(8.0)
            .fill(Color::rgb(0.16, 0.18, 0.22))
            .stroke(Stroke {
                width: 1.0,
                color: Color::rgb(0.30, 0.34, 0.42),
            })
            .radius(4.0)
            .show(ui, |ui| {
                Text::with_id(("section-title", id), "title")
                    .size_px(12.0)
                    .show(ui);
                body(ui);
            });
    };

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(UVec2::new(1620, 980), 1.0));
    Panel::vstack()
        .padding(12.0)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Panel::hstack()
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |_| {});
            Panel::zstack()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(16.0)
                .show(ui, |ui| {
                    Panel::vstack()
                        .gap(16.0)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            section(ui, "two-hug-columns", &mut |ui| {
                                Grid::with_id("two-hug-inner")
                                    .cols(Rc::from([Track::hug(), Track::hug()]))
                                    .rows(Rc::from([Track::hug()]))
                                    .gap_xy(0.0, 16.0)
                                    .show(ui, |ui| {
                                        Text::new(PARAGRAPH)
                                            .size_px(14.0)
                                            .wrapping()
                                            .grid_cell((0, 0))
                                            .show(ui);
                                        Text::new("right column")
                                            .size_px(14.0)
                                            .grid_cell((0, 1))
                                            .show(ui);
                                    });
                            });
                            section(ui, "property-grid", &mut |ui| {
                                Grid::with_id("property-grid-inner")
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .cols(Rc::from([Track::hug(), Track::fill()]))
                                    .rows(Rc::from([Track::hug(), Track::hug(), Track::hug()]))
                                    .gap_xy(6.0, 16.0)
                                    .show(ui, |ui| {
                                        Text::new("Title:")
                                            .size_px(14.0)
                                            .grid_cell((0, 0))
                                            .show(ui);
                                        Text::new(
                                            "Lorem Ipsum is simply dummy text of the printing industry.",
                                        )
                                        .size_px(14.0)
                                        .wrapping()
                                        .grid_cell((0, 1))
                                        .show(ui);
                                        Text::new("Description:")
                                            .size_px(14.0)
                                            .grid_cell((1, 0))
                                            .show(ui);
                                        Text::new(PARAGRAPH)
                                            .size_px(14.0)
                                            .wrapping()
                                            .grid_cell((1, 1))
                                            .show(ui);
                                        Text::new("Tags:")
                                            .size_px(14.0)
                                            .grid_cell((2, 0))
                                            .show(ui);
                                        Text::new("layout, grid, intrinsic, wrapping, css")
                                            .size_px(14.0)
                                            .wrapping()
                                            .grid_cell((2, 1))
                                            .show(ui);
                                    });
                            });
                        });
                });
        });
    ui.end_frame();

    let mut cmds = RenderCmdBuffer::new();
    crate::renderer::encode(
        ui.tree(),
        ui.layout_engine.result(),
        ui.cascades.result(),
        None,
        &mut cmds,
    );
    let mut entries: Vec<(f32, f32, u64)> = Vec::new();
    for i in 0..cmds.len() {
        if let RenderCmd::DrawText(p) = cmds.get(i) {
            entries.push((p.rect.min.x, p.rect.min.y, p.key.text_hash));
        }
    }
    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            let (xi, yi, hi) = entries[i];
            let (xj, yj, hj) = entries[j];
            if hi != hj && (xi - xj).abs() < 0.5 && (yi - yj).abs() < 0.5 {
                panic!(
                    "two distinct texts at same (x,y): #{i} hash={hi:#x} vs #{j} hash={hj:#x} at ({xi}, {yi})",
                );
            }
        }
    }
}

/// Showcase regression: the property-grid section overlapped its
/// label column ("Title:", "Description:", "Tags:") with its
/// wrapping value column. Pin that label cells in column 0 don't
/// horizontally overlap value cells in column 1.
#[test]
fn property_grid_hug_label_does_not_overlap_fill_value() {
    use crate::primitives::Track;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    let mut label = None;
    let mut value = None;
    Panel::vstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Grid::new()
                .size((Sizing::FILL, Sizing::Hug))
                .cols(Rc::from([Track::hug(), Track::fill()]))
                .rows(Rc::from([Track::hug()]))
                .gap_xy(6.0, 16.0)
                .show(ui, |ui| {
                    label = Some(
                        Text::new("Title:")
                            .size_px(14.0)
                            .grid_cell((0, 0))
                            .show(ui)
                            .node,
                    );
                    value = Some(
                        Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                            .size_px(14.0)
                            .wrapping()
                            .grid_cell((0, 1))
                            .show(ui)
                            .node,
                    );
                });
        });
    ui.end_frame();

    let layout = ui.layout_engine.result();
    let lr = layout.rect(label.unwrap());
    let vr = layout.rect(value.unwrap());
    assert!(lr.size.w > 0.0, "label cell must have a positive width");
    assert!(
        vr.min.x >= lr.max().x - 0.5,
        "value cell must start at or past the label cell's right edge: \
         label={lr:?}, value={vr:?}",
    );
}

/// Cache-correctness generalization (catches the grid-hugs class).
/// A measure-cache hit must not perturb ANY downstream consumer of
/// per-frame engine state — so a fully-encoded `RenderCmdBuffer`
/// from a warm frame must be byte-identical to one from a cold
/// frame. This generalizes the rect-equality tests to every pass
/// the encoder reads (cascade rects + invisibility, layout rects,
/// text shapes, transforms, clips, grid track sizes via arrange).
/// Any future per-frame state we forget to snapshot/restore would
/// surface here, not in a targeted test.
#[test]
fn encoded_buffer_stable_across_cache_hit_boundary() {
    use crate::primitives::{Stroke, Track, TranslateScale};
    use crate::renderer::RenderCmdBuffer;
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    // Cover every primitive that has per-frame state worth pinning:
    // wrapping text (text_shapes), grid (hugs), transform/clip
    // (cascade), nested panels.
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(8.0)
            .gap(6.0)
            .show(ui, |ui| {
                Panel::zstack_with_id("transformed")
                    .transform(TranslateScale::new(glam::Vec2::new(4.0, 2.0), 1.0))
                    .clip(true)
                    .size((Sizing::FILL, Sizing::Hug))
                    .padding(6.0)
                    .fill(Color::rgb(0.16, 0.18, 0.22))
                    .stroke(Stroke {
                        width: 1.0,
                        color: Color::rgb(0.3, 0.34, 0.42),
                    })
                    .radius(4.0)
                    .show(ui, |ui| {
                        Grid::with_id("grid")
                            .size((Sizing::FILL, Sizing::Hug))
                            .cols(Rc::from([Track::hug(), Track::fill()]))
                            .rows(Rc::from([Track::hug(), Track::hug()]))
                            .gap_xy(6.0, 8.0)
                            .show(ui, |ui| {
                                Text::new("Title:").size_px(14.0).grid_cell((0, 0)).show(ui);
                                Text::new(
                                    "The quick brown fox jumps over the lazy dog. \
                                     Pack my box with five dozen liquor jugs.",
                                )
                                .size_px(14.0)
                                .wrapping()
                                .grid_cell((0, 1))
                                .show(ui);
                                Text::new("Tag:").size_px(14.0).grid_cell((1, 0)).show(ui);
                                Text::new("layout, grid, intrinsic, wrapping")
                                    .size_px(14.0)
                                    .wrapping()
                                    .grid_cell((1, 1))
                                    .show(ui);
                            });
                    });
                Frame::with_id("under")
                    .size((Sizing::FILL, Sizing::Fixed(20.0)))
                    .fill(Color::rgb(0.4, 0.4, 0.5))
                    .show(ui);
            });
    };

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));

    let encode = |ui: &Ui| {
        let mut cmds = RenderCmdBuffer::new();
        crate::renderer::encode(
            ui.tree(),
            ui.layout_engine.result(),
            ui.cascades.result(),
            None,
            &mut cmds,
        );
        cmds
    };

    // Frame 1: cold.
    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    build(&mut ui);
    ui.end_frame();
    let cold = encode(&ui);

    // Frame 2: warm — cache hit at every reachable subtree.
    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 1.0));
    build(&mut ui);
    ui.end_frame();
    let warm = encode(&ui);

    assert_eq!(
        cold.kinds, warm.kinds,
        "cmd kind sequence must match across cache-hit boundary",
    );
    assert_eq!(
        cold.starts, warm.starts,
        "cmd payload offsets must match across cache-hit boundary",
    );
    assert_eq!(
        cold.data, warm.data,
        "cmd payload bytes must match across cache-hit boundary",
    );
}

/// Stress test: alternating surface widths force the cache through
/// repeated hit/replace transitions. At each step, the warm cache's
/// rects must equal what a cold remeasure produces — a forced miss
/// via `__clear_measure_cache()` is the ground-truth oracle. Catches
/// any "second visit at the original width is wrong" bug where a
/// stale snapshot survives a width change.
#[test]
fn cache_rects_match_cold_oracle_across_width_changes() {
    use crate::primitives::{Track, TranslateScale};
    use crate::text::{CosmicMeasure, share};
    use crate::widgets::{Grid, Text};
    use std::rc::Rc;

    let build = |ui: &mut Ui, capture: &mut Vec<crate::tree::NodeId>| {
        capture.clear();
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack_with_id("xform")
                    .transform(TranslateScale::new(glam::Vec2::new(2.0, 2.0), 1.0))
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Grid::with_id("g")
                            .size((Sizing::FILL, Sizing::Hug))
                            .cols(Rc::from([Track::hug(), Track::fill()]))
                            .rows(Rc::from([Track::hug()]))
                            .show(ui, |ui| {
                                capture.push(
                                    Text::new("Title:")
                                        .size_px(14.0)
                                        .grid_cell((0, 0))
                                        .show(ui)
                                        .node,
                                );
                                capture.push(
                                    Text::new(
                                        "Lorem ipsum dolor sit amet, consectetur \
                                     adipiscing elit, sed do eiusmod tempor.",
                                    )
                                    .size_px(14.0)
                                    .wrapping()
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node,
                                );
                            });
                    });
            });
    };

    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));

    let widths = [800u32, 800, 600, 800, 600, 600, 800, 1000, 600];
    for (i, &w) in widths.iter().enumerate() {
        // Warm pass — cache state from any prior frame applies.
        ui.begin_frame(Display::from_physical(UVec2::new(w, 600), 1.0));
        let mut warm_nodes = Vec::new();
        build(&mut ui, &mut warm_nodes);
        ui.end_frame();
        let warm_rects: Vec<_> = warm_nodes
            .iter()
            .map(|n| ui.layout_engine.result().rect(*n))
            .collect();

        // Cold oracle: same width, same build, but the cache is
        // cleared so measure runs from scratch. Rects must match.
        ui.__clear_measure_cache();
        ui.begin_frame(Display::from_physical(UVec2::new(w, 600), 1.0));
        let mut cold_nodes = Vec::new();
        build(&mut ui, &mut cold_nodes);
        ui.end_frame();
        let cold_rects: Vec<_> = cold_nodes
            .iter()
            .map(|n| ui.layout_engine.result().rect(*n))
            .collect();

        assert_eq!(
            warm_rects, cold_rects,
            "step {i}: warm-cache rects diverged from cold remeasure at width={w}",
        );
    }
}
