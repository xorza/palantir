//! [`QuadRuns`]: a closing group's quads reordered so each draw's quads lie together.

use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::quad_row::QuadKey;

/// Reorders a group's quads so each joins the last run of its key that lies within
/// [`Self::LOOKBACK`] of the end, where no quad after that run overlaps it: a quad keeps its paint
/// order against every quad it overlaps, and the group draws one run per key where it can. As
/// WebRender joins a primitive to an earlier batch past ones it does not overlap
/// (`batch_lookback_count`). The scratch is retained, so a frame allocates nothing past warmup.
#[derive(Debug, Default)]
pub(super) struct QuadRuns {
    /// The source position of each quad of the group, in its new order.
    order: Vec<u32>,
    /// Each placed quad's key, in `order`'s order: a fill's form, or a shadow, whose entry the
    /// cutout plan picks later.
    keys: Vec<QuadKey>,
    moved: Vec<Quad>,
}

impl QuadRuns {
    /// How many quads back from the group's end a quad looks for its run, as WebRender's
    /// `batch_lookback_count` defaults to.
    pub(super) const LOOKBACK: usize = 10;

    /// Reorder `quads` and their `keys`, one group's, by [`Self`]'s rule. A group of one key, the
    /// common one, is left as it is after one read of each key; a quad of its predecessor's key
    /// appends without a look back, so only a change of key scans.
    pub(super) fn join(&mut self, keys: &mut [QuadKey], quads: &mut [Quad]) {
        assert_eq!(keys.len(), quads.len(), "a key for each quad");
        let Some(&first_key) = keys.first() else {
            return;
        };
        if keys.iter().all(|&key| key == first_key) {
            return;
        }
        self.order.clear();
        self.keys.clear();
        let mut moved = false;
        for (at, (&key, quad)) in keys.iter().zip(&*quads).enumerate() {
            let end = self.keys.len();
            let mut slot = end;
            if self.keys.last().is_some_and(|&last| last != key) {
                let shaded = quad.header.shaded_rect();
                for back in (end.saturating_sub(Self::LOOKBACK)..end).rev() {
                    if self.keys[back] == key {
                        slot = back + 1;
                        break;
                    }
                    let earlier = &quads[self.order[back] as usize].header;
                    if earlier.shaded_rect().intersects(shaded) {
                        break;
                    }
                }
            }
            moved |= slot != end;
            self.order.insert(slot, at as u32);
            self.keys.insert(slot, key);
        }
        if !moved {
            return;
        }
        self.moved.clear();
        self.moved
            .extend(self.order.iter().map(|&from| quads[from as usize]));
        quads.copy_from_slice(&self.moved);
        keys.copy_from_slice(&self.keys);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::composer::quad_runs::QuadRuns;

    impl QuadRuns {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self { order, keys, moved } = self;
            scratch_report::vec_bytes(order)
                + scratch_report::vec_bytes(keys)
                + scratch_report::vec_bytes(moved)
        }
    }
}

#[cfg(test)]
mod tests;
