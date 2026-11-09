//! [`QuadRuns`]: a closing group's quads reordered so each draw's quads lie together.

use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::quad_form::QuadForm;

/// What a quad's run is keyed by: its form, or a shadow, whose entry the cutout plan picks later.
fn run_key(quad: &Quad) -> Option<QuadForm> {
    (!quad.fill_kind.is_shadow()).then(|| QuadForm::of(quad.fill_kind))
}

/// Reorders a group's quads so each joins the last run of its key that lies within
/// [`Self::LOOKBACK`] of the end, where no quad after that run overlaps it: a quad keeps its paint
/// order against every quad it overlaps, and the group draws one run per key where it can. As
/// WebRender joins a primitive to an earlier batch past ones it does not overlap
/// (`batch_lookback_count`). The scratch is retained, so a frame allocates nothing past warmup.
#[derive(Debug, Default)]
pub(super) struct QuadRuns {
    /// The source position of each quad of the group, in its new order.
    order: Vec<u32>,
    /// Each placed quad's key, in `order`'s order.
    keys: Vec<Option<QuadForm>>,
    moved: Vec<Quad>,
}

impl QuadRuns {
    /// How many quads back from the group's end a quad looks for its run, as WebRender's
    /// `batch_lookback_count` defaults to.
    pub(super) const LOOKBACK: usize = 10;

    /// Reorder `quads`, one group's, by [`Self`]'s rule. A group of one key, the common one, is
    /// left as it is after one read of each quad's key; a quad of its predecessor's key appends
    /// without a look back, so only a change of key scans.
    pub(super) fn join(&mut self, quads: &mut [Quad]) {
        let Some(first) = quads.first() else {
            return;
        };
        let first_key = run_key(first);
        if quads.iter().all(|quad| run_key(quad) == first_key) {
            return;
        }
        self.order.clear();
        self.keys.clear();
        let mut moved = false;
        for (at, quad) in quads.iter().enumerate() {
            let key = run_key(quad);
            let end = self.keys.len();
            let mut slot = end;
            if self.keys.last().is_some_and(|&last| last != key) {
                let shaded = quad.shaded_rect();
                for back in (end.saturating_sub(Self::LOOKBACK)..end).rev() {
                    if self.keys[back] == key {
                        slot = back + 1;
                        break;
                    }
                    if quads[self.order[back] as usize]
                        .shaded_rect()
                        .intersects(shaded)
                    {
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
