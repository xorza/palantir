use crate::damage::Damage;
use crate::damage::region::DamageRegion;
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::{BackdropRow, PaneSource};
use crate::renderer::repaint_scissors::RepaintScissors;
use crate::shape::paint::blur_plan::BlurPlan;
use glam::UVec2;
use std::time::Duration;

fn buffer() -> RenderBuffer {
    let mut buffer = RenderBuffer::new();
    buffer.start_frame(FrameStamp::new(
        Display::from_physical(UVec2::new(100, 100), 2.0),
        Duration::ZERO,
    ));
    buffer
}

#[test]
fn full_repaint_has_no_partial_scissors() {
    let repaint = RepaintScissors::new(Damage::Full, &buffer());
    assert!(matches!(repaint, Some(RepaintScissors::Full)));
}

/// Eight capturing footprints, bands across eight damage strips, which they split into eight
/// pieces apiece: 8 + 8 × 8 = 72 scissors, past the cap of 40, so no partial repaint fits.
#[test]
fn scissors_past_the_cap_fit_no_partial_repaint() {
    let strips: Vec<Rect> = (0..8)
        .map(|i| Rect::new(1.0 + 6.0 * i as f32, 0.0, 1.0, 50.0))
        .collect();
    let damage = DamageRegion::collapse_from(&strips, 0.0, Rect::new(0.0, 0.0, 50.0, 50.0));
    assert_eq!(damage.region.iter_rects().count(), 8);
    let mut buffer = buffer();
    buffer.backdrops.extend((0..8).map(|j| BackdropRow {
        source: PaneSource::Capture { barrier: j + 1 },
        ..BackdropRow::fixture(j, URect::new(0, 12 * j, 100, 6), BlurPlan::new(2.0))
    }));
    assert!(RepaintScissors::new(Damage::Partial(damage), &buffer).is_none());
}

/// A partial repaint takes the footprint of each pane that captures, as the target there holds
/// the last frame's pane, then each damage rect, scaled, padded and clamped, less what the
/// footprints already cover: no pixel lies under two scissors, and damage inside a footprint
/// adds no piece. A pane that keeps its result or shows the clear reads no pixel of the target,
/// so its footprint adds none.
#[test]
fn partial_repaint_preserves_padded_physical_scissors() {
    let damage = DamageRegion::collapse_from(
        &[
            Rect::new(5.0, 5.0, 5.0, 5.0),
            Rect::new(30.0, 20.0, 10.0, 5.0),
            Rect::new(20.0, 18.0, 4.0, 4.0),
        ],
        0.0,
        Rect::new(0.0, 0.0, 50.0, 50.0),
    );
    // Three panes, one capturing: its footprint, already physical, joins the scissors first.
    let mut buffer = buffer();
    let pane = |shape, footprint, source| BackdropRow {
        source,
        ..BackdropRow::fixture(shape, footprint, BlurPlan::new(2.0))
    };
    buffer.backdrops.extend([
        pane(0, URect::new(0, 80, 20, 20), PaneSource::Clear),
        pane(
            1,
            URect::new(30, 30, 40, 20),
            PaneSource::Capture { barrier: 1 },
        ),
        pane(2, URect::new(0, 60, 20, 20), PaneSource::Held),
    ]);
    let repaint = RepaintScissors::new(Damage::Partial(damage), &buffer);
    let Some(RepaintScissors::Partial(rects)) = repaint else {
        panic!("partial plan produced a full repaint");
    };
    // At 2x the rects are (10,10)-(20,20), (60,40)-(80,50) and (40,36)-(48,44); the 2px AA pad
    // extends each edge. The footprint, (30,30)-(70,50), holds the third whole, and of the
    // second leaves the band below it, (58,50)-(82,52), and the part right of it,
    // (70,38)-(82,50).
    assert_eq!(
        rects.iter().collect::<Vec<_>>(),
        [
            URect::new(30, 30, 40, 20),
            URect::new(8, 8, 14, 14),
            URect::new(58, 50, 24, 2),
            URect::new(70, 38, 12, 12),
        ]
    );
    // Their bounds: x from 8 to 82, y from 8 to 52.
    assert_eq!(rects.bounds(), URect::new(8, 8, 74, 44));
    assert!(matches!(
        RepaintScissors::new(Damage::Full, &buffer),
        Some(RepaintScissors::Full)
    ));
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "probes a debug_assert!, which release compiles out"
)]
#[should_panic(expected = "Partial plan produced no damage scissors")]
fn partial_repaint_rejects_scissors_clamped_outside_viewport() {
    RepaintScissors::new(
        Damage::Partial(DamageRegion::from(Rect::new(200.0, 200.0, 10.0, 10.0)).unmeasured()),
        &buffer(),
    );
}
