//! The rows a flush owes the GPU, kept as a bitset and handed out as sorted runs.

use std::ops::Range;

/// Rows changed since the last flush. A bitset, so marking in any order costs one store and the
/// runs come out sorted; [`Self::take_runs`] turns it into the copies one upload makes.
#[derive(Debug, Default)]
pub(super) struct DirtyRows {
    /// One bit per row, 64 rows a word.
    bits: Vec<u64>,
    /// [`Self::take_runs`]'s output, retained so a flush allocates nothing.
    runs: Vec<Range<u32>>,
}

impl DirtyRows {
    /// Track `rows` rows; rows past the old count start clean.
    pub(super) fn resize(&mut self, rows: u32) {
        self.bits.resize(rows.div_ceil(u64::BITS) as usize, 0);
    }

    pub(super) fn mark(&mut self, row: u32) {
        self.bits[(row / u64::BITS) as usize] |= 1 << (row % u64::BITS);
    }

    /// The dirty rows as sorted runs, a gap of up to `merge_gap` clean rows folded into the run
    /// around it, and every row clean again. An empty slice when nothing was dirty.
    pub(super) fn take_runs(&mut self, merge_gap: u32) -> &[Range<u32>] {
        self.runs.clear();
        for (word_idx, word) in self.bits.iter_mut().enumerate() {
            let mut bits = std::mem::take(word);
            let base = word_idx as u32 * u64::BITS;
            while bits != 0 {
                let start = base + bits.trailing_zeros();
                // The run of ones from the lowest set bit, to the word's end at most.
                let ones = (bits >> bits.trailing_zeros()).trailing_ones();
                let end = start + ones;
                bits &= u64::MAX.checked_shl(end - base).unwrap_or(0);
                match self.runs.last_mut() {
                    Some(last) if start - last.end <= merge_gap => last.end = end,
                    _ => self.runs.push(start..end),
                }
            }
        }
        &self.runs
    }
}

#[cfg(test)]
mod tests {
    use crate::renderer::gradient_atlas::dirty_rows::DirtyRows;

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
}
