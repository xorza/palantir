use crate::scene::tree::paint_anims::*;
use crate::shape::paint_animation::{PaintRepeat, curves};
use std::f32::consts::TAU;

const START: Duration = Duration::from_secs(1);

fn spinning(shape_idx: u32, speed: f32) -> PaintAnimEntry {
    PaintAnimEntry {
        anim: PaintAnimation::turn(0.0, 1.0)
            .with_started_at(START)
            .with_period(Duration::from_secs_f32(TAU / speed))
            .with_repeat(PaintRepeat::Forever)
            .with_curve(curves::linear),
        shape_idx,
        row: 0,
        node: NodeId(0),
    }
}

#[test]
fn sparse_cursor_samples_boundaries_and_advances_across_skipped_animations() {
    const LAST_SHAPE: u32 = 1_000_000;
    let mut anims = PaintAnims::default();
    anims.push_entry(spinning(0, 1.0));
    anims.push_entry(spinning(5, 2.0));
    anims.push_entry(spinning(10, 3.0));
    anims.push_entry(spinning(LAST_SHAPE, 4.0));

    let registered: Vec<u32> = anims.entries.iter().map(|entry| entry.shape_idx).collect();
    assert_eq!(registered, [0, 5, 10, LAST_SHAPE]);

    let now = START + Duration::from_secs(1);
    let mut cursor = anims.cursor();
    assert_eq!(cursor.sample(0, now).rotation, 1.0);
    assert_eq!(cursor.sample(1, now), PaintMod::IDENTITY);
    assert_eq!(cursor.sample(5, now).rotation, 2.0);
    assert_eq!(
        cursor.sample(LAST_SHAPE, now).rotation,
        4.0,
        "jumping over culled shape 10 must not strand the cursor",
    );

    // A jump between two registrations: culling skipped shape 5, and shape 6 sits
    // below the next registered index. It owns no animation; taking 10's would
    // misparent it and leave shape 10 unanimated.
    let mut cursor = anims.cursor();
    assert_eq!(cursor.sample(0, now).rotation, 1.0);
    assert_eq!(
        cursor.sample(6, now),
        PaintMod::IDENTITY,
        "a shape between registrations owns no animation",
    );
    assert_eq!(
        cursor.sample(10, now).rotation,
        3.0,
        "the overshot registration must still be there for its own shape",
    );

    let entry_capacity = anims.entries.capacity();
    anims.clear();
    assert!(anims.entries.is_empty());
    assert_eq!(anims.entries.capacity(), entry_capacity);
}

/// The reading half of the ordering contract `push_entry` asserts. Debug-only:
/// the check needs a field the release cursor lacks.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "must be monotonic")]
fn sampling_backwards_is_a_caller_bug() {
    let mut anims = PaintAnims::default();
    anims.push_entry(spinning(2, 1.0));
    let now = START + Duration::from_secs(1);
    let mut cursor = anims.cursor();
    cursor.sample(4, now);
    cursor.sample(3, now);
}
