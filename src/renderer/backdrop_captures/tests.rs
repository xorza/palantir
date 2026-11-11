use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::damage::dirty_backdrops::DirtyBackdrops;
use crate::display::Display;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::renderer::backdrop_captures::{
    BackdropCaptures, BackdropDamage, BackdropTicket, Settled,
};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::BackdropRow;
use crate::renderer::render_buffer::backdrop_row::BackdropSource::{
    self, Capture, Clear, Held, Hidden,
};
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(400, 300);

fn display() -> Display {
    Display::from_physical(SURFACE, 1.0)
}

fn key(i: u32) -> BackdropKey {
    BackdropKey {
        owner: WidgetId::from_hash("backdrop"),
        shape: i,
    }
}

/// Backdrop `i` at σ `sigma` whose footprint, at scale 1, is `footprint`.
fn backdrop(i: u32, footprint: URect, sigma: f32) -> PlaneBackdrop {
    let rect = Rect::new(
        footprint.min.x as f32,
        footprint.min.y as f32,
        footprint.size.x as f32,
        footprint.size.y as f32,
    );
    PlaneBackdrop {
        key: key(i),
        row: i,
        paint_row: i,
        rect,
        plan: BlurPlan::new(sigma),
        footprint: rect,
    }
}

/// How the composer draws the backdrops that take a new result: from a capture, from the clear, or
/// not at all, under an opaque cover.
#[derive(Clone, Copy, Debug)]
enum Drawn {
    Captured,
    Cleared,
    Covered,
}

/// One frame of a sweep: its name, its backdrops, their damage, how the composer draws the
/// capturing ones, and whether each should capture.
type Step<'a> = (
    &'a str,
    &'a [PlaneBackdrop],
    BackdropDamage<'a>,
    Drawn,
    &'a [bool],
);

/// One frame of `backdrops` on `captures`: its tickets, then the rows the composer writes from
/// them, settled. Returns whether each backdrop captures.
fn frame(
    captures: &mut BackdropCaptures,
    backdrops: &[PlaneBackdrop],
    damage: BackdropDamage<'_>,
    drawn: Drawn,
) -> Vec<bool> {
    assert_eq!(captures.decide(backdrops, display(), damage), Settled::Fits);
    let tickets: Vec<BackdropTicket> = captures.tickets().iter().flatten().copied().collect();
    let mut buffer = RenderBuffer::new();
    buffer.display = display();
    buffer
        .live_backdrops
        .refill(backdrops.iter().map(|backdrop| backdrop.key));
    buffer
        .backdrops
        .extend(tickets.iter().enumerate().map(|(i, ticket)| {
            let source = match (ticket.captures, drawn) {
                (false, _) => Held,
                (true, Drawn::Captured) => Capture {
                    barrier: i as u32 + 1,
                },
                (true, Drawn::Cleared) => Clear,
                (true, Drawn::Covered) => Hidden,
            };
            BackdropRow {
                reads: ticket.capture.footprint.into(),
                look: None,
                capture: ticket.capture,
                source,
            }
        }));
    captures.settle(&buffer);
    tickets.iter().map(|ticket| ticket.captures).collect()
}

/// A backdrop captures when it is new, dirty, on a whole frame, or when its held capture does not
/// serve it: another plan, or a footprint that leaves the held region. At σ 10 the plan blurs two
/// levels down, a 4 px lattice: a footprint over x 41..139 snaps to texels 10..35, one over 42..140
/// too, and one over 37..135 starts at texel 9, outside. A held backdrop's ticket names the held
/// region: 40..130, texels 10..33, inside the 9..34 one over 37..135 holds. A backdrop of the clear
/// holds the clear. A capturing backdrop drawn under a cover drops what it held, so it captures
/// next; a held one under a cover keeps it.
#[test]
fn a_backdrop_captures_when_its_held_capture_does_not_serve() {
    let none = DirtyBackdrops::default();
    let at = |x: u32| URect::new(x, 20, 98, 80);
    let plan = BlurPlan::new(10.0);
    assert_eq!(plan.lattice(), 4);
    assert_eq!(
        plan.region(at(41), SURFACE),
        Some(URect::new(10, 5, 25, 20))
    );
    assert_eq!(plan.region(at(42), SURFACE), plan.region(at(41), SURFACE));
    assert_eq!(plan.region(at(37), SURFACE), Some(URect::new(9, 5, 25, 20)));

    let partial = BackdropDamage::Stale(&none);
    let dirty: [DirtyBackdrops; 2] = [
        [key(0)].into_iter().collect(),
        [key(1)].into_iter().collect(),
    ];
    let mut captures = BackdropCaptures::default();
    let steps: [Step<'_>; 15] = [
        (
            "new",
            &[backdrop(0, at(41), 10.0)],
            partial,
            Drawn::Captured,
            &[true],
        ),
        (
            "still",
            &[backdrop(0, at(41), 10.0)],
            partial,
            Drawn::Captured,
            &[false],
        ),
        (
            "inside",
            &[backdrop(0, at(42), 10.0)],
            partial,
            Drawn::Captured,
            &[false],
        ),
        (
            "outside",
            &[backdrop(0, at(37), 10.0)],
            partial,
            Drawn::Captured,
            &[true],
        ),
        (
            "dirty",
            &[backdrop(0, at(37), 10.0)],
            BackdropDamage::Stale(&dirty[0]),
            Drawn::Captured,
            &[true],
        ),
        (
            "plan",
            &[backdrop(0, at(37), 12.0)],
            partial,
            Drawn::Captured,
            &[true],
        ),
        (
            "whole",
            &[backdrop(0, at(37), 12.0)],
            BackdropDamage::Whole,
            Drawn::Captured,
            &[true],
        ),
        (
            "beside",
            &[backdrop(0, at(37), 12.0), backdrop(1, at(200), 12.0)],
            partial,
            Drawn::Captured,
            &[false, true],
        ),
        (
            "dirty under a cover",
            &[backdrop(1, at(200), 12.0)],
            BackdropDamage::Stale(&dirty[1]),
            Drawn::Covered,
            &[true],
        ),
        (
            "uncovered",
            &[backdrop(1, at(200), 12.0)],
            partial,
            Drawn::Captured,
            &[true],
        ),
        (
            "held under a cover",
            &[backdrop(1, at(200), 12.0)],
            partial,
            Drawn::Covered,
            &[false],
        ),
        (
            "held after it",
            &[backdrop(1, at(200), 12.0)],
            partial,
            Drawn::Captured,
            &[false],
        ),
        (
            "clear",
            &[backdrop(2, at(300), 10.0)],
            partial,
            Drawn::Cleared,
            &[true],
        ),
        (
            "clear still",
            &[backdrop(2, at(300), 10.0)],
            partial,
            Drawn::Cleared,
            &[false],
        ),
        (
            "clear whole",
            &[backdrop(2, at(300), 10.0)],
            BackdropDamage::Whole,
            Drawn::Cleared,
            &[true],
        ),
    ];
    for (what, backdrops, damage, drawn, want) in steps {
        assert_eq!(
            frame(&mut captures, backdrops, damage, drawn),
            want,
            "{what}"
        );
    }
    frame(
        &mut captures,
        &[backdrop(3, at(37), 10.0)],
        partial,
        Drawn::Captured,
    );
    captures.decide(
        &[backdrop(3, URect::new(40, 20, 90, 80), 10.0)],
        display(),
        partial,
    );
    let inside = captures.tickets()[0].expect("on the surface");
    assert!(!inside.captures);
    assert_eq!(
        inside.capture.region,
        plan.region(at(37), SURFACE).unwrap(),
        "held region"
    );
}

/// A backdrop that leaves the screen drops its capture, so it captures when it returns; one only
/// culled from a partial frame's draws stays on screen and keeps it. A backdrop whose footprint
/// misses the surface paints nothing and takes no ticket.
#[test]
fn a_backdrop_off_screen_drops_its_capture() {
    let none = DirtyBackdrops::default();
    let partial = BackdropDamage::Stale(&none);
    let on = backdrop(0, URect::new(10, 10, 50, 50), 3.0);
    let mut captures = BackdropCaptures::default();
    frame(&mut captures, &[on], partial, Drawn::Captured);

    captures.decide(&[on], display(), partial);
    // The frame's scene still holds the backdrop, which its draws leave out.
    let mut culled = RenderBuffer::new();
    culled.display = display();
    culled.live_backdrops.refill([on.key]);
    captures.settle(&culled);
    assert_eq!(
        frame(&mut captures, &[on], partial, Drawn::Captured),
        [false],
        "culled"
    );

    frame(&mut captures, &[], partial, Drawn::Captured);
    assert_eq!(
        frame(&mut captures, &[on], partial, Drawn::Captured),
        [true],
        "returned"
    );

    let past = backdrop(1, URect::new(SURFACE.x + 10, 10, 50, 50), 3.0);
    captures.decide(&[on, past], display(), partial);
    assert!(captures.tickets()[1].is_none(), "past the surface");
}

/// A partial frame holds a footprint scissor for at most `PARTIAL_CAP` captures; one more makes
/// it whole, which a whole frame already is. The ledger counts before the composer knows which
/// show the clear, so each capturing backdrop counts.
#[test]
fn more_captures_than_a_partial_frame_holds_overflow() {
    let none = DirtyBackdrops::default();
    let backdrops = |n: u32| -> Vec<PlaneBackdrop> {
        (0..n)
            .map(|i| backdrop(i, URect::new(10 * i, 0, 10, 10), 1.0))
            .collect()
    };
    let cap = BackdropCaptures::PARTIAL_CAP as u32;
    for (n, damage, want) in [
        (cap, BackdropDamage::Stale(&none), Settled::Fits),
        (cap + 1, BackdropDamage::Stale(&none), Settled::Overflow),
        (cap + 1, BackdropDamage::Whole, Settled::Fits),
    ] {
        let settled = BackdropCaptures::default().decide(&backdrops(n), display(), damage);
        assert_eq!(settled, want, "{n} backdrops, {damage:?}");
    }

    // Past the cap the caller decides again, whole: a backdrop whose capture would serve captures
    // too.
    let mut captures = BackdropCaptures::default();
    let held = backdrop(cap + 1, URect::new(200, 100, 10, 10), 1.0);
    frame(
        &mut captures,
        &[held],
        BackdropDamage::Stale(&none),
        Drawn::Captured,
    );
    let mut overflowing = backdrops(cap + 1);
    overflowing.push(held);
    assert_eq!(
        captures.decide(&overflowing, display(), BackdropDamage::Stale(&none)),
        Settled::Overflow
    );
    assert_eq!(
        captures.decide(&overflowing, display(), BackdropDamage::Whole),
        Settled::Fits
    );
    assert!(
        captures
            .tickets()
            .iter()
            .flatten()
            .all(|ticket| ticket.captures),
        "every backdrop of a whole frame captures"
    );
}

/// A frame's captured area sums the footprints of the backdrops that capture, an overlap twice, and
/// leaves out a backdrop that keeps its result or shows the clear: 98 × 80 twice, overlapping,
/// beside a held backdrop and one of the clear.
#[test]
fn the_captured_area_sums_the_capturing_footprints() {
    let at = |x: u32| URect::new(x, 20, 98, 80);
    let plan = BlurPlan::new(10.0);
    let row = |i: u32, x: u32, source: BackdropSource| {
        BackdropRow::fixture(i, at(x), plan).with_source(source)
    };
    let mut buffer = RenderBuffer::new();
    buffer.backdrops.extend([
        row(0, 10, Capture { barrier: 1 }),
        row(1, 50, Capture { barrier: 1 }),
        row(2, 200, Held),
        row(3, 300, Clear),
    ]);
    assert_eq!(buffer.captured_area(), 2 * 98 * 80);
}

/// A row removed before a backdrop on its canvas, outside the backdrop's footprint, gives the
/// backdrop a new key, which the ledger holds nothing for: the backdrop captures again what lies
/// under it. A later frame that changes only a square over it holds its capture, so it keeps none
/// of the draws under its footprint the damage misses: the canvas under it is not encoded.
#[test]
fn a_backdrop_whose_key_moves_takes_a_new_result() {
    let record = |lead: bool, over: RgbaF32| {
        move |ui: &mut Ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("scene"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::canvas()
                        .id(WidgetId::from_hash("under"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            let square = Rect::new(42.0, 42.0, 4.0, 4.0);
                            ui.add_shape(Shape::rect().at(square), RgbaF32::WHITE);
                        });
                    Panel::canvas()
                        .id(WidgetId::from_hash("moved"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            if lead {
                                let lead = Rect::new(0.0, 0.0, 4.0, 4.0);
                                ui.add_shape(Shape::rect().at(lead), RgbaF32::WHITE);
                            }
                            let backdrop = Rect::new(40.0, 40.0, 40.0, 40.0);
                            ui.add_shape(Shape::rect().at(backdrop), Material::blur(4.0));
                            let square = Rect::new(60.0, 60.0, 10.0, 10.0);
                            ui.add_shape(Shape::rect().at(square), over);
                        });
                });
        }
    };
    let blue = RgbaF32::srgb(0.1, 0.3, 1.0);
    let red = RgbaF32::srgb(1.0, 0.2, 0.1);
    let mut h = Harness::builder(UVec2::new(200, 200))
        .mono()
        .depth(Depth::Compose)
        .build();
    let sources = |h: &Harness| -> Vec<(u32, BackdropSource)> {
        h.render_buffer()
            .backdrops
            .iter()
            .map(|row| (row.capture.key.shape, row.source))
            .collect()
    };
    let captured = Capture { barrier: 1 };
    h.frame(record(true, blue));
    assert_eq!(sources(&h), [(1, captured)], "first");
    h.frame(record(false, blue));
    assert_eq!(sources(&h), [(0, captured)], "moved");
    h.frame(record(false, red));
    assert_eq!(sources(&h), [(0, Held)], "still");
    assert_eq!(h.render_buffer().effects.len(), 1, "the backdrop");
    assert_eq!(h.render_buffer().quads.len(), 1, "the square over it");
}
