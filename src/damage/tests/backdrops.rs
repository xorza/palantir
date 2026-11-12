//! Which backdrops a frame's damage reaches from below.

use crate::damage::tests::support::{BLUE, DISPLAY, RED, frame};
use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::scene::plane::Plane;
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::paint_animation::curves;
use crate::shape::rect::RectShape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use std::time::Duration;

/// σ of every backdrop here: at scale 1 it stays at full resolution, reaching its radius, 16 px.
const SIGMA: f32 = 4.0;

/// A backdrop's rect and the screen its footprint covers: grown by the reach and a pixel.
fn footprint(rect: Rect) -> Rect {
    rect.inflated(BlurPlan::new(SIGMA).reach() as f32 + 1.0)
}

#[derive(Clone, Copy, Debug)]
struct Scene {
    /// A square painted before the backdrops, if any, and its color.
    under: Option<(Rect, RgbaF32)>,
    /// The backdrops, each on its own canvas in paint order.
    backdrops: &'static [Rect],
    /// The color of a square painted on the first backdrop's canvas, after its backdrop.
    over: RgbaF32,
    /// A blinking square on the first backdrop's canvas, after its backdrop.
    blink: bool,
}

const UNDER: Rect = Rect::new(30.0, 30.0, 10.0, 10.0);
const BACKDROP: Rect = Rect::new(40.0, 40.0, 40.0, 40.0);
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
                            ui.add_shape(Shape::rect().at(rect), color);
                        }
                    });
                for (i, &rect) in self.backdrops.iter().enumerate() {
                    Panel::canvas()
                        .id(WidgetId::from_hash(("backdrop", i)))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            ui.add_shape(Shape::rect().at(rect), Material::blur(SIGMA));
                            if i == 0 {
                                ui.add_shape(Shape::rect().at(OVER), self.over);
                                if self.blink
                                    && let Some(shape) = ui.add_shape(blink_shape(), RgbaF32::WHITE)
                                {
                                    shape.animate(blink());
                                }
                            }
                        });
                }
            });
    }
}

/// A caret-like square inside [`BACKDROP`]'s footprint, which [`blink`] turns on and off.
fn blink_shape() -> RectShape {
    Shape::rect().at(Rect::new(60.0, 60.0, 2.0, 10.0))
}

/// A square-wave alpha that flips every 500 ms.
fn blink() -> PaintAnimation {
    PaintAnimation::alpha(0.0, 1.0)
        .with_period(Duration::from_millis(1000))
        .with_steps(2)
        .with_curve(curves::square)
}

/// The key of backdrop `i`: the first row of its chromeless canvas.
fn key(i: usize) -> BackdropKey {
    BackdropKey {
        owner: WidgetId::from_hash(("backdrop", i)),
        shape: 0,
    }
}

/// Two frames of `before` then `after`; the backdrops the second found stale, and its raw rects.
fn second(before: Scene, after: Scene) -> (Vec<BackdropKey>, Vec<Rect>) {
    let mut h = Harness::mono(DISPLAY.physical);
    frame(&mut h, |ui| before.record(ui));
    frame(&mut h, |ui| after.record(ui));
    let dirty = (0..after.backdrops.len())
        .map(key)
        .filter(|&k| h.frames().damage().dirty_backdrops().contains(k))
        .collect();
    (dirty, h.frames().damage().raw_rects().to_vec())
}

const ONE: Scene = Scene {
    under: Some((UNDER, BLUE)),
    backdrops: &[BACKDROP],
    over: BLUE,
    blink: false,
};

/// A change painted before a backdrop, inside its footprint, makes it stale and damages its rect;
/// one after it, inside its rect, does not; one before it but outside its footprint does not.
#[test]
fn damage_before_a_backdrop_inside_its_footprint_makes_it_stale() {
    assert!(footprint(BACKDROP).intersects(UNDER));
    let (dirty, rects) = second(
        ONE,
        Scene {
            under: Some((UNDER, RED)),
            ..ONE
        },
    );
    assert_eq!(dirty, [key(0)]);
    assert!(
        rects.contains(&BACKDROP),
        "the backdrop's rect joins the damage"
    );

    let (dirty, rects) = second(ONE, Scene { over: RED, ..ONE });
    assert!(dirty.is_empty(), "a change above the backdrop leaves it");
    assert!(!rects.contains(&BACKDROP));

    let far = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert!(!footprint(BACKDROP).intersects(far));
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

/// A widget removed from under a backdrop makes it stale: its rows no longer have a place.
#[test]
fn a_removed_widget_under_a_backdrop_makes_it_stale() {
    let (dirty, _) = second(ONE, Scene { under: None, ..ONE });
    assert_eq!(dirty, [key(0)]);
}

/// A backdrop over a backdrop: the lower one's stale rect lies in the upper one's footprint, so one
/// forward walk makes both stale, though the change is outside the upper footprint.
#[test]
fn stacked_backdrops_go_stale_in_one_walk() {
    const STACKED: &[Rect] = &[BACKDROP, Rect::new(70.0, 70.0, 30.0, 30.0)];
    let near = Rect::new(25.0, 25.0, 5.0, 5.0);
    assert!(footprint(BACKDROP).intersects(near));
    assert!(!footprint(STACKED[1]).intersects(near));
    let (dirty, _) = second(
        Scene {
            under: Some((near, BLUE)),
            backdrops: STACKED,
            ..ONE
        },
        Scene {
            under: Some((near, RED)),
            backdrops: STACKED,
            ..ONE
        },
    );
    assert_eq!(dirty, [key(0), key(1)]);
}

/// A caret-like blink after a backdrop, inside it, repaints without recording: the paint-only frame
/// damages the blink's rect and leaves the backdrop.
#[test]
fn a_blink_inside_a_backdrop_leaves_it() {
    let scene = Scene { blink: true, ..ONE };
    let mut h = Harness::mono(DISPLAY.physical);
    frame(&mut h, |ui| scene.record(ui));
    h.advance(Duration::from_millis(600));
    let damage = frame(&mut h, |ui| scene.record(ui));
    assert!(damage.is_some(), "the blink flipped");
    assert!(!h.frames().damage().dirty_backdrops().contains(key(0)));
}

/// A blink that a clip hides, painted before a backdrop, damages nothing when it flips and leaves
/// the backdrop, though the clip clamps its row to an empty rect that stays inside the footprint.
#[test]
fn a_clipped_blink_before_a_backdrop_leaves_it() {
    let record = |ui: &mut Ui| {
        Panel::zstack()
            .id(WidgetId::from_hash("scene"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("clip"))
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .clip_rect()
                    .show(ui, |ui| {
                        if let Some(shape) = ui.add_shape(blink_shape(), RgbaF32::WHITE) {
                            shape.animate(blink());
                        }
                    });
                Panel::canvas()
                    .id(WidgetId::from_hash(("backdrop", 0)))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        ui.add_shape(Shape::rect().at(BACKDROP), Material::blur(SIGMA));
                    });
            });
    };
    let mut h = Harness::mono(DISPLAY.physical);
    frame(&mut h, record);
    h.advance(Duration::from_millis(600));
    let damage = frame(&mut h, record);
    assert_eq!(damage, None, "the hidden blink damages nothing");
    assert!(h.frames().damage().raw_rects().is_empty());
    assert!(!h.frames().damage().dirty_backdrops().contains(key(0)));
}

/// A row removed before a backdrop on its canvas, outside the backdrop's footprint, gives the
/// backdrop a new key, which no capture holds: the backdrop is stale and its rect joins the damage,
/// though the row matcher sees the same backdrop row. The frame after, which changes nothing,
/// leaves it.
#[test]
fn a_backdrop_whose_row_moves_is_stale() {
    let far = Rect::new(0.0, 0.0, 4.0, 4.0);
    assert!(!footprint(BACKDROP).intersects(far));
    let record = |lead: bool| {
        move |ui: &mut Ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("moved"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    if lead {
                        ui.add_shape(Shape::rect().at(far), BLUE);
                    }
                    ui.add_shape(Shape::rect().at(BACKDROP), Material::blur(SIGMA));
                });
        }
    };
    let moved = BackdropKey {
        owner: WidgetId::from_hash("moved"),
        shape: 0,
    };
    let mut h = Harness::mono(DISPLAY.physical);
    frame(&mut h, record(true));
    frame(&mut h, record(false));
    assert!(
        h.frames().damage().dirty_backdrops().contains(moved),
        "moved"
    );
    assert!(h.frames().damage().raw_rects().contains(&BACKDROP));
    frame(&mut h, record(false));
    assert!(
        !h.frames().damage().dirty_backdrops().contains(moved),
        "still"
    );
}

/// The outer backdrop of that scene captures what the child canvas paints under it: a recolor of
/// the child's square, under the outer backdrop's footprint, stales the outer backdrop, which
/// paints after the square though its row comes first. The inner backdrop, painted before the
/// square, stays.
#[test]
fn a_child_canvas_change_under_a_later_ancestor_backdrop_stales_it() {
    const INNER: Rect = Rect::new(20.0, 20.0, 20.0, 20.0);
    const SQUARE: Rect = Rect::new(100.0, 100.0, 10.0, 10.0);
    const OUTER: Rect = Rect::new(90.0, 90.0, 40.0, 40.0);
    let record = |fill: RgbaF32| {
        move |ui: &mut Ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("outer"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::canvas()
                        .id(WidgetId::from_hash("inner"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            ui.add_shape(Shape::rect().at(INNER), Material::blur(SIGMA));
                            ui.add_shape(Shape::rect().at(SQUARE), fill);
                        });
                    ui.add_shape(Shape::rect().at(OUTER), Material::blur(SIGMA));
                });
        }
    };
    let mut h = Harness::mono(DISPLAY.physical);
    frame(&mut h, record(BLUE));
    frame(&mut h, record(BLUE));
    let backdrops = &h.ui().cascade().planes[Plane::MAIN].backdrops;
    let (inner, outer) = (backdrops[0].key, backdrops[1].key);
    frame(&mut h, record(RED));
    let dirty = h.frames().damage().dirty_backdrops();
    assert!(
        dirty.contains(outer),
        "the square under the outer backdrop changed"
    );
    assert!(
        !dirty.contains(inner),
        "the inner backdrop paints before the square"
    );
}

/// A canvas paints a backdrop after a child canvas that paints its own backdrop: the outer backdrop
/// paints last, though its row comes before the child's rows. The walk meets the backdrops out of
/// position order and stays quiet on a frame that changes nothing.
#[test]
fn backdrops_out_of_row_order_walk_quietly() {
    const INNER: Rect = Rect::new(20.0, 20.0, 20.0, 20.0);
    const SQUARE: Rect = Rect::new(100.0, 100.0, 10.0, 10.0);
    const OUTER: Rect = Rect::new(90.0, 90.0, 40.0, 40.0);
    let record = |ui: &mut Ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("outer"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("inner"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        ui.add_shape(Shape::rect().at(INNER), Material::blur(SIGMA));
                        ui.add_shape(Shape::rect().at(SQUARE), BLUE);
                    });
                ui.add_shape(Shape::rect().at(OUTER), Material::blur(SIGMA));
            });
    };
    let mut h = Harness::mono(DISPLAY.physical);
    frame(&mut h, record);
    let backdrops = &h.ui().cascade().planes[Plane::MAIN].backdrops;
    let rows: Vec<u32> = backdrops.iter().map(|backdrop| backdrop.row).collect();
    assert_eq!(rows.len(), 2);
    assert!(
        rows[1] < rows[0],
        "the outer backdrop paints last at a lower row: {rows:?}"
    );
    let outer = backdrops[1].key;
    let inner = backdrops[0].key;

    frame(&mut h, record);
    assert!(
        !h.frames().damage().dirty_backdrops().contains(outer),
        "still"
    );
    assert!(
        !h.frames().damage().dirty_backdrops().contains(inner),
        "still"
    );
}
