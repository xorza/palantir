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
    /// Track `rows` rows; rows past the old count start clean, and none past the new is dirty.
    pub(super) fn resize(&mut self, rows: u32) {
        let words = rows.div_ceil(u64::BITS) as usize;
        debug_assert!(
            self.bits.iter().skip(words).all(|&word| word == 0),
            "a row past the new count is dirty"
        );
        self.bits.resize(words, 0);
    }

    pub(super) fn mark(&mut self, row: u32) {
        self.bits[(row / u64::BITS) as usize] |= 1 << (row % u64::BITS);
    }

    /// Mark `row` clean, as no flush owes a row that holds no ramp.
    pub(super) fn clear(&mut self, row: u32) {
        self.bits[(row / u64::BITS) as usize] &= !(1 << (row % u64::BITS));
    }

    /// Give back what the bits and the runs hold past their need.
    pub(super) fn shrink_to_fit(&mut self) {
        self.bits.shrink_to_fit();
        self.runs.shrink_to_fit();
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
mod tests;
