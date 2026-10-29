//! Which panes a frame's damage reaches from below.

use crate::damage::tests::support::{BLUE, DISPLAY, RED, frame};
use crate::internals::ui_harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::paint_animation::curves;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use std::time::Duration;

/// σ of every pane here: at scale 1 it stays at full resolution, reaching its radius, 16 px.
const SIGMA: f32 = 4.0;

/// A pane's rect and the screen its footprint covers: grown by the reach and a pixel.
fn footprint(rect: Rect) -> Rect {
    rect.inflated(BlurPlan::new(SIGMA).reach() as f32 + 1.0)
}

#[derive(Clone, Copy, Debug)]
struct Scene {
    /// A square painted before the panes, if any, and its color.
    under: Option<(Rect, RgbaF32)>,
    /// The panes, each on its own canvas in paint order.
    panes: &'static [Rect],
    /// The color of a square painted on the first pane's canvas, after its pane.
    over: RgbaF32,
    /// A blinking square on the first pane's canvas, after its pane.
    blink: bool,
}

const UNDER: Rect = Rect::new(30.0, 30.0, 10.0, 10.0);
const PANE: Rect = Rect::new(40.0, 40.0, 40.0, 40.0);
const OVER: Rect = Rect::new(50.0, 50.0, 10.0, 10.0);

impl Scene {
    fn record(self, ui: &mut Ui) {
        Panel::zstack()
            .id(WidgetId::from_hash("scene"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("under"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        if let Some((rect, color)) = self.under {
                            ui.add_shape(Shape::rect(rect).fill(color));
                        }
                    });
                for (i, &rect) in self.panes.iter().enumerate() {
                    Panel::canvas()
                        .id(WidgetId::from_hash(("pane", i)))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            ui.add_shape(
                                Shape::rect(rect).backdrop_filter(BackdropFilter::blur(SIGMA)),
                            );
                            if i == 0 {
                                ui.add_shape(Shape::rect(OVER).fill(self.over));
                                if self.blink {
                                    ui.add_shape_animated(
                                        Shape::rect(Rect::new(60.0, 60.0, 2.0, 10.0))
                                            .fill(RgbaF32::WHITE),
                                        PaintAnimation::alpha(0.0, 1.0)
                                            .with_period(Duration::from_millis(1000))
                                            .with_steps(2)
                                            .with_curve(curves::square),
                                    );
                                }
                            }
                        });
                }
            });
    }
}

/// The key of pane `i`: the first row of its chromeless canvas.
fn key(i: usize) -> PaneKey {
    PaneKey {
        owner: WidgetId::from_hash(("pane", i)),
        shape: 0,
    }
}

/// Two frames of `before` then `after`; the panes the second found stale, and its raw rects.
fn second(before: Scene, after: Scene) -> (Vec<PaneKey>, Vec<Rect>) {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| before.record(ui));
    frame(&mut h, |ui| after.record(ui));
    let dirty = (0..after.panes.len())
        .map(key)
        .filter(|&k| h.engines.damage.dirty_panes().contains(k))
        .collect();
    (dirty, h.engines.damage.raw_rects().to_vec())
}

const ONE: Scene = Scene {
    under: Some((UNDER, BLUE)),
    panes: &[PANE],
    over: BLUE,
    blink: false,
};

/// A change painted before a pane, inside its footprint, makes it stale and damages its rect;
/// one after it, inside its rect, does not; one before it but outside its footprint does not.
#[test]
fn damage_before_a_pane_inside_its_footprint_makes_it_stale() {
    assert!(footprint(PANE).intersects(UNDER));
    let (dirty, rects) = second(
        ONE,
        Scene {
            under: Some((UNDER, RED)),
            ..ONE
        },
    );
    assert_eq!(dirty, [key(0)]);
    assert!(rects.contains(&PANE), "the pane's rect joins the damage");

    let (dirty, rects) = second(ONE, Scene { over: RED, ..ONE });
    assert!(dirty.is_empty(), "a change above the pane leaves it");
    assert!(!rects.contains(&PANE));

    let far = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert!(!footprint(PANE).intersects(far));
    let (dirty, _) = second(
        Scene {
            under: Some((far, BLUE)),
            ..ONE
        },
        Scene {
            under: Some((far, RED)),
            ..ONE
        },
    );
    assert!(dirty.is_empty(), "a change outside the footprint leaves it");
}

/// A widget removed from under a pane makes it stale: its rows no longer have a place.
#[test]
fn a_removed_widget_under_a_pane_makes_it_stale() {
    let (dirty, _) = second(ONE, Scene { under: None, ..ONE });
    assert_eq!(dirty, [key(0)]);
}

/// A pane over a pane: the lower one's stale rect lies in the upper one's footprint, so one
/// forward walk makes both stale, though the change is outside the upper footprint.
#[test]
fn stacked_panes_go_stale_in_one_walk() {
    const STACKED: &[Rect] = &[PANE, Rect::new(70.0, 70.0, 30.0, 30.0)];
    let near = Rect::new(25.0, 25.0, 5.0, 5.0);
    assert!(footprint(PANE).intersects(near));
    assert!(!footprint(STACKED[1]).intersects(near));
    let (dirty, _) = second(
        Scene {
            under: Some((near, BLUE)),
            panes: STACKED,
            ..ONE
        },
        Scene {
            under: Some((near, RED)),
            panes: STACKED,
            ..ONE
        },
    );
    assert_eq!(dirty, [key(0), key(1)]);
}

/// A caret-like blink after a pane, inside it, repaints without recording: the paint-only frame
/// damages the blink's rect and leaves the pane.
#[test]
fn a_blink_inside_a_pane_leaves_it() {
    let scene = Scene { blink: true, ..ONE };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| scene.record(ui));
    h.advance(Duration::from_millis(600));
    let damage = frame(&mut h, |ui| scene.record(ui));
    assert!(damage.is_some(), "the blink flipped");
    assert!(!h.engines.damage.dirty_panes().contains(key(0)));
}

/// A chrome with a backdrop filter is a pane too, keyed by its node's first row: a change under it,
/// inside its footprint, makes it stale, and its rect joins the damage.
#[test]
fn a_chrome_pane_goes_stale_as_a_shape_pane() {
    let record = |under: RgbaF32| {
        move |ui: &mut Ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("chrome-scene"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::rect(UNDER).fill(under));
                    Panel::canvas()
                        .id(WidgetId::from_hash("chrome-pane"))
                        .pos(PANE.min)
                        .size((Sizing::fixed(PANE.size.w), Sizing::fixed(PANE.size.h)))
                        .chrome(Chrome::NONE.with_backdrop_filter(BackdropFilter::blur(SIGMA)))
                        .show(ui, |_| {});
                });
        }
    };
    let key = PaneKey {
        owner: WidgetId::from_hash("chrome-pane"),
        shape: 0,
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, record(BLUE));
    frame(&mut h, record(BLUE));
    assert!(!h.engines.damage.dirty_panes().contains(key), "still");
    frame(&mut h, record(RED));
    assert!(h.engines.damage.dirty_panes().contains(key), "under");
    assert!(h.engines.damage.raw_rects().contains(&PANE));
}

/// A row removed before a pane on its canvas, outside the pane's footprint, gives the pane a new
/// key, which no capture holds: the pane is stale and its rect joins the damage, though the row
/// matcher sees the same pane row. The frame after, which changes nothing, leaves it.
#[test]
fn a_pane_whose_row_moves_is_stale() {
    let far = Rect::new(0.0, 0.0, 4.0, 4.0);
    assert!(!footprint(PANE).intersects(far));
    let record = |lead: bool| {
        move |ui: &mut Ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("moved"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    if lead {
                        ui.add_shape(Shape::rect(far).fill(BLUE));
                    }
                    ui.add_shape(Shape::rect(PANE).backdrop_filter(BackdropFilter::blur(SIGMA)));
                });
        }
    };
    let moved = PaneKey {
        owner: WidgetId::from_hash("moved"),
        shape: 0,
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, record(true));
    frame(&mut h, record(false));
    assert!(h.engines.damage.dirty_panes().contains(moved), "moved");
    assert!(h.engines.damage.raw_rects().contains(&PANE));
    frame(&mut h, record(false));
    assert!(!h.engines.damage.dirty_panes().contains(moved), "still");
}
