use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};
use glam::{UVec2, Vec2};

/// `Surface::apply_to` (from `Panel::show`) writes the clip bit and records chrome in `Tree::chrome_table`. One fixture sweeps: no surface, paint-only `From<Background>`, `scissor`, `clipped`, `rounded` with radius, `rounded` with zero-radius downgrade.
#[test]
fn surface_apply_to_sets_clip_bit_and_chrome() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let mut cases: Vec<(&str, NodeId, ClipMode, bool)> = Vec::new();
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let n = Panel::zstack()
                .id(WidgetId::from_hash("none"))
                .size(50.0)
                .show(ui, |_| {})
                .response
                .node();
            cases.push(("none", n, ClipMode::None, false));

            let n = Panel::zstack()
                .id(WidgetId::from_hash("paint-only"))
                .size(50.0)
                .background(Background::fill(RgbaF32::srgb(0.5, 0.5, 0.5)))
                .show(ui, |_| {})
                .response
                .node();
            cases.push(("paint-only", n, ClipMode::None, true));

            // Chrome is dropped at install (`Tree::open_node` filters invisible paint); only the clip flag survives.
            let n = Panel::zstack()
                .id(WidgetId::from_hash("scissor"))
                .size(50.0)
                .clip_rect()
                .show(ui, |_| {})
                .response
                .node();
            cases.push(("scissor", n, ClipMode::Rect, false));

            let n = Panel::zstack()
                .id(WidgetId::from_hash("clipped"))
                .size(50.0)
                .background(Background::fill(RgbaF32::srgb(0.2, 0.2, 0.2)))
                .clip_rect()
                .show(ui, |_| {})
                .response
                .node();
            cases.push(("clipped", n, ClipMode::Rect, true));

            let n = Panel::zstack()
                .id(WidgetId::from_hash("rounded"))
                .size(50.0)
                .background(Background {
                    fill: RgbaF32::srgb(0.2, 0.2, 0.2).into(),
                    corners: Corners::all(4.0),
                    ..Default::default()
                })
                .clip_rounded()
                .show(ui, |_| {})
                .response
                .node();
            cases.push(("rounded", n, ClipMode::Rounded, true));

            // Zero radius: open_node downgrades.
            let n = Panel::zstack()
                .id(WidgetId::from_hash("rounded-zero"))
                .size(50.0)
                .background(Background::fill(RgbaF32::srgb(0.2, 0.2, 0.2)))
                .clip_rounded()
                .show(ui, |_| {})
                .response
                .node();
            cases.push(("rounded-zero", n, ClipMode::Rect, true));
        });
    });
    for (name, id, expected_clip, expects_chrome) in &cases {
        let clip = h.ui.tree(Layer::Main).records.attrs()[id.idx()].clip_mode();
        assert_eq!(clip, *expected_clip, "[{name}] clip mode");
        let chrome = h.ui.tree(Layer::Main).chrome(*id);
        assert_eq!(
            chrome.is_some(),
            *expects_chrome,
            "[{name}] chrome stamping"
        );
    }
}

#[test]
fn explicit_no_chrome_and_no_clip_override_panel_theme() {
    let mut h = UiHarness::new(UVec2::new(200, 120));
    h.ui.theme_mut().panel_background = Some(Background::fill(RgbaF32::WHITE));
    h.ui.theme_mut().panel_clip = ClipMode::Rect;
    let [explicit, inherited] = h.frame_value(|ui| {
        [
            Panel::vstack()
                .background(Background::NONE)
                .clip(ClipMode::None)
                .show(ui, |_| {})
                .response
                .node(),
            Panel::vstack().show(ui, |_| {}).response.node(),
        ]
    });

    let tree = h.ui.tree(Layer::Main);
    assert_eq!(
        tree.records.attrs()[explicit.idx()].clip_mode(),
        ClipMode::None,
    );
    assert!(tree.chrome(explicit).is_none());
    assert_eq!(
        tree.records.attrs()[inherited.idx()].clip_mode(),
        ClipMode::Rect,
    );
    assert!(tree.chrome(inherited).is_some());
}

#[test]
fn panel_hugs_largest_child_and_layers_them() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    let [panel_node, a_node, b_node] = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                let panel = Panel::zstack()
                    .id(WidgetId::from_hash("card"))
                    .padding(10.0)
                    .background(Background {
                        fill: RgbaF32::srgb(0.1, 0.1, 0.15).into(),
                        corners: Corners::all(8.0),
                        ..Default::default()
                    })
                    .show(ui, |ui| {
                        [
                            Button::new()
                                .id(WidgetId::from_hash("a"))
                                .size((Sizing::fixed(80.0), Sizing::fixed(30.0)))
                                .show(ui)
                                .node(),
                            Button::new()
                                .id(WidgetId::from_hash("b"))
                                .size((Sizing::fixed(60.0), Sizing::fixed(50.0)))
                                .show(ui)
                                .node(),
                        ]
                    });
                [panel.response.node(), panel.inner[0], panel.inner[1]]
            })
            .inner
    });
    // Panel hugs to (max(80, 60) + 2*10, max(30, 50) + 2*10) = (100, 70).
    let panel = h.ui.arranged_rect(Layer::Main, panel_node);
    assert_eq!(panel.size.w, 100.0);
    assert_eq!(panel.size.h, 70.0);

    let a = h.ui.arranged_rect(Layer::Main, a_node);
    let b = h.ui.arranged_rect(Layer::Main, b_node);
    assert_eq!((a.min.x, a.min.y), (10.0, 10.0));
    assert_eq!((b.min.x, b.min.y), (10.0, 10.0));
    assert_eq!((a.size.w, a.size.h), (80.0, 30.0));
    assert_eq!((b.size.w, b.size.h), (60.0, 50.0));

    assert!(
        h.ui.tree(Layer::Main)
            .shapes_of(panel_node)
            .next()
            .is_none(),
        "panel chrome doesn't show up in the shape stream"
    );
    assert!(
        h.ui.tree(Layer::Main).chrome(panel_node).is_some(),
        "panel chrome recorded in chrome table",
    );
}

#[test]
fn panel_with_fill_child_grows_to_panel_inner() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let child_node = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("p"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
                    .padding(10.0)
                    .show(ui, |ui| {
                        Block::new()
                            .id(WidgetId::from_hash("filler"))
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background::fill(RgbaF32::srgb(0.5, 0.5, 0.5)))
                            .show(ui)
                            .node()
                    })
                    .inner
            })
            .inner
    });
    let child = h.ui.arranged_rect(Layer::Main, child_node);
    // Panel = 200×100; inner (after padding 10) = 180×80, child fills it at (10, 10).
    assert_eq!(child.min.x, 10.0);
    assert_eq!(child.min.y, 10.0);
    assert_eq!(child.size.w, 180.0);
    assert_eq!(child.size.h, 80.0);
}

/// Regression: a child in a `.disabled(true)` panel must see `state.disabled = true` while recording on its first frame. Cascade lags a frame, so without `Forest::ancestor_disabled` the animation cache snapped to the alive look and flashed it.
#[test]
fn child_inside_disabled_panel_sees_disabled_at_record_time() {
    use crate::primitives::identity::widget_id::WidgetId;
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let child_id = WidgetId::from_hash("child");
    let observed = h.frame_value(|ui| {
        Panel::vstack()
            .auto_id()
            .disabled(true)
            .show(ui, |ui| {
                let observed = ui.response_for(child_id);
                Block::new().id(child_id).size(10.0).show(ui);
                observed
            })
            .inner
    });
    assert!(
        observed.disabled,
        "child inside disabled panel must see disabled at record time",
    );
}

/// The enabled row is the control: the same click lands there, so the disabled row can't pass by missing.
#[test]
fn disabled_panel_suppresses_clicks_on_descendants() {
    use glam::Vec2;

    for (disabled, clicks) in [(true, false), (false, true)] {
        let mut h = UiHarness::new(UVec2::new(400, 200));
        let body = |ui: &mut Ui| {
            let mut clicked = false;
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("locked"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(80.0)))
                    .padding(20.0)
                    .background(Background::fill(RgbaF32::srgb(0.2, 0.2, 0.2)))
                    .disabled(disabled)
                    .show(ui, |ui| {
                        clicked = Button::new()
                            .id(WidgetId::from_hash("inside"))
                            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                            .show(ui)
                            .left
                            .clicked();
                    });
            });
            clicked
        };
        h.frame(|ui| {
            body(ui);
        });
        h.click_at(Vec2::new(40.0, 40.0));

        let passes = h.frame_passes(body);
        assert_eq!(*passes.a(), clicks, "disabled = {disabled}");
        assert_eq!(passes.count_where(|clicked| *clicked), usize::from(clicks));
    }
}

#[test]
fn canvas_places_children_at_absolute_positions_and_hugs_bbox() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let [canvas_node, a_node, b_node] = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                let canvas = Panel::canvas().id(WidgetId::from_hash("c")).show(ui, |ui| {
                    [
                        Block::new()
                            .id(WidgetId::from_hash("a"))
                            .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                            .position(Vec2::new(10.0, 5.0))
                            .show(ui)
                            .node(),
                        Block::new()
                            .id(WidgetId::from_hash("b"))
                            .size((Sizing::fixed(30.0), Sizing::fixed(60.0)))
                            .position(Vec2::new(80.0, 40.0))
                            .show(ui)
                            .node(),
                    ]
                });
                [canvas.response.node(), canvas.inner[0], canvas.inner[1]]
            })
            .inner
    });
    let c = h.ui.arranged_rect(Layer::Main, canvas_node);
    // Hugs bbox: max(10+40, 80+30)=110, max(5+20, 40+60)=100.
    assert_eq!(c.size.w, 110.0);
    assert_eq!(c.size.h, 100.0);

    let a = h.ui.arranged_rect(Layer::Main, a_node);
    let b = h.ui.arranged_rect(Layer::Main, b_node);
    assert_eq!((a.min.x, a.min.y), (10.0, 5.0));
    assert_eq!((a.size.w, a.size.h), (40.0, 20.0));
    assert_eq!((b.min.x, b.min.y), (80.0, 40.0));
    assert_eq!((b.size.w, b.size.h), (30.0, 60.0));
}

#[test]
fn zstack_layers_children_without_painting_background() {
    // In an HStack so the ZStack's Hug size is honored (a root would expand to the surface).
    let mut h = UiHarness::new(UVec2::new(400, 200));
    let [z, bg_node, fg_node] = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                let zstack = Panel::zstack()
                    .id(WidgetId::from_hash("layered"))
                    .show(ui, |ui| {
                        [
                            Block::new()
                                .id(WidgetId::from_hash("bg"))
                                .size((Sizing::fixed(120.0), Sizing::fixed(80.0)))
                                .background(Background::fill(RgbaF32::srgb(0.1, 0.1, 0.2)))
                                .show(ui)
                                .node(),
                            Button::new()
                                .id(WidgetId::from_hash("fg"))
                                .size((Sizing::fixed(60.0), Sizing::fixed(30.0)))
                                .show(ui)
                                .node(),
                        ]
                    });
                [zstack.response.node(), zstack.inner[0], zstack.inner[1]]
            })
            .inner
    });
    assert!(h.ui.tree(Layer::Main).shapes_of(z).next().is_none());

    let zr = h.ui.arranged_rect(Layer::Main, z);
    assert_eq!(zr.size.w, 120.0);
    assert_eq!(zr.size.h, 80.0);

    let bg = h.ui.arranged_rect(Layer::Main, bg_node);
    let fg = h.ui.arranged_rect(Layer::Main, fg_node);
    assert_eq!((bg.min.x, bg.min.y), (0.0, 0.0));
    assert_eq!((fg.min.x, fg.min.y), (0.0, 0.0));
    assert_eq!((bg.size.w, bg.size.h), (120.0, 80.0));
    assert_eq!((fg.size.w, fg.size.h), (60.0, 30.0));
}

/// ZStack inner = 200×100, child = 40×20. `align` resolves per axis: Center → (100-40)/2 leading; End → inner - child; Start → 0.
#[test]
fn zstack_aligns_child_per_axis() {
    let cases: &[(&str, Align, (f32, f32))] = &[
        ("center", Align::CENTER, (80.0, 40.0)),
        (
            "right_center_independent_axes",
            Align::new(HAlign::Right, VAlign::Center),
            (160.0, 40.0),
        ),
    ];
    for (label, align, expected) in cases {
        let mut h = UiHarness::new(UVec2::new(400, 400));
        let child_node = h.frame_value(|ui| {
            Panel::hstack()
                .auto_id()
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("box"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
                        .show(ui, |ui| {
                            Block::new()
                                .id(WidgetId::from_hash("c"))
                                .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                                .align(*align)
                                .background(Background::fill(RgbaF32::srgb(0.5, 0.5, 0.5)))
                                .show(ui)
                                .node()
                        })
                        .inner
                })
                .inner
        });
        let r = h.ui.arranged_rect(Layer::Main, child_node);
        assert_eq!((r.min.x, r.min.y), *expected, "case: {label}");
        assert_eq!(
            (r.size.w, r.size.h),
            (40.0, 20.0),
            "case: {label} Fixed size honored under align"
        );
    }
}
