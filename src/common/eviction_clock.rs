//! [`ClockSweep`]: one turn of an eviction clock, judged by its caller, so the walk is testable with
//! no slab.

/// What the clock makes of one slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Not a candidate: free or exempt.
    Skip,
    /// Evictable, but freeing it makes no room for the item, as an atlas slot on a shelf that does
    /// not take it.
    Refuse,
    /// The victim.
    Take,
}

/// One turn of the clock: the victim it found, whether it passed an evictable slot it refused,
/// and where the hand stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClockSweep {
    pub(crate) victim: Option<u32>,
    /// With no victim, whether the turn passed a slot [`Verdict::Refuse`]d: a list with none is
    /// dry, and learning it costs no second turn.
    pub(crate) refused: bool,
    /// Where the next turn resumes: past the victim, whose slot is about to be refilled.
    pub(crate) hand: u32,
}

impl ClockSweep {
    /// Walk `len` slots from `hand`, wrapping once, to the first `judge` takes: an approximately
    /// oldest victim without the per-eviction scan of exact LRU.
    pub(crate) fn over(len: usize, hand: u32, judge: impl Fn(usize) -> Verdict) -> Self {
        if len == 0 {
            return Self {
                victim: None,
                refused: false,
                hand: 0,
            };
        }
        let mut at = hand as usize % len;
        let mut refused = false;
        for _ in 0..len {
            let idx = at;
            at = if at + 1 == len { 0 } else { at + 1 };
            match judge(idx) {
                Verdict::Skip => {}
                Verdict::Refuse => refused = true,
                Verdict::Take => {
                    return Self {
                        victim: Some(idx as u32),
                        refused,
                        hand: at as u32,
                    };
                }
            }
        }
        Self {
            victim: None,
            refused,
            hand: at as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::eviction_clock::{ClockSweep, Verdict};

    /// The walk resumes at the hand and wraps once: over `[Skip, Take, Refuse, Take]` from 2 it
    /// refuses 2, takes 3 and parks at 0; from 0 it skips 0, takes 1 and parks at 2.
    /// A turn with nothing to take walks every slot, parks where it began and says whether it
    /// refused one; an empty slab walks none.
    #[test]
    fn the_hand_resumes_and_wraps_once() {
        let slots = [Verdict::Skip, Verdict::Take, Verdict::Refuse, Verdict::Take];
        let judge = |idx: usize| slots[idx];
        assert_eq!(
            ClockSweep::over(4, 2, judge),
            ClockSweep {
                victim: Some(3),
                refused: true,
                hand: 0,
            }
        );
        assert_eq!(
            ClockSweep::over(4, 0, judge),
            ClockSweep {
                victim: Some(1),
                refused: false,
                hand: 2,
            }
        );
        assert_eq!(
            ClockSweep::over(3, 5, |idx| [Verdict::Skip, Verdict::Refuse, Verdict::Skip]
                [idx]),
            ClockSweep {
                victim: None,
                refused: true,
                hand: 2,
            }
        );
        assert!(
            !ClockSweep::over(2, 1, |_| Verdict::Skip).refused,
            "nothing refused, so the list is dry"
        );
        assert_eq!(
            ClockSweep::over(0, 7, |_| Verdict::Take),
            ClockSweep {
                victim: None,
                refused: false,
                hand: 0,
            }
        );
    }
}
