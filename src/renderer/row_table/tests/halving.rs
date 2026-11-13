//! The table halving at the end of an idle expiry window.

use crate::common::counters::CounterSet;
use crate::renderer::row_table::tests::support::{distinct_grad, fill_rows, fresh_row};
use crate::renderer::row_table::*;

/// A table grown to 512 rows by 300 ramps, of which frames go on to register the last ten alone,
/// halves at the end of the first expiry window that holds none of the 300's own frame: the window
/// starts at epoch 1, the frame that registers all 300, so the first ends at epoch
/// `EXPIRY_FRAMES` with all 300 live, and the second at `2 · EXPIRY_FRAMES` with ten, under a
/// quarter of 512. The ten keep their ramps, renumbered to rows 1 to 10 in the order they last
/// went, and are hits at their new rows; the 290 others go; and the flush after it uploads row 0's
/// magenta and the ten rows' bakes, exactly, as the halved texture starts empty.
#[test]
fn an_idle_atlas_halves_and_keeps_its_live_rows_exact() {
    let mut table = RowTable::default();
    table.begin_frame();
    let rows = fill_rows(&mut table, 300);
    assert_eq!(table.capacity(), 512);
    let _ = table.flush();
    let live: Vec<u32> = (290..300).collect();
    for &i in &live {
        assert_eq!(rows[i as usize].0, i + 1, "claims walk ascending from 1");
    }
    let before = table.counters.counts();
    // Frames until the halving, at most two windows of them.
    for _ in 0..2 * EXPIRY_FRAMES {
        table.begin_frame();
        if table.capacity() != 512 {
            break;
        }
        for &i in &live {
            table.register(&distinct_grad(i).ramp);
        }
        let _ = table.flush();
    }
    assert_eq!(table.epoch, 2 * EXPIRY_FRAMES, "the second window's end");
    assert_eq!(table.capacity(), 256);
    assert_eq!(table.index_len(), live.len());
    for (row, &i) in (1..).zip(&live) {
        assert_eq!(
            table.register(&distinct_grad(i).ramp),
            LutRow(row),
            "ramp {i}"
        );
    }
    let delta = table.counters.counts() - before;
    assert_eq!(delta.halvings, 1);
    assert_eq!(delta.bakes, 0, "every live ramp a hit");

    let f = table.flush().expect("the halved rows flush");
    #[expect(
        clippy::single_range_in_vec_init,
        reason = "a flush of one run is a one-run slice"
    )]
    let runs = [0..11];
    assert_eq!(f.runs, runs);
    let mut want = vec![[RgbaF16::new(1.0, 0.0, 1.0, 1.0); LUT_ROW_TEXELS]];
    for &i in &live {
        let mut row = fresh_row();
        bake::row(&distinct_grad(i).ramp, &mut row);
        want.push(row);
    }
    assert_eq!(f.bytes(0..11), bytemuck::cast_slice::<_, u8>(&want));
    assert!(table.epoch_prefix_holds());
}

/// The quarter is the line: a table of 512 rows whose frames register 128 of them, a quarter,
/// keeps 512 through two windows, and one whose frames register 127 halves. A table at its first
/// size never halves, however idle.
#[test]
fn an_atlas_halves_below_a_quarter_and_never_below_its_first_size() {
    let after_two_windows = |live: u32| {
        let mut table = RowTable::default();
        table.begin_frame();
        let _ = fill_rows(&mut table, 300);
        let _ = table.flush();
        for _ in 0..2 * EXPIRY_FRAMES {
            table.begin_frame();
            let _ = fill_rows(&mut table, live);
            let _ = table.flush();
        }
        table.capacity()
    };
    assert_eq!(after_two_windows(128), 512, "a quarter live");
    assert_eq!(after_two_windows(127), 256, "under a quarter");

    let mut idle = RowTable::default();
    for _ in 0..2 * EXPIRY_FRAMES {
        idle.begin_frame();
        let _ = idle.flush();
    }
    assert_eq!(idle.capacity(), INITIAL_ROWS);
    assert_eq!(idle.counters.counts().halvings, 0);
}
