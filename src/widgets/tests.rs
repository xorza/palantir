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
    ui.end_frame(Rect::new(0.0, 0.0, 400.0, 200.0));

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
    ui.end_frame(Rect::new(0.0, 0.0, 400.0, 100.0));

    let kids: Vec<_> = ui.tree.children(root).collect();
    let hid = ui.rect(kids[1]);
    let b = ui.rect(kids[2]);
    // Hidden node still occupies its slot.
    assert_eq!(hid.size.w, 40.0);
    // ...so b's offset includes hidden's width + both gaps.
    assert_eq!(b.min.x, 40.0 + 10.0 + 40.0 + 10.0);

    // ...but emits no DrawRect.
    let mut cmds = Vec::new();
    encode(
        &ui.tree,
        ui.layout_result(),
        ui.cascades(),
        1.0,
        None,
        &mut cmds,
    );
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
    ui.end_frame(Rect::new(0.0, 0.0, 400.0, 200.0));

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
    ui.end_frame(Rect::new(0.0, 0.0, 200.0, 100.0));

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
    ui.end_frame(Rect::new(0.0, 0.0, 400.0, 400.0));

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
    ui.end_frame(Rect::new(0.0, 0.0, 400.0, 400.0));

    let r = ui.rect(text_node.unwrap());
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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 200.0, 400.0));

    let node = text_node.unwrap();
    let shaped = ui
        .layout_result()
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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 200.0, 400.0));

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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 200.0, 400.0));

    let shaped = ui
        .layout_result()
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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 200.0, 400.0));

    let shaped = ui
        .layout_result()
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
    ui.begin_frame();
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
    ui.layout(Rect::new(0.0, 0.0, 800.0, 600.0));

    // Hug ZStack should hug the Fixed child (60 × 40). If the per-axis
    // logic broke, ZStack would stretch to surface size.
    let r = ui.rect(zstack_node.unwrap());
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
        ui.begin_frame();
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
        ui.end_frame(Rect::new(0.0, 0.0, surface_w, 400.0));
        ui.layout_result()
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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 200.0, 400.0));

    let shaped = ui
        .layout_result()
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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 200.0, 400.0));

    let shaped = ui
        .layout_result()
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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 200.0, 400.0));

    let shaped = ui
        .layout_result()
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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 400.0, 600.0));

    // Two rows of 14 px text. Single-line height ≈18 px → 36 px total
    // would mean both rows collapsed to single-line (no wrapping
    // accounted for). Wrapped paragraph in the value col must push
    // row 0 to multiple lines.
    let h = ui.layout_result().rect(grid_node.unwrap()).size.h;
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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 400.0, 600.0));

    let h = ui.layout_result().rect(grid_node.unwrap()).size.h;
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
    ui.begin_frame();
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
    ui.end_frame(Rect::new(0.0, 0.0, 200.0, 400.0));

    let shaped_w = ui
        .layout_result()
        .text_shape(message_node.unwrap())
        .expect("text was shaped")
        .measured
        .w;
    let rect_w = ui.layout_result().rect(message_node.unwrap()).size.w;

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
