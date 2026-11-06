//! [`ThemeTransition`]: the blend a window shows while its theme moves to a new one.

use std::mem;
use std::rc::Rc;

use crate::animation::animation_spec::AnimationSpec;
use crate::animation::phase::{Phase, PhaseEnd};
use crate::theme::Theme;

/// A window's theme transition: the blend while one runs, under the spec of the theme it heads for.
///
/// Driven by the change, not by a compare: a frame with no run pays nothing. Refused: a theme
/// row in the `AnimationMap`, which would compare a whole `Theme` every frame.
#[derive(Debug, Default)]
pub(crate) struct ThemeTransition {
    run: Option<ThemeRun>,
    /// A finished run's buffers, its blend and the start a third theme gave it, which later runs
    /// write into, so only a window's first runs allocate, also when the app holds its themes.
    spares: [Option<Rc<Theme>>; 2],
    /// A finished run's shared ends, which the next run writes into.
    spare_ends: Option<Box<SharedEnds>>,
}

/// One run: `shown` is `lerp(from, to, phase)`, blended in place from `ends`, so a frame copies
/// neither 28 KiB theme and shares no text axes again.
#[derive(Debug)]
struct ThemeRun {
    from: Rc<Theme>,
    to: Rc<Theme>,
    shown: Rc<Theme>,
    phase: Phase,
    /// `from` and `to` with each one's text axes shared with the other, as `Theme::lerp` shares
    /// them before it blends; [`Self::aim`] keeps them with the ends.
    ends: Box<SharedEnds>,
}

impl ThemeRun {
    /// Head for `to` from rest, from the start the run holds now.
    fn aim(&mut self, to: Rc<Theme>, spec: AnimationSpec) {
        self.to = to;
        self.phase = Phase::start(spec);
        self.ends.refill(&self.from, &self.to);
    }
}

/// A run's ends, prepared for a blend in place.
#[derive(Debug)]
struct SharedEnds {
    from: Theme,
    to: Theme,
}

impl SharedEnds {
    /// `from` and `to`, their text axes shared, in `spare` where one is kept.
    fn of(spare: Option<Box<Self>>, from: &Theme, to: &Theme) -> Box<Self> {
        if let Some(mut ends) = spare {
            ends.refill(from, to);
            return ends;
        }
        let mut ends = Box::new(Self {
            from: from.clone(),
            to: to.clone(),
        });
        Theme::share_text_axes(&mut ends.from, &mut ends.to);
        ends
    }

    /// Hold `from` and `to` from now on, their text axes shared.
    fn refill(&mut self, from: &Theme, to: &Theme) {
        self.from.clone_from(from);
        self.to.clone_from(to);
        Theme::share_text_axes(&mut self.from, &mut self.to);
    }
}

impl ThemeTransition {
    /// The blend this frame shows, while a run is on.
    pub(crate) const fn shown(&self) -> Option<&Rc<Theme>> {
        match &self.run {
            Some(run) => Some(&run.shown),
            None => None,
        }
    }

    /// The window's target moved from `old` to `new`, and the change animates. With no run, one
    /// starts at `old`. A run heading for one of its own ends turns to `new` by the row rules; a
    /// third theme starts a new run from the blend shown now, from rest.
    pub(crate) fn follow(&mut self, old: Rc<Theme>, new: &Rc<Theme>) {
        let spec = new.transition;
        debug_assert!(
            !spec.is_instant(),
            "a change animates only under a moving spec"
        );
        match &mut self.run {
            None => {
                let shown = match take_spare(&mut self.spares) {
                    Some(mut spare) => {
                        Rc::make_mut(&mut spare).clone_from(&old);
                        spare
                    }
                    None => Rc::clone(&old),
                };
                let ends = SharedEnds::of(self.spare_ends.take(), &old, new);
                self.run = Some(ThemeRun {
                    from: old,
                    to: Rc::clone(new),
                    shown,
                    phase: Phase::start(spec),
                    ends,
                });
            }
            Some(run) if Rc::ptr_eq(new, &run.from) => run.phase.turn(PhaseEnd::Start, spec),
            Some(run) if Rc::ptr_eq(new, &run.to) => run.phase.turn(PhaseEnd::End, spec),
            Some(run) => {
                // The blend becomes the new start, and the rest of this frame shows a copy of it:
                // in the old start's buffer where the run owns it, else in a spare, as the app
                // may hold that theme, which a write would clone.
                mem::swap(&mut run.from, &mut run.shown);
                if Rc::get_mut(&mut run.shown).is_none()
                    && let Some(spare) = take_spare(&mut self.spares)
                {
                    run.shown = spare;
                }
                Rc::make_mut(&mut run.shown).clone_from(&run.from);
                run.aim(Rc::clone(new), spec);
            }
        }
    }

    /// End a run where it heads. Returns whether one ran.
    pub(crate) fn snap(&mut self) -> bool {
        match self.run.take() {
            Some(run) => {
                keep_spare(&mut self.spares, run.shown);
                keep_spare(&mut self.spares, run.from);
                self.spare_ends = Some(run.ends);
                true
            }
            None => false,
        }
    }

    /// Move a run `dt` seconds on toward `target`, the theme the window shows at rest. Returns
    /// whether the shown theme changed: a new blend, or the run's end, where the window shows
    /// `target`. A spring's swing past an end shows the end: a theme past one could hold a
    /// negative length, which a widget refuses.
    pub(crate) fn advance(&mut self, dt: f32, target: &Rc<Theme>) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        if dt == 0.0 {
            return false;
        }
        let step = run.phase.advance(dt);
        if step.settled {
            #[expect(
                clippy::debug_assert_with_mut_call,
                reason = "the ends are read through the run's `&mut`, never written"
            )]
            {
                debug_assert!(
                    Rc::ptr_eq(
                        if step.value == 0.0 {
                            &run.from
                        } else {
                            &run.to
                        },
                        target
                    ),
                    "a run lands on the theme the window shows at rest",
                );
            }
            self.snap();
            return true;
        }
        let t = step.value.clamp(0.0, 1.0);
        let shown = Rc::make_mut(&mut run.shown);
        if t == 0.0 {
            shown.clone_from(&run.from);
        } else if t == 1.0 {
            shown.clone_from(&run.to);
        } else {
            Theme::lerp_ends_into(&run.ends.from, &run.ends.to, t, shown);
        }
        true
    }
}

/// A spare buffer, if one is kept.
fn take_spare(spares: &mut [Option<Rc<Theme>>; 2]) -> Option<Rc<Theme>> {
    spares.iter_mut().find_map(Option::take)
}

/// Keep `theme` for a later run, unless something else holds it or both spares are kept.
fn keep_spare(spares: &mut [Option<Rc<Theme>>; 2], mut theme: Rc<Theme>) {
    if Rc::get_mut(&mut theme).is_some()
        && let Some(slot) = spares.iter_mut().find(|slot| slot.is_none())
    {
        *slot = Some(theme);
    }
}
