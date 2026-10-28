use crate::damage::dirty_panes::DirtyPanes;
use crate::display::Display;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::identity::widget_id::WidgetId;
use crate::renderer::pane_captures::{PaneCaptures, Settled};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::BackdropRow;
use crate::shape::paint::blur_plan::BlurPlan;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(400, 300);

fn key(i: u32) -> PaneKey {
    PaneKey {
        owner: WidgetId::from_hash("pane"),
        shape: i,
    }
}

fn row(i: u32, footprint: URect, sigma: f32) -> BackdropRow {
    BackdropRow {
        key: key(i),
        footprint,
        plan: BlurPlan::new(sigma),
        barrier: i,
        lens: None,
        max_luminance: 65504.0,
        capture: false,
    }
}

/// A frame holding `rows`, every one of them on screen.
fn buffer(rows: &[BackdropRow]) -> RenderBuffer {
    let mut buffer = RenderBuffer::new();
    buffer.display = Display::from_physical(SURFACE, 1.0);
    buffer.backdrops.extend_from_slice(rows);
    buffer.live_panes.extend(rows.iter().map(|row| row.key));
    buffer
}

/// One frame of a sweep: its name, rows, whether it is whole, its dirty set, and the rows that
/// should capture.
type Step<'a> = (&'a str, &'a [BackdropRow], bool, DirtyPanes, &'a [bool]);

/// Settle `rows` on `captures` as a frame would; the rows that capture, and the outcome.
fn settle(
    captures: &mut PaneCaptures,
    rows: &[BackdropRow],
    full: bool,
    dirty: &DirtyPanes,
) -> (Vec<bool>, Settled) {
    let mut frame = buffer(rows);
    let settled = captures.settle(&mut frame, full, dirty);
    (
        frame.backdrops.iter().map(|row| row.capture).collect(),
        settled,
    )
}

/// A pane captures when it is new, dirty, on a whole frame, or when its held capture does not
/// serve it: another plan, or a footprint that leaves the held region. At σ 10 the plan blurs
/// two levels down, a 4 px lattice: a footprint over x 41..139 snaps to texels 10..35, one over
/// 42..140 too, and one over 37..135 starts at texel 9, outside.
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

    let mut captures = PaneCaptures::default();
    let steps: [Step<'_>; 8] = [
        ("new", &[row(0, at(41), 10.0)], false, none, &[true]),
        ("still", &[row(0, at(41), 10.0)], false, none, &[false]),
        ("inside", &[row(0, at(42), 10.0)], false, none, &[false]),
        ("outside", &[row(0, at(37), 10.0)], false, none, &[true]),
        (
            "dirty",
            &[row(0, at(37), 10.0)],
            false,
            [key(0)].into_iter().collect(),
            &[true],
        ),
        ("plan", &[row(0, at(37), 12.0)], false, none, &[true]),
        ("full", &[row(0, at(37), 12.0)], true, none, &[true]),
        (
            "beside",
            &[row(0, at(37), 12.0), row(1, at(200), 12.0)],
            false,
            none,
            &[false, true],
        ),
    ];
    for (what, rows, full, dirty, want) in steps {
        let (got, settled) = settle(&mut captures, rows, full, &dirty);
        assert_eq!(got, want, "{what}");
        assert_eq!(settled, Settled::Fits, "{what}");
    }
}

/// A pane that leaves the screen drops its capture, so it captures when it returns; one only
/// culled from a partial frame's draws stays on screen and keeps it.
#[test]
fn a_pane_off_screen_drops_its_capture() {
    let none = DirtyPanes::default();
    let pane = row(0, URect::new(10, 10, 50, 50), 3.0);
    let mut captures = PaneCaptures::default();
    settle(&mut captures, &[pane], false, &none);

    let mut culled = buffer(&[]);
    culled.live_panes.push(pane.key);
    captures.settle(&mut culled, false, &none);
    assert_eq!(
        settle(&mut captures, &[pane], false, &none).0,
        [false],
        "culled"
    );

    settle(&mut captures, &[], false, &none);
    assert_eq!(
        settle(&mut captures, &[pane], false, &none).0,
        [true],
        "returned"
    );
}

/// A partial frame holds a footprint scissor for at most `PARTIAL_CAP` captures; one more makes
/// it whole, which a whole frame already is.
#[test]
fn more_captures_than_a_partial_frame_holds_overflow() {
    let none = DirtyPanes::default();
    let rows = |n: u32| -> Vec<BackdropRow> {
        (0..n)
            .map(|i| row(i, URect::new(10 * i, 0, 10, 10), 1.0))
            .collect()
    };
    let cap = PaneCaptures::PARTIAL_CAP as u32;
    for (n, full, want) in [
        (cap, false, Settled::Fits),
        (cap + 1, false, Settled::Overflow),
        (cap + 1, true, Settled::Fits),
    ] {
        let (_, settled) = settle(&mut PaneCaptures::default(), &rows(n), full, &none);
        assert_eq!(settled, want, "{n} panes, full {full}");
    }
}
