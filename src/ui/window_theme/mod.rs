//! [`WindowTheme`]: a window's themes, the scheme it shows, and the blend while the shown one
//! moves.

use std::mem;
use std::rc::Rc;

use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::theme_pair::ThemePair;
use crate::ui::theme_transition::ThemeTransition;

/// Whether a change of the shown theme may run under the new theme's spec or lands at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    Animate,
    Snap,
}

/// A window's theme for each scheme, the scheme it shows, the blend while the shown theme moves,
/// and the epoch a recorded tree compares with. Every change of what [`Self::shown`] returns
/// moves the epoch, here and nowhere else, so a frame can tell its tree was recorded under another
/// theme (`RecordBaseline`).
#[derive(Debug)]
pub(crate) struct WindowTheme {
    /// Refcounted so a widget can hold the theme across the `&mut Ui` its `show` takes; cloning
    /// the bundle per widget per frame would be a large copy.
    pair: ThemePair,
    transition: ThemeTransition,
    /// The scheme this frame shows, resolved once at its start so a frame never mixes two.
    scheme: ColorScheme,
    epoch: u32,
    /// Whether the pair is new since [`Self::take_replaced`] last handed it out: the stock pair at
    /// the start, whose floats wear the stock frost, or a set that replaced either theme.
    replaced: bool,
}

impl WindowTheme {
    /// Each scheme's stock theme, showing `scheme`.
    pub(crate) fn new(scheme: ColorScheme) -> Self {
        Self {
            pair: ThemePair::stock(),
            transition: ThemeTransition::default(),
            scheme,
            epoch: 0,
            replaced: true,
        }
    }

    /// The theme this frame shows: the blend while one runs, else the shown scheme's.
    pub(crate) const fn shown(&self) -> &Rc<Theme> {
        match self.transition.shown() {
            Some(blend) => blend,
            None => self.pair.get(self.scheme),
        }
    }

    /// The theme for `scheme`, shown or not.
    pub(crate) const fn get(&self, scheme: ColorScheme) -> &Rc<Theme> {
        self.pair.get(scheme)
    }

    pub(crate) const fn scheme(&self) -> ColorScheme {
        self.scheme
    }

    pub(crate) const fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Whether a transition runs, which owes the next frame.
    pub(crate) const fn is_moving(&self) -> bool {
        self.transition.shown().is_some()
    }

    /// Hold `theme` for `scheme`. Returns whether the shown target changed, which the next frame
    /// shows: a transition starts or turns under [`Motion::Animate`] and a moving spec, else the
    /// theme lands. A theme equal to the held one is no change, as Flutter's `AnimatedTheme` compares
    /// `ThemeData` by value, so an app that sets its theme each frame still idles.
    pub(crate) fn set(&mut self, scheme: ColorScheme, theme: Rc<Theme>, motion: Motion) -> bool {
        let slot = self.pair.get_mut(scheme);
        if Rc::ptr_eq(slot, &theme) || **slot == *theme {
            return false;
        }
        let old = mem::replace(slot, theme);
        self.replaced = true;
        if scheme != self.scheme {
            return false;
        }
        self.follow(old, motion);
        true
    }

    /// Both themes, once at the start and once after a set replaced either: what the next frame
    /// wants the press and material effects of ([`Ui::set_theme`](crate::Ui::set_theme)). A theme
    /// a later set replaced before that frame is never handed out, so its effects build nothing.
    pub(crate) fn take_replaced(&mut self) -> Option<[&Rc<Theme>; 2]> {
        if !mem::take(&mut self.replaced) {
            return None;
        }
        Some([
            self.pair.get(ColorScheme::Light),
            self.pair.get(ColorScheme::Dark),
        ])
    }

    /// At a frame's start, after its clock: move a transition `dt` seconds on, or land it under
    /// [`Motion::Snap`], then show `resolved`, the scheme the app-global settings resolve to. A
    /// scheme change starts its transition at the theme shown, so the frame that adopts it shows
    /// no step yet; two schemes that hold one theme change nothing shown.
    pub(crate) fn advance(&mut self, dt: f32, resolved: ColorScheme, motion: Motion) {
        let moved = match motion {
            Motion::Snap => self.transition.snap(),
            Motion::Animate => self.transition.advance(dt, self.pair.get(self.scheme)),
        };
        if moved {
            self.bump();
        }
        if resolved == self.scheme {
            return;
        }
        let old = Rc::clone(self.pair.get(self.scheme));
        self.scheme = resolved;
        if !Rc::ptr_eq(&old, self.pair.get(resolved)) {
            self.follow(old, motion);
        }
    }

    /// The shown target moved from `old` to the shown scheme's theme: start or turn a transition,
    /// which shows `old` this frame, or land the new theme, as it does under an instant spec.
    fn follow(&mut self, old: Rc<Theme>, motion: Motion) {
        let new = self.pair.get(self.scheme);
        if motion == Motion::Animate && !new.transition.is_instant() {
            self.transition.follow(old, new);
        } else {
            self.transition.snap();
            self.bump();
        }
    }

    const fn bump(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::theme::Theme;
    use crate::ui::window_theme::WindowTheme;
    use std::rc::Rc;

    impl WindowTheme {
        /// The shown scheme's theme, for an in-place edit, ending any transition. Copy-on-write,
        /// so a handle from [`Self::shown`] keeps its values.
        pub(crate) fn edit(&mut self) -> &mut Theme {
            self.transition.snap();
            self.bump();
            self.replaced = true;
            Rc::make_mut(self.pair.get_mut(self.scheme))
        }
    }
}

#[cfg(test)]
mod tests;
