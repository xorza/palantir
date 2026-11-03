//! The app-global settings any window may write, and the one signal that says one moved.

use crate::diagnostics::DebugOverlayConfig;
use crate::display::user_scale::UserScale;
use crate::input_state::capture::DOUBLE_CLICK_WINDOW;
use crate::primitives::reduce_preference::{ReducePreference, Reduction};
use crate::theme::color_scheme::ColorScheme;
use crate::theme::scheme_preference::SchemePreference;
use std::cell::Cell;
use std::time::Duration;

/// Settings every window of a host shares, held once through an `Rc` in
/// [`UiResources`](crate::ui::resources::UiResources), and whether any moved since the host last
/// asked: windows that did not record are asleep, so a write raises the signal and the host's
/// [`Self::take_change`] repaints them all. Writing the held value is not a change, so a
/// per-frame assign repaints nothing. Refused: a signal per setting, which the host must take
/// one by one and OR, and a new setting's could be missed.
#[derive(Debug)]
pub(crate) struct AppSettings {
    /// The debug overlays every window paints.
    debug_overlay: Cell<DebugOverlayConfig>,
    /// The one scale every window's `Display` is minted from; app-global because per-monitor
    /// scale is already `Display::system_scale`, leaving a preference windows should not disagree
    /// about.
    user_scale: Cell<UserScale>,
    /// The scheme the person picked; a pick in one window turns every window.
    scheme_preference: Cell<SchemePreference>,
    /// The scheme the system reports, which the winit host writes; `None` where nothing reports
    /// one.
    system_color_scheme: Cell<Option<ColorScheme>>,
    /// Whether the person wants motion reduced: the system's ask, or a pick by hand.
    motion_preference: Cell<ReducePreference>,
    /// Whether the system asks for less motion, which the winit host writes; full where nothing
    /// reports it.
    system_motion: Cell<Reduction>,
    /// Whether the person wants transparency reduced: the system's ask, or a pick by hand.
    transparency_preference: Cell<ReducePreference>,
    /// Whether the system asks for less transparency, which the winit host writes; full where
    /// nothing reports it.
    system_transparency: Cell<Reduction>,
    /// How soon a second press on the same target makes a double click: the platform's, where
    /// the winit host reads one, else [`DOUBLE_CLICK_WINDOW`]. Its change shows nothing, and
    /// repaints the windows only as every setting's does.
    double_click_window: Cell<Duration>,
    changed: Cell<bool>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            debug_overlay: Cell::default(),
            user_scale: Cell::default(),
            scheme_preference: Cell::default(),
            system_color_scheme: Cell::default(),
            motion_preference: Cell::default(),
            system_motion: Cell::default(),
            transparency_preference: Cell::default(),
            system_transparency: Cell::default(),
            double_click_window: Cell::new(DOUBLE_CLICK_WINDOW),
            changed: Cell::default(),
        }
    }
}

impl AppSettings {
    pub(crate) const fn debug_overlay(&self) -> DebugOverlayConfig {
        self.debug_overlay.get()
    }

    pub(crate) fn set_debug_overlay(&self, overlay: DebugOverlayConfig) {
        self.write(&self.debug_overlay, overlay);
    }

    pub(crate) const fn user_scale(&self) -> UserScale {
        self.user_scale.get()
    }

    pub(crate) fn set_user_scale(&self, scale: UserScale) {
        self.write(&self.user_scale, scale);
    }

    pub(crate) const fn scheme_preference(&self) -> SchemePreference {
        self.scheme_preference.get()
    }

    pub(crate) fn set_scheme_preference(&self, preference: SchemePreference) {
        self.write(&self.scheme_preference, preference);
    }

    pub(crate) const fn system_color_scheme(&self) -> Option<ColorScheme> {
        self.system_color_scheme.get()
    }

    pub(crate) fn set_system_color_scheme(&self, scheme: Option<ColorScheme>) {
        self.write(&self.system_color_scheme, scheme);
    }

    pub(crate) const fn motion_preference(&self) -> ReducePreference {
        self.motion_preference.get()
    }

    pub(crate) fn set_motion_preference(&self, preference: ReducePreference) {
        self.write(&self.motion_preference, preference);
    }

    pub(crate) const fn system_motion(&self) -> Reduction {
        self.system_motion.get()
    }

    pub(crate) fn set_system_motion(&self, motion: Reduction) {
        self.write(&self.system_motion, motion);
    }

    /// The motion every window runs now: the preference resolved against the system's.
    pub(crate) const fn motion(&self) -> Reduction {
        self.motion_preference().resolve(self.system_motion())
    }

    pub(crate) const fn transparency_preference(&self) -> ReducePreference {
        self.transparency_preference.get()
    }

    pub(crate) fn set_transparency_preference(&self, preference: ReducePreference) {
        self.write(&self.transparency_preference, preference);
    }

    pub(crate) const fn system_transparency(&self) -> Reduction {
        self.system_transparency.get()
    }

    pub(crate) fn set_system_transparency(&self, transparency: Reduction) {
        self.write(&self.system_transparency, transparency);
    }

    /// The transparency every window paints now: the preference resolved against the system's.
    pub(crate) const fn transparency(&self) -> Reduction {
        self.transparency_preference()
            .resolve(self.system_transparency())
    }

    pub(crate) const fn double_click_window(&self) -> Duration {
        self.double_click_window.get()
    }

    #[cfg(any(test, feature = "winit"))]
    pub(crate) fn set_double_click_window(&self, window: Duration) {
        self.write(&self.double_click_window, window);
    }

    /// The scheme every window shows now: the preference resolved against the system's.
    pub(crate) const fn color_scheme(&self) -> ColorScheme {
        self.scheme_preference().resolve(self.system_color_scheme())
    }

    /// Whether a setting moved since the last ask, which lowers the signal.
    #[cfg(any(test, feature = "winit"))]
    pub(crate) const fn take_change(&self) -> bool {
        self.changed.replace(false)
    }

    fn write<T: Copy + PartialEq>(&self, setting: &Cell<T>, value: T) {
        if setting.replace(value) != value {
            self.changed.set(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::diagnostics::DebugOverlayConfig;
    use crate::display::user_scale::UserScale;
    use crate::primitives::reduce_preference::{ReducePreference, Reduction};
    use crate::theme::color_scheme::ColorScheme;
    use crate::theme::scheme_preference::SchemePreference;
    use crate::ui::resources::app_settings::AppSettings;

    /// A real move of any setting raises the one signal, which an ask lowers; writing the held
    /// value raises nothing.
    #[test]
    fn a_move_of_any_setting_signals_once_and_a_repeat_not_at_all() {
        let settings = AppSettings::default();
        assert!(!settings.take_change(), "nothing has written yet");
        let writes: [&dyn Fn(&AppSettings); 8] = [
            &|s| {
                s.set_debug_overlay(DebugOverlayConfig {
                    frame_stats: true,
                    ..DebugOverlayConfig::default()
                });
            },
            &|s| s.set_user_scale(UserScale::new(1.25).unwrap()),
            &|s| s.set_scheme_preference(SchemePreference::Light),
            &|s| s.set_system_color_scheme(Some(ColorScheme::Dark)),
            &|s| s.set_system_motion(Reduction::Reduced),
            &|s| s.set_motion_preference(ReducePreference::Full),
            &|s| s.set_system_transparency(Reduction::Reduced),
            &|s| s.set_transparency_preference(ReducePreference::Reduced),
        ];
        for (i, write) in writes.iter().enumerate() {
            write(&settings);
            assert!(settings.take_change(), "setting {i} moved");
            assert!(!settings.take_change(), "the ask lowers the signal");
            write(&settings);
            assert!(!settings.take_change(), "setting {i} held: no change");
        }
        assert!(settings.debug_overlay().frame_stats);
        assert_eq!(settings.user_scale(), UserScale::new(1.25).unwrap());
        assert_eq!(settings.color_scheme(), ColorScheme::Light, "the pick wins");
        assert_eq!(
            settings.motion(),
            Reduction::Full,
            "the pick wins over the system"
        );
        assert_eq!(settings.transparency(), Reduction::Reduced);
    }
}
