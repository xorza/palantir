//! One row's contribution to a pixel diff, and the scan that produces it.

/// A row's `(max delta, differing count)`. Rows are independent, so the
/// per-row parallel reduction is a trivial merge of these.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RowStats {
    pub(super) max_delta: u8,
    pub(super) differing: u32,
}

impl RowStats {
    /// Scan one row: writes each diff pixel into `d_row` (red on miss,
    /// dimmed actual on match) and returns the row's tallies.
    pub(super) fn scan_row(a_row: &[u8], e_row: &[u8], d_row: &mut [u8], per_channel: u8) -> Self {
        let mut stats = Self::default();
        for ((a, e), d) in a_row
            .as_chunks::<4>()
            .0
            .iter()
            .zip(e_row.as_chunks::<4>().0)
            .zip(d_row.as_chunks_mut::<4>().0)
        {
            let delta = (0..4).map(|c| a[c].abs_diff(e[c])).max().unwrap();
            if delta > stats.max_delta {
                stats.max_delta = delta;
            }
            if delta > per_channel {
                stats.differing += 1;
                *d = [255, 0, 0, 255];
            } else {
                d[0] = a[0] / 4;
                d[1] = a[1] / 4;
                d[2] = a[2] / 4;
                d[3] = 255;
            }
        }
        stats
    }

    pub(super) fn merge(self, other: Self) -> Self {
        Self {
            max_delta: self.max_delta.max(other.max_delta),
            differing: self.differing + other.differing,
        }
    }
}
