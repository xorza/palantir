//! [`ThemeTransition`]: the blend a window shows while its theme moves to a new one.

use std::mem;
use std::rc::Rc;

use crate::animation::animation_spec::AnimationSpec;
use crate::animation::phase::{Phase, PhaseEnd};
use crate::primitives::math::animatable::Animatable;
use crate::theme::Theme;

/// A window's theme transition: the spec a change runs under, and the blend while one runs.
///
/// Driven by the change, not by a compare: a frame with no run pays nothing. Refused: a theme
/// row in the `AnimationMap`, which would compare a whole `Theme` every frame.
#[derive(Debug)]
pub(crate) struct ThemeTransition {
    /// `None` snaps.
    spec: Option<AnimationSpec>,
    run: Option<ThemeRun>,
    /// A finished run's blend, which the next run writes into, so only a window's first run
    /// allocates.
    spare: Option<Rc<Theme>>,
}

/// One run: `shown` is `lerp(from, to, phase)`.
#[derive(Debug)]
struct ThemeRun {
    from: Rc<Theme>,
    to: Rc<Theme>,
    shown: Rc<Theme>,
    phase: Phase,
}

impl Default for ThemeTransition {
    fn default() -> Self {
        Self {
            spec: Some(AnimationSpec::THEME),
            run: None,
            spare: None,
        }
    }
}

impl ThemeTransition {
    pub(crate) const fn spec(&self) -> Option<AnimationSpec> {
        self.spec
    }

    /// Whether a change animates under the spec: `None` and an instant spec snap.
    pub(crate) fn animates(&self) -> bool {
        self.spec.is_some_and(|spec| !spec.is_instant())
    }

    /// Run on under `spec`. Returns whether a run ended, as an instant or absent spec ends one.
    pub(crate) fn set_spec(&mut self, spec: Option<AnimationSpec>) -> bool {
        self.spec = spec;
        match spec {
            Some(spec) if !spec.is_instant() => {
                if let Some(run) = &mut self.run {
                    run.phase.conform(spec);
                }
                false
            }
            _ => self.snap(),
        }
    }

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
        let spec = self.spec.expect("a change animates only under a spec");
        match &mut self.run {
            None => {
                let shown = match self.spare.take() {
                    Some(mut spare) => {
                        Rc::make_mut(&mut spare).clone_from(&old);
                        spare
                    }
                    None => Rc::clone(&old),
                };
                self.run = Some(ThemeRun {
                    from: old,
                    to: Rc::clone(new),
                    shown,
                    phase: Phase::start(spec),
                });
            }
            Some(run) if Rc::ptr_eq(new, &run.from) => run.phase.turn(PhaseEnd::Start),
            Some(run) if Rc::ptr_eq(new, &run.to) => run.phase.turn(PhaseEnd::End),
            Some(run) => {
                // The blend becomes the new start; the old start's buffer takes a copy, as the
                // rest of this frame shows the blend.
                mem::swap(&mut run.from, &mut run.shown);
                Rc::make_mut(&mut run.shown).clone_from(&run.from);
                run.to = Rc::clone(new);
                run.phase = Phase::start(spec);
            }
        }
    }

    /// End a run where it heads. Returns whether one ran.
    pub(crate) fn snap(&mut self) -> bool {
        match self.run.take() {
            Some(run) => {
                self.keep_spare(run.shown);
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
                    Rc::ptr_eq(if step.cur == 0.0 { &run.from } else { &run.to }, target),
                    "a run lands on the theme the window shows at rest",
                );
            }
            self.snap();
            return true;
        }
        let mix = Theme::lerp(
            Theme::clone(&run.from),
            Theme::clone(&run.to),
            step.cur.clamp(0.0, 1.0),
        );
        *Rc::make_mut(&mut run.shown) = mix;
        true
    }

    /// Keep `shown` for the next run unless something else holds it.
    fn keep_spare(&mut self, shown: Rc<Theme>) {
        if Rc::strong_count(&shown) == 1 {
            self.spare = Some(shown);
        }
    }
}
