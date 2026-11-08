//! [`PressList`]: the live presses of one widget, which its theme's press effect draws.

use std::time::Duration;

use glam::Vec2;
use tinyvec::ArrayVec;

use crate::theme::look::press_edge::PressEdge;
use crate::theme::look::press_effect::PressEffect;

/// One widget's live presses, at most [`PressEffect::MAX_PRESSES`], oldest first, and the effect
/// that draws them. A press past the bound starts none, as Android's `RippleDrawable` drops a
/// ripple past `MAX_RIPPLES`. A press goes once its fade has run ([`Press::ends_at`]), at the next
/// record. The widget's state row holds a list only while a press is live, as
/// [`Ui::set_state`](crate::Ui::set_state) keeps state that something under way holds, so a widget
/// no one presses costs one lookup a frame.
#[derive(Debug)]
pub(crate) struct PressList {
    /// The effect the widget wore at its last [`Self::note`], which draws every press.
    effect: PressEffect,
    presses: ArrayVec<[Press; PressEffect::MAX_PRESSES]>,
}

/// One press: where it landed, when, and when it was released.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Press {
    pub(crate) point: Vec2,
    pub(crate) pressed_at: Duration,
    pub(crate) released_at: Option<Duration>,
}

impl Press {
    /// When `effect` ends this press: `exit` after the release or the end of `enter`, whichever
    /// is later, as [`PressInput`](crate::PressInput) times it; `None` while it is held.
    pub(crate) fn ends_at(&self, effect: &PressEffect) -> Option<Duration> {
        self.released_at
            .map(|released| released.max(self.pressed_at + effect.enter) + effect.exit)
    }
}

impl PressList {
    /// A list of no press, drawn in `effect`.
    pub(crate) fn new(effect: PressEffect) -> Self {
        Self {
            effect,
            presses: ArrayVec::new(),
        }
    }

    /// Follow `edge` at `now` in `effect`, past the presses it ended. A frame's second record
    /// pass shows no edge, as the first drains them, so a press starts once.
    pub(crate) fn note(&mut self, edge: PressEdge, now: Duration, effect: PressEffect) {
        self.effect = effect;
        self.presses
            .retain(|press| press.ends_at(&effect).is_none_or(|end| end > now));
        let mut held = self
            .presses
            .iter_mut()
            .filter(|press| press.released_at.is_none());
        match edge {
            PressEdge::Held => {}
            PressEdge::Idle => held.for_each(|press| press.released_at = Some(now)),
            PressEdge::Pressed { point } => self.start(Press {
                point,
                pressed_at: now,
                released_at: None,
            }),
            PressEdge::Released { center } => match held.next_back() {
                Some(held) => held.released_at = Some(now),
                None => self.start(Press {
                    point: center,
                    pressed_at: now,
                    released_at: Some(now),
                }),
            },
        }
    }

    /// Start `press`, unless the list holds as many as it bounds.
    fn start(&mut self, press: Press) {
        if self.presses.len() < PressEffect::MAX_PRESSES {
            self.presses.push(press);
        }
    }

    pub(crate) const fn effect(&self) -> PressEffect {
        self.effect
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.presses.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Press> {
        self.presses.iter()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::Vec2;

    use crate::effect::effect_name::EffectName;
    use crate::primitives::paint::color::RgbaF32;
    use crate::theme::look::press_edge::PressEdge;
    use crate::theme::look::press_effect::PressEffect;
    use crate::widget_core::press_list::{Press, PressList};

    /// Material's times: 0.225 s to enter, 0.15 s to exit.
    const RIPPLE: PressEffect = PressEffect {
        effect: EffectName::RIPPLE,
        color: RgbaF32::WHITE,
        enter: Duration::from_millis(225),
        exit: Duration::from_millis(150),
    };

    const fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn pressed(x: f32) -> PressEdge {
        PressEdge::Pressed {
            point: Vec2::new(x, 2.0),
        }
    }

    const RELEASED: PressEdge = PressEdge::Released {
        center: Vec2::new(8.0, 4.0),
    };

    fn presses(list: &PressList) -> Vec<Press> {
        list.iter().copied().collect()
    }

    /// A press past the tenth starts none: eleven presses, one a frame and none released, keep
    /// the first ten, the ones at x = 0 to 9.
    #[test]
    fn a_press_past_the_bound_starts_none() {
        let mut list = PressList::new(RIPPLE);
        for i in 0..11_u16 {
            list.note(pressed(f32::from(i)), ms(u64::from(i)), RIPPLE);
        }
        let xs: Vec<f32> = list.iter().map(|press| press.point.x).collect();
        assert_eq!(xs, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    }

    /// A press released before its enter ran fades from the end of the enter: pressed at 0 and
    /// released at 0.1 s, it ends at max(0.1, 0.225) + 0.15 = 0.375 s, so a note at 0.374 s keeps
    /// it and one at 0.375 s drops it. Released after its enter, at 0.3 s, it ends at
    /// 0.3 + 0.15 = 0.45 s.
    #[test]
    fn a_release_fades_from_the_later_of_itself_and_the_enter() {
        let mut list = PressList::new(RIPPLE);
        list.note(pressed(1.0), ms(0), RIPPLE);
        assert_eq!(presses(&list)[0].ends_at(&RIPPLE), None, "held");
        list.note(RELEASED, ms(100), RIPPLE);
        assert_eq!(
            presses(&list),
            [Press {
                point: Vec2::new(1.0, 2.0),
                pressed_at: ms(0),
                released_at: Some(ms(100)),
            }]
        );
        assert_eq!(presses(&list)[0].ends_at(&RIPPLE), Some(ms(375)));
        list.note(PressEdge::Idle, ms(374), RIPPLE);
        assert_eq!(presses(&list).len(), 1, "before its end");
        list.note(PressEdge::Idle, ms(375), RIPPLE);
        assert_eq!(presses(&list), [], "at its end");

        let mut late = PressList::new(RIPPLE);
        late.note(pressed(1.0), ms(0), RIPPLE);
        late.note(RELEASED, ms(300), RIPPLE);
        assert_eq!(presses(&late)[0].ends_at(&RIPPLE), Some(ms(450)));
    }

    /// A release ends the newest held press, an older one keeping its own release; one with
    /// none held is a tap at the center that starts and releases at once, as a key's activation.
    /// A frame with no press on the widget, as after it was disabled under a held press, ends
    /// that press there and starts none, and a press that goes on keeps it held.
    #[test]
    fn a_release_ends_the_newest_held_press_or_taps_the_center() {
        let mut list = PressList::new(RIPPLE);
        list.note(pressed(1.0), ms(0), RIPPLE);
        list.note(RELEASED, ms(10), RIPPLE);
        list.note(pressed(3.0), ms(20), RIPPLE);
        list.note(RELEASED, ms(30), RIPPLE);
        let released: Vec<_> = list.iter().map(|press| press.released_at).collect();
        assert_eq!(released, [Some(ms(10)), Some(ms(30))]);

        let mut idle = PressList::new(RIPPLE);
        idle.note(pressed(1.0), ms(0), RIPPLE);
        idle.note(PressEdge::Held, ms(10), RIPPLE);
        assert_eq!(presses(&idle)[0].released_at, None, "held");
        idle.note(PressEdge::Idle, ms(20), RIPPLE);
        assert_eq!(
            presses(&idle),
            [Press {
                point: Vec2::new(1.0, 2.0),
                pressed_at: ms(0),
                released_at: Some(ms(20)),
            }]
        );

        let mut tap = PressList::new(RIPPLE);
        tap.note(RELEASED, ms(40), RIPPLE);
        assert_eq!(
            presses(&tap),
            [Press {
                point: Vec2::new(8.0, 4.0),
                pressed_at: ms(40),
                released_at: Some(ms(40)),
            }]
        );
    }
}
