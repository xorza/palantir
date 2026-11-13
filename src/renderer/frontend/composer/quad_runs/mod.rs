//! [`QuadRuns`]: a closing group's quads reordered so each draw's quads lie together.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::quad_draw_row::{QuadInstance, QuadKey};

/// What a quad joins a run by: the pipeline it draws through and the scissor of its run.
type RunKey = (QuadKey, Option<URect>);

/// Reorders a group's quads so each joins the last run of its key and scissor that lies within
/// [`Self::LOOKBACK`] quads of the end, where no quad after that run overlaps it: a quad keeps its
/// paint order against every quad it overlaps, and the group draws one run per key where it can.
/// As WebRender joins a primitive to an earlier batch past ones it does not overlap
/// (`batch_lookback_count`). The runs link their quads, so a quad joins one in constant time
/// however large the group. The scratch is retained, so a frame allocates nothing past warmup.
#[derive(Debug, Default)]
pub(super) struct QuadRuns {
    /// The runs in order, each a chain of the group's quads.
    runs: Vec<Run>,
    /// Each quad's predecessor in its run, by source position, [`NONE`] for a run's first.
    prev: Vec<u32>,
    /// Each quad's successor in its run, by source position, [`NONE`] for a run's last.
    next: Vec<u32>,
    keys: Vec<QuadKey>,
    scissors: Vec<Option<URect>>,
    moved: Vec<QuadInstance>,
    /// What each quad of the group shades, by source position, read once a join.
    shaded: Vec<Rect>,
    /// The clips that cut a quad of the group, whose every quad draws under them, sorted.
    cutting: Vec<URect>,
    /// Whether a run opened under each of [`Self::cutting`], by its position: a quad of a clip no
    /// run is under yet opens one without a look back.
    cutting_opened: Vec<bool>,
}

/// One run of a group: what joins it, and its first and last quad by source position.
#[derive(Clone, Copy, Debug)]
struct Run {
    key: RunKey,
    first: u32,
    last: u32,
}

/// The end of a run's chain.
const NONE: u32 = u32::MAX;

/// How [`URect`]s sort in [`QuadRuns::cutting`].
const fn order_key(rect: &URect) -> (u32, u32, u32, u32) {
    (rect.min.x, rect.min.y, rect.size.x, rect.size.y)
}

impl QuadRuns {
    /// How many quads back from the group's end a quad looks for its run, as WebRender's
    /// `batch_lookback_count` defaults to.
    pub(super) const LOOKBACK: usize = 10;

    /// Reorder `quads`, their `keys` and their `scissors`, one group's, by [`Self`]'s rule. A
    /// quad's scissor comes in as the clip in force where it was drawn and leaves as its run's: a
    /// run of its key takes it where the run's scissor is that clip, or, for a quad under a clip
    /// that cuts no quad of the group, where the run's scissor holds what the quad shades; a quad
    /// no run takes opens one under its clip where that cuts a quad of the group, and under none
    /// otherwise. So the quads inside a scroll whose edge cuts some join one run of each form, as
    /// one scissor must draw them anyway, and a field's, which its clip cuts none of, join the
    /// runs around it under no clip. A group of
    /// one key and scissor, the common one, is left as it is after one read of each; a quad the
    /// last run takes appends without a look back, so only a change scans.
    pub(super) fn join(
        &mut self,
        keys: &mut [QuadKey],
        scissors: &mut [Option<URect>],
        quads: &mut [QuadInstance],
    ) {
        assert_eq!(keys.len(), quads.len(), "a key for each quad");
        assert_eq!(scissors.len(), quads.len(), "a scissor for each quad");
        let Some(first) = keys.first().map(|&key| (key, scissors[0])) else {
            return;
        };
        if keys
            .iter()
            .zip(&*scissors)
            .all(|(&key, &scissor)| (key, scissor) == first)
        {
            return;
        }
        self.shaded.clear();
        self.shaded
            .extend(quads.iter().map(|quad| quad.header.shaded_rect()));
        self.cutting.clear();
        self.cutting.extend(
            self.shaded
                .iter()
                .zip(&*scissors)
                .filter_map(|(&shaded, &clip)| {
                    clip.filter(|clip| !Rect::from(*clip).contains_rect(shaded))
                }),
        );
        self.cutting.sort_unstable_by_key(order_key);
        self.cutting.dedup();
        self.cutting_opened.clear();
        self.cutting_opened.resize(self.cutting.len(), false);
        self.runs.clear();
        self.prev.clear();
        self.prev.resize(quads.len(), NONE);
        self.next.clear();
        self.next.resize(quads.len(), NONE);
        // Whether a quad joined a run before the last, which reorders the group; a scissor traded
        // for a run's is written in place.
        let mut moved = false;
        for at in 0..quads.len() {
            let (key, clip, shaded) = (keys[at], scissors[at], self.shaded[at]);
            let holds = |scissor: Option<URect>| {
                scissor.is_none_or(|scissor| Rect::from(scissor).contains_rect(shaded))
            };
            // A clip that cuts a quad of the group holds every quad under it; any other leaves its
            // quads free to join a run whose scissor holds them.
            let cutting = clip.and_then(|clip| {
                self.cutting
                    .binary_search_by_key(&order_key(&clip), order_key)
                    .ok()
            });
            let free = cutting.is_none();
            let takes = |&(run_key, run_scissor): &RunKey| {
                run_key == key && (run_scissor == clip || (free && holds(run_scissor)))
            };
            let joins = match cutting {
                Some(at) if !self.cutting_opened[at] => None,
                _ => self.joining(shaded, takes),
            };
            let at = at as u32;
            if let Some(joined) = joins {
                moved |= joined + 1 != self.runs.len();
                let run = &mut self.runs[joined];
                self.next[run.last as usize] = at;
                self.prev[at as usize] = run.last;
                run.last = at;
                scissors[at as usize] = run.key.1;
            } else {
                let scissor = if free { None } else { clip };
                scissors[at as usize] = scissor;
                self.runs.push(Run {
                    key: (key, scissor),
                    first: at,
                    last: at,
                });
                if let Some(at) = cutting {
                    self.cutting_opened[at] = true;
                }
            }
        }
        if !moved {
            return;
        }
        self.moved.clear();
        self.keys.clear();
        self.scissors.clear();
        for run in &self.runs {
            let mut at = run.first;
            while at != NONE {
                self.moved.push(quads[at as usize]);
                self.keys.push(run.key.0);
                self.scissors.push(run.key.1);
                at = self.next[at as usize];
            }
        }
        quads.copy_from_slice(&self.moved);
        keys.copy_from_slice(&self.keys);
        scissors.copy_from_slice(&self.scissors);
    }

    /// The run a quad shading `shaded` joins, as `takes` says, looking back [`Self::LOOKBACK`]
    /// quads from the end of the runs and no further, and not past one it overlaps.
    fn joining(&self, shaded: Rect, takes: impl Fn(&RunKey) -> bool) -> Option<usize> {
        let last = self.runs.len().checked_sub(1)?;
        if takes(&self.runs[last].key) {
            return Some(last);
        }
        let mut looked = 0;
        for at in (0..self.runs.len()).rev() {
            let run = self.runs[at];
            // The run's last quad is the next one back, which must lie within the look back.
            if looked == Self::LOOKBACK {
                return None;
            }
            if takes(&run.key) {
                return Some(at);
            }
            let mut member = run.last;
            while member != NONE {
                if looked == Self::LOOKBACK || self.shaded[member as usize].intersects(shaded) {
                    return None;
                }
                looked += 1;
                member = self.prev[member as usize];
            }
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::composer::quad_runs::QuadRuns;

    impl QuadRuns {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                runs,
                prev,
                next,
                keys,
                scissors,
                moved,
                shaded,
                cutting,
                cutting_opened,
            } = self;
            scratch_report::vec_bytes(runs)
                + scratch_report::vec_bytes(prev)
                + scratch_report::vec_bytes(next)
                + scratch_report::vec_bytes(keys)
                + scratch_report::vec_bytes(scissors)
                + scratch_report::vec_bytes(moved)
                + scratch_report::vec_bytes(shaded)
                + scratch_report::vec_bytes(cutting)
                + scratch_report::vec_bytes(cutting_opened)
        }
    }
}

#[cfg(test)]
mod tests;
