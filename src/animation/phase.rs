//! [`Phase`]: a position between two ends that moves toward one of them under an
//! [`AnimationSpec`], by the rules every animated value follows.

use crate::animation::animation_map_typed::TickResult;
use crate::animation::animation_row::{MotionRow, Retarget};
use crate::animation::animation_spec::AnimationSpec;

/// Which end a [`Phase`] heads for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhaseEnd {
    /// 0.
    Start,
    /// 1.
    End,
}

impl PhaseEnd {
    const fn at(self) -> f32 {
        match self {
            Self::Start => 0.0,
            Self::End => 1.0,
        }
    }
}

/// How far a blend between two values has come, 0 at its start and 1 at its end, moving under one
/// row: a turn back runs CSS Transitions' shortened reversal under a duration and keeps the
/// velocity under a spring, as a turn of any animated value does. A spring may carry it past an
/// end.
#[derive(Debug)]
pub(crate) struct Phase {
    motion: MotionRow<f32>,
    heads_for: PhaseEnd,
}

impl Phase {
    /// At the start, heading for the end under `spec`.
    pub(crate) fn start(spec: AnimationSpec) -> Self {
        Self {
            motion: MotionRow::new(spec.motion, &0.0, &1.0),
            heads_for: PhaseEnd::End,
        }
    }

    /// Where it is now.
    pub(crate) fn at(&self) -> f32 {
        self.motion.cur(&self.heads_for.at())
    }

    /// Head for `end` from where it is; nothing if it already does.
    pub(crate) fn turn(&mut self, end: PhaseEnd) {
        if end == self.heads_for {
            return;
        }
        let cur = self.at();
        self.motion.retarget(&Retarget {
            cur: &cur,
            old_target: &self.heads_for.at(),
            target: &end.at(),
            old_key: None,
            key: None,
            at_rest: false,
        });
        self.heads_for = end;
    }

    /// Run on under `spec` from where it is: the same model keeps its state, another starts
    /// from rest.
    pub(crate) fn conform(&mut self, spec: AnimationSpec) {
        let cur = self.at();
        self.motion.conform(spec.motion, &cur, &self.heads_for.at());
    }

    /// Move `dt` seconds on; settled once it rests on the end it heads for.
    pub(crate) fn advance(&mut self, dt: f32) -> TickResult<f32> {
        self.motion.advance(self.heads_for.at(), dt)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::animation::animation_spec::AnimationSpec;
    use crate::animation::easing::Easing;
    use crate::animation::phase::{Phase, PhaseEnd};

    /// Linear over 1 s: a quarter second in is ¼. Turned back there, the reversal runs only the
    /// way it came, ¼ s, so ⅛ s later it is halfway home at ⅛, and ⅛ s more lands at 0.
    #[test]
    fn a_turn_back_runs_only_the_way_it_came() {
        let mut phase = Phase::start(AnimationSpec::duration(
            Duration::from_secs(1),
            Easing::Linear,
        ));
        assert_eq!(phase.at(), 0.0);
        let step = phase.advance(0.25);
        assert_eq!((step.cur, step.settled), (0.25, false));
        phase.turn(PhaseEnd::Start);
        assert_eq!(phase.at(), 0.25);
        assert_eq!(phase.advance(0.125).cur, 0.125);
        let step = phase.advance(0.125);
        assert_eq!((step.cur, step.settled), (0.0, true));
    }

    /// A turn keeps the position, so the blend does not jump; a turn toward the end it already
    /// heads for changes nothing, step for step.
    #[test]
    fn a_turn_keeps_the_position_and_a_second_one_is_none() {
        let mut phase = Phase::start(AnimationSpec::SPRING);
        let mut twin = Phase::start(AnimationSpec::SPRING);
        for _ in 0..3 {
            phase.advance(1.0 / 60.0);
            twin.advance(1.0 / 60.0);
        }
        phase.turn(PhaseEnd::End);
        assert_eq!(phase.advance(1.0 / 60.0).cur, twin.advance(1.0 / 60.0).cur);
        let at = phase.at();
        phase.turn(PhaseEnd::Start);
        assert_eq!(phase.at(), at);
    }
}
