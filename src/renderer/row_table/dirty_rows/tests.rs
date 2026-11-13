use crate::renderer::row_table::dirty_rows::DirtyRows;

/// Runs come out sorted and exact whatever order rows were marked in; a run crossing a word
/// boundary is one run; a gap of up to `merge_gap` clean rows joins its neighbors, a wider one
/// does not; and taking the runs leaves nothing dirty.
#[test]
#[expect(
    clippy::single_range_in_vec_init,
    reason = "a flush of one run is a one-run slice"
)]
fn runs_are_sorted_merged_across_words_and_small_gaps() {
    let mut dirty = DirtyRows::default();
    dirty.resize(200);
    assert!(dirty.take_runs(0).is_empty());
    for row in [130, 5, 63, 64, 6, 65, 9, 199] {
        dirty.mark(row);
    }
    assert_eq!(
        dirty.take_runs(0),
        [5..7, 9..10, 63..66, 130..131, 199..200]
    );
    assert!(
        dirty.take_runs(0).is_empty(),
        "taking the runs cleans every row"
    );

    for row in [5, 6, 9, 63, 64, 65, 130] {
        dirty.mark(row);
    }
    // Gaps: 7..9 is two rows, 10..63 fifty-three, 66..130 sixty-four.
    assert_eq!(dirty.take_runs(2), [5..10, 63..66, 130..131]);

    (0..70).for_each(|row| dirty.mark(row));
    assert_eq!(dirty.take_runs(0), [0..70]);
    (0..128).for_each(|row| dirty.mark(row));
    assert_eq!(dirty.take_runs(0), [0..128], "a whole number of words");
    assert!(dirty.take_runs(0).is_empty());
}
