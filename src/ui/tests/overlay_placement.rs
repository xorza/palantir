//! Anchored side-layer placement through the published surface — the
//! path a widget written outside this crate takes.

use crate::Ui;
use crate::layout::types::overlay::OverlayPosition;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 300);
const BODY_W: f32 = 120.0;
const BODY_H: f32 = 100.0;

fn body(ui: &mut Ui) {
    Panel::vstack()
        .id(WidgetId::from_hash("overlay-body"))
        .size((Sizing::fixed(BODY_W), Sizing::fixed(BODY_H)))
        .show(ui, |_| {});
}

/// Record `place` under a full-surface main panel and report where the
/// popup layer's root landed.
fn placed(mut place: impl FnMut(&mut Ui)) -> Rect {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, &mut place);
    });
    let root = h.ui.tree(Layer::Popup).roots[0].first_node.idx();
    h.ui.layout(Layer::Popup).rect[root]
}

/// The three answers `OverlayPosition` gives for one 120x100 body,
/// hand-computed against a 400x300 surface:
///
/// - Anchor `(20, 30, 100, 20)`, no gap: the body fits below, so its top
///   is the anchor's bottom, `30 + 20 = 50`, and the cross axis starts
///   at the anchor's left, `20`.
/// - The same anchor with a gap of 8: `50 + 8 = 58`. The gap is the only
///   difference between the two rows, so they prove it reaches the
///   resolver.
/// - Anchor `(20, 250, 100, 20)`: below would put the top at `270` and
///   the bottom at `370`, past the surface, so the body flips above —
///   `250 - 100 = 150`, which fits.
#[test]
fn an_anchored_layer_takes_the_gap_and_flips_to_fit() {
    let cases: &[(Rect, f32, Vec2)] = &[
        (
            Rect::new(20.0, 30.0, 100.0, 20.0),
            0.0,
            Vec2::new(20.0, 50.0),
        ),
        (
            Rect::new(20.0, 30.0, 100.0, 20.0),
            8.0,
            Vec2::new(20.0, 58.0),
        ),
        (
            Rect::new(20.0, 250.0, 100.0, 20.0),
            0.0,
            Vec2::new(20.0, 150.0),
        ),
    ];
    for &(anchor, gap, expected) in cases {
        let rect = placed(|ui| {
            ui.layer(Layer::Popup)
                .anchored(OverlayPosition::below(anchor).gap(gap))
                .show(body);
        });
        assert_eq!(rect.min, expected, "anchor {anchor:?} gap {gap}");
        assert_eq!(rect.size.w, BODY_W, "anchor {anchor:?} gap {gap}");
        assert_eq!(rect.size.h, BODY_H, "anchor {anchor:?} gap {gap}");
    }
}

/// `at` is the other origin form and it does not move. The same
/// near-bottom point that made `anchored` flip leaves a fixed layer
/// hanging off the surface, which is what makes the two distinct
/// answers rather than one with a tolerance.
#[test]
fn a_fixed_layer_stays_where_it_was_put() {
    let rect = placed(|ui| {
        ui.layer(Layer::Popup).at(Vec2::new(20.0, 250.0)).show(body);
    });
    assert_eq!(rect.min, Vec2::new(20.0, 250.0));
}
