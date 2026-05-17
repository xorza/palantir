//! End-to-end tests for `Popup`'s click-outside contract.
//!
//! Both `Block` and `Dismiss` install a full-surface click-eater
//! leaf in the `Popup` layer behind the body. These tests pin:
//! - clicks **inside** the body's rect aren't classified as outside
//!   clicks (no `dismissed`, no eater click);
//! - clicks **outside** the body land on the eater (popup beats Main
//!   in hit-test) and are consumed before reaching Main;
//! - `Dismiss` surfaces the outside-click via `PopupResponse.dismissed`
//!   while `Block` swallows it silently.

use crate::Ui;
use crate::forest::element::Configure;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::panel::Panel;
use crate::widgets::popup::{ClickOutside, Popup};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 400);
const ANCHOR: Vec2 = Vec2::new(50.0, 50.0);
const BODY_W: f32 = 100.0;
const BODY_H: f32 = 60.0;

// `Ui::frame` re-runs the build closure when action input is pending,
// so we OR `dismissed` across passes — pass 1 sees the click, pass 2
// would otherwise overwrite with a fresh false.
fn record_body(ui: &mut Ui, config: ClickOutside, dismissed: &mut bool) {
    Panel::vstack()
        .id(WidgetId::from_hash("main-bg"))
        .size((Sizing::FILL, Sizing::FILL))
        .sense(crate::Sense::CLICK)
        .show(ui, |ui| {
            let r = Popup::anchored_to(ANCHOR)
                .id(WidgetId::from_hash("test-popup"))
                .click_outside(config)
                .padding(4.0)
                .show(ui, |ui, _popup| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("popup-content"))
                        .size((Sizing::Fixed(100.0), Sizing::Fixed(60.0)))
                        .show(ui, |_| {});
                });
            *dismissed |= r.dismissed;
        });
}

fn main_panel_clicked(ui: &Ui) -> bool {
    let main_id = crate::primitives::widget_id::WidgetId::from_hash("main-bg");
    ui.response_for(main_id).clicked
}

#[test]
fn click_inside_popup_does_not_dismiss() {
    let mut ui = Ui::for_test();
    let mut dismissed = false;
    ui.run_at_acked(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    let inside = Vec2::new(ANCHOR.x + BODY_W * 0.5, ANCHOR.y + BODY_H * 0.5);
    ui.click_at(inside);

    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    assert!(!dismissed, "click inside body must not signal dismissal");
    assert!(
        !main_panel_clicked(&ui),
        "click inside body must not leak to Main"
    );
}

#[test]
fn click_outside_popup_dismisses_and_blocks_main() {
    let mut ui = Ui::for_test();
    let mut dismissed = false;
    ui.run_at_acked(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    ui.click_at(Vec2::new(300.0, 300.0));

    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    assert!(
        dismissed,
        "outside click with `Dismiss` must signal dismissal"
    );
    assert!(
        !main_panel_clicked(&ui),
        "outside click must be eaten by the popup eater, not leak to Main",
    );
}

/// `Ui::frame` settles popup dismissal in a single host call.
/// Pass 1 records the open popup, sees the eater click, sets
/// `dismissed = true`, host flips `open = false`. Pass 2 sees
/// `open == false` and records no popup. The painted tree (pass 2)
/// has no popup-layer widgets — no stale frame ever reaches submit.
#[test]
fn run_frame_settles_popup_dismissal_in_one_call() {
    let mut ui = Ui::for_test();
    let mut open = true;
    let scene = |ui: &mut Ui, open: &mut bool| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                if *open {
                    let r = Popup::anchored_to(ANCHOR)
                        .id(WidgetId::from_hash("test-popup"))
                        .click_outside(ClickOutside::Dismiss)
                        .show(ui, |ui, _popup| {
                            Panel::vstack()
                                .id(WidgetId::from_hash("popup-content"))
                                .size((Sizing::Fixed(100.0), Sizing::Fixed(60.0)))
                                .show(ui, |_| {});
                        });
                    if r.dismissed {
                        *open = false;
                    }
                }
            });
    };
    ui.run_at_acked(SURFACE, |ui| scene(ui, &mut open));
    ui.click_at(Vec2::new(300.0, 300.0));
    ui.run_at(SURFACE, |ui| scene(ui, &mut open));
    assert!(!open, "host flag must flip to false in pass 1");
    assert_eq!(
        ui.forest.tree(crate::forest::Layer::Popup).records.len(),
        0,
        "painted tree (pass 2) must contain no Popup-layer widgets",
    );
}

/// Pin popup-body sizing under each `Sizing` mode.
#[test]
fn popup_body_sizing_matches_sizing_mode() {
    use crate::forest::Layer;
    let anchor = Vec2::new(20.0, 30.0);
    let cases: &[(Sizing, Sizing, Size)] = &[
        (Sizing::Hug, Sizing::Hug, Size::new(100.0, 60.0)),
        (
            Sizing::FILL,
            Sizing::FILL,
            Size::new(SURFACE.x as f32 - anchor.x, SURFACE.y as f32 - anchor.y),
        ),
        (
            Sizing::Fixed(80.0),
            Sizing::Fixed(40.0),
            Size::new(80.0, 40.0),
        ),
    ];
    for &(sw, sh, expected) in cases {
        let mut ui = Ui::for_test();
        ui.run_at(SURFACE, |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("main-bg"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Popup::anchored_to(anchor)
                        .id(WidgetId::from_hash("sized-popup"))
                        .padding(0.0)
                        .size((sw, sh))
                        .show(ui, |ui, _popup| {
                            Panel::vstack()
                                .id(WidgetId::from_hash("popup-content"))
                                .size((Sizing::Fixed(100.0), Sizing::Fixed(60.0)))
                                .show(ui, |_| {});
                        });
                });
        });
        let popup_tree = ui.forest.tree(Layer::Popup);
        let body_root = popup_tree.roots[1].first_node as usize;
        let body_rect = ui.layout[Layer::Popup].rect[body_root];
        assert_eq!(
            body_rect.size, expected,
            "size=({:?},{:?}) → expected {:?}, got {:?}",
            sw, sh, expected, body_rect.size,
        );
        assert_eq!(body_rect.min, anchor, "anchor placement preserved");
    }
}

#[test]
fn click_outside_blocks_main_without_signaling_with_block_mode() {
    let mut ui = Ui::for_test();
    let mut dismissed = false;
    ui.run_at_acked(SURFACE, |ui| {
        record_body(ui, ClickOutside::Block, &mut dismissed);
    });
    ui.click_at(Vec2::new(300.0, 300.0));

    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| {
        record_body(ui, ClickOutside::Block, &mut dismissed);
    });
    assert!(!dismissed, "`Block` mode must not signal dismissal");
    assert!(
        !main_panel_clicked(&ui),
        "`Block` mode must still eat the click — no leak to Main",
    );
}
