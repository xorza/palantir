use crate::cascade::pane_row::PaneRow;
use crate::damage::dirty_panes::DirtyPanes;
use crate::display::Display;
use crate::internals::ui_harness::UiHarness;
use crate::internals::ui_harness::frontend_harness::FrontendHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::pane_captures::{PaneCaptures, PaneDamage, PaneTicket, Settled};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::BackdropRow;
use crate::renderer::render_buffer::backdrop_row::PaneSource::{
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

fn key(i: u32) -> PaneKey {
    PaneKey {
        owner: WidgetId::from_hash("pane"),
        shape: i,
    }
}

/// Pane `i` at σ `sigma` whose footprint, at scale 1, is `footprint`.
fn pane(i: u32, footprint: URect, sigma: f32) -> PaneRow {
    let rect = Rect::new(
        footprint.min.x as f32,
        footprint.min.y as f32,
        footprint.size.x as f32,
        footprint.size.y as f32,
    );
    PaneRow {
        key: key(i),
        row: i,
        rect,
        plan: BlurPlan::new(sigma),
        footprint: rect,
    }
}

/// How the composer draws the panes that take a new result: from a capture, from the clear, or
/// not at all, under an opaque cover.
#[derive(Clone, Copy, Debug)]
enum Drawn {
    Captured,
    Cleared,
    Covered,
}

/// One frame of a sweep: its name, its panes, their damage, how the composer draws the capturing
/// ones, and whether each should capture.
type Step<'a> = (&'a str, &'a [PaneRow], PaneDamage<'a>, Drawn, &'a [bool]);

/// One frame of `panes` on `captures`: its tickets, then the rows the composer writes from them,
/// settled. Returns whether each pane captures.
fn frame(
    captures: &mut PaneCaptures,
    panes: &[PaneRow],
    damage: PaneDamage<'_>,
    drawn: Drawn,
) -> Vec<bool> {
    assert_eq!(captures.decide(panes, display(), damage), Settled::Fits);
    let tickets: Vec<PaneTicket> = captures.tickets().iter().flatten().copied().collect();
    let mut buffer = RenderBuffer::new();
    buffer.display = display();
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
                key: ticket.key,
                footprint: ticket.footprint,
                plan: ticket.plan,
                region: ticket.region,
                lens: None,
                max_luminance: 65504.0,
                source,
            }
        }));
    captures.settle(&buffer);
    tickets.iter().map(|ticket| ticket.captures).collect()
}

/// A pane captures when it is new, dirty, on a whole frame, or when its held capture does not
/// serve it: another plan, or a footprint that leaves the held region. At σ 10 the plan blurs
/// two levels down, a 4 px lattice: a footprint over x 41..139 snaps to texels 10..35, one over
/// 42..140 too, and one over 37..135 starts at texel 9, outside. A held pane's ticket names the
/// held region: 40..130, texels 10..33, inside the 9..34 one over 37..135 holds. A pane of the clear holds the clear. A capturing pane drawn under a cover drops
/// what it held, so it captures next; a held one under a cover keeps it.
#[test]
fn a_pane_captures_when_its_held_capture_does_not_serve() {
    let none = DirtyPanes::default();
    let at = |x: u32| URect::new(x, 20, 98, 80);
    let plan = BlurPlan::new(10.0);
    assert_eq!(plan.lattice(), 4);
    assert_eq!(
        plan.region(at(41), SURFACE),
        Some(URect::new(10, 5, 25, 20))
    );
    assert_eq!(plan.region(at(42), SURFACE), plan.region(at(41), SURFACE));
    assert_eq!(plan.region(at(37), SURFACE), Some(URect::new(9, 5, 25, 20)));

    let partial = PaneDamage::Stale(&none);
    let dirty: [DirtyPanes; 2] = [
        [key(0)].into_iter().collect(),
        [key(1)].into_iter().collect(),
    ];
    let mut captures = PaneCaptures::default();
    let steps: [Step<'_>; 15] = [
        (
            "new",
            &[pane(0, at(41), 10.0)],
            partial,
            Drawn::Captured,
            &[true],
        ),
        (
            "still",
            &[pane(0, at(41), 10.0)],
            partial,
            Drawn::Captured,
            &[false],
        ),
        (
            "inside",
            &[pane(0, at(42), 10.0)],
            partial,
            Drawn::Captured,
            &[false],
        ),
        (
            "outside",
            &[pane(0, at(37), 10.0)],
            partial,
            Drawn::Captured,
            &[true],
        ),
        (
            "dirty",
            &[pane(0, at(37), 10.0)],
            PaneDamage::Stale(&dirty[0]),
            Drawn::Captured,
            &[true],
        ),
        (
            "plan",
            &[pane(0, at(37), 12.0)],
            partial,
            Drawn::Captured,
            &[true],
        ),
        (
            "whole",
            &[pane(0, at(37), 12.0)],
            PaneDamage::Whole,
            Drawn::Captured,
            &[true],
        ),
        (
            "beside",
            &[pane(0, at(37), 12.0), pane(1, at(200), 12.0)],
            partial,
            Drawn::Captured,
            &[false, true],
        ),
        (
            "dirty under a cover",
            &[pane(1, at(200), 12.0)],
            PaneDamage::Stale(&dirty[1]),
            Drawn::Covered,
            &[true],
        ),
        (
            "uncovered",
            &[pane(1, at(200), 12.0)],
            partial,
            Drawn::Captured,
            &[true],
        ),
        (
            "held under a cover",
            &[pane(1, at(200), 12.0)],
            partial,
            Drawn::Covered,
            &[false],
        ),
        (
            "held after it",
            &[pane(1, at(200), 12.0)],
            partial,
            Drawn::Captured,
            &[false],
        ),
        (
            "clear",
            &[pane(2, at(300), 10.0)],
            partial,
            Drawn::Cleared,
            &[true],
        ),
        (
            "clear still",
            &[pane(2, at(300), 10.0)],
            partial,
            Drawn::Cleared,
            &[false],
        ),
        (
            "clear whole",
            &[pane(2, at(300), 10.0)],
            PaneDamage::Whole,
            Drawn::Cleared,
            &[true],
        ),
    ];
    for (what, panes, damage, drawn, want) in steps {
        assert_eq!(frame(&mut captures, panes, damage, drawn), want, "{what}");
    }
    frame(
        &mut captures,
        &[pane(3, at(37), 10.0)],
        partial,
        Drawn::Captured,
    );
    captures.decide(
        &[pane(3, URect::new(40, 20, 90, 80), 10.0)],
        display(),
        partial,
    );
    let inside = captures.tickets()[0].expect("on the surface");
    assert!(!inside.captures);
    assert_eq!(
        inside.region,
        plan.region(at(37), SURFACE).unwrap(),
        "held region"
    );
}

/// A pane that leaves the screen drops its capture, so it captures when it returns; one only
/// culled from a partial frame's draws stays on screen and keeps it. A pane whose footprint
/// misses the surface paints nothing and takes no ticket.
#[test]
fn a_pane_off_screen_drops_its_capture() {
    let none = DirtyPanes::default();
    let partial = PaneDamage::Stale(&none);
    let on = pane(0, URect::new(10, 10, 50, 50), 3.0);
    let mut captures = PaneCaptures::default();
    frame(&mut captures, &[on], partial, Drawn::Captured);

    captures.decide(&[on], display(), partial);
    let mut culled = RenderBuffer::new();
    culled.display = display();
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

    let past = pane(1, URect::new(SURFACE.x + 10, 10, 50, 50), 3.0);
    captures.decide(&[on, past], display(), partial);
    assert!(captures.tickets()[1].is_none(), "past the surface");
}

/// A partial frame holds a footprint scissor for at most `PARTIAL_CAP` captures; one more makes
/// it whole, which a whole frame already is, and turns its tickets into a whole frame's. The
/// ledger counts before the composer knows which show the clear, so each capturing pane counts.
#[test]
fn more_captures_than_a_partial_frame_holds_overflow() {
    let none = DirtyPanes::default();
    let panes = |n: u32| -> Vec<PaneRow> {
        (0..n)
            .map(|i| pane(i, URect::new(10 * i, 0, 10, 10), 1.0))
            .collect()
    };
    let cap = PaneCaptures::PARTIAL_CAP as u32;
    for (n, damage, want) in [
        (cap, PaneDamage::Stale(&none), Settled::Fits),
        (cap + 1, PaneDamage::Stale(&none), Settled::Overflow),
        (cap + 1, PaneDamage::Whole, Settled::Fits),
    ] {
        let settled = PaneCaptures::default().decide(&panes(n), display(), damage);
        assert_eq!(settled, want, "{n} panes, {damage:?}");
    }

    // Past the cap the tickets are a whole frame's: a pane whose capture would serve captures too.
    let mut captures = PaneCaptures::default();
    let held = pane(cap + 1, URect::new(200, 100, 10, 10), 1.0);
    frame(
        &mut captures,
        &[held],
        PaneDamage::Stale(&none),
        Drawn::Captured,
    );
    let mut overflowing = panes(cap + 1);
    overflowing.push(held);
    assert_eq!(
        captures.decide(&overflowing, display(), PaneDamage::Stale(&none)),
        Settled::Overflow
    );
    assert!(
        captures
            .tickets()
            .iter()
            .flatten()
            .all(|ticket| ticket.captures),
        "every pane of a whole frame captures"
    );
}

/// A frame's captured area sums the footprints of the panes that capture, an overlap twice, and
/// leaves out a pane that keeps its result or shows the clear: 98 × 80 twice, overlapping, beside
/// a held pane and one of the clear.
#[test]
fn the_captured_area_sums_the_capturing_footprints() {
    let at = |x: u32| URect::new(x, 20, 98, 80);
    let plan = BlurPlan::new(10.0);
    let row = |i: u32, x: u32, source: PaneSource| BackdropRow {
        source,
        ..BackdropRow::fixture(i, at(x), plan)
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

/// A row removed before a pane on its canvas, outside the pane's footprint, gives the pane a new
/// key, which the ledger holds nothing for: the pane captures again what lies under it. A later
/// frame that changes only a square over it holds its capture, so it keeps none of the draws under
/// its footprint the damage misses: the canvas under it is not encoded.
#[test]
fn a_pane_whose_key_moves_takes_a_new_result() {
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
                            ui.add_shape(Shape::rect(square).fill(RgbaF32::WHITE));
                        });
                    Panel::canvas()
                        .id(WidgetId::from_hash("moved"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            if lead {
                                let lead = Rect::new(0.0, 0.0, 4.0, 4.0);
                                ui.add_shape(Shape::rect(lead).fill(RgbaF32::WHITE));
                            }
                            let pane = Rect::new(40.0, 40.0, 40.0, 40.0);
                            ui.add_shape(
                                Shape::rect(pane).backdrop_filter(BackdropFilter::blur(4.0)),
                            );
                            let square = Rect::new(60.0, 60.0, 10.0, 10.0);
                            ui.add_shape(Shape::rect(square).fill(over));
                        });
                });
        }
    };
    let blue = RgbaF32::srgb(0.1, 0.3, 1.0);
    let red = RgbaF32::srgb(1.0, 0.2, 0.1);
    let mut h = FrontendHarness::new(UiHarness::new(UVec2::new(200, 200)));
    let sources = |h: &FrontendHarness| -> Vec<(u32, PaneSource)> {
        h.frontend
            .buffer
            .backdrops
            .iter()
            .map(|row| (row.key.shape, row.source))
            .collect()
    };
    let captured = Capture { barrier: 1 };
    h.frame(record(true, blue));
    assert_eq!(sources(&h), [(1, captured)], "first");
    h.frame(record(false, blue));
    assert_eq!(sources(&h), [(0, captured)], "moved");
    h.frame(record(false, red));
    assert_eq!(sources(&h), [(0, Held)], "still");
    assert_eq!(
        h.frontend.buffer.quads.len(),
        2,
        "the pane and the square over it"
    );
}
