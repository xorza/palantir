//! The app-global settings any window may write, and the one signal that says one moved.

use crate::diagnostics::DebugOverlayConfig;
use crate::display::user_scale::UserScale;
use crate::input::capture::DOUBLE_CLICK_INTERVAL;
use crate::primitives::reduction::Reduction;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::ui_preferences::UiPreferences;
use std::cell::Cell;
use std::time::Duration;

/// Settings every window of a host shares, held once in
/// [`UiResources`](crate::ui::resources::UiResources), and whether any moved since the host last
/// asked: windows that did not record are asleep, so a write raises the signal and the host's
/// [`Self::take_change`] repaints them all. Writing the held value is not a change, so a
/// per-frame assign repaints nothing. Each setter returns whether its value moved, so the
/// writing window asks for its own frame on every host, a winit one or a headless one. Refused: a signal per setting, which the host must take
/// one by one and OR, and a new setting's could be missed.
#[derive(Debug)]
pub(crate) struct AppSettings {
    /// The debug overlays every window paints.
    debug_overlay: Cell<DebugOverlayConfig>,
    /// The one scale every window's `Display` is minted from; app-global because per-monitor
    /// scale is already `Display::system_scale`, leaving a preference windows should not disagree
    /// about.
    user_scale: Cell<UserScale>,
    /// The person's picks over the system's answers; a pick in one window turns every window.
    preferences: Cell<UiPreferences>,
    /// The scheme the system reports, which the winit host writes; `None` where nothing reports
    /// one.
    system_color_scheme: Cell<Option<ColorScheme>>,
    /// Whether the system asks for less motion, which the winit host writes; full where nothing
    /// reports it.
    system_motion: Cell<Reduction>,
    /// Whether the system asks for less transparency, which the winit host writes; full where
    /// nothing reports it.
    system_transparency: Cell<Reduction>,
    /// How soon a second press on the same target makes a double click: the platform's, where
    /// the winit host reads one, else [`DOUBLE_CLICK_INTERVAL`]. Its change shows nothing, and
    /// repaints the windows only as every setting's does.
    double_click_interval: Cell<Duration>,
    /// Whether axis-aligned paint edges snap to physical pixels in every window: the host's,
    /// which its builder sets.
    pixel_snap: Cell<bool>,
    changed: Cell<bool>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            debug_overlay: Cell::default(),
            user_scale: Cell::default(),
            preferences: Cell::default(),
            system_color_scheme: Cell::default(),
            system_motion: Cell::default(),
            system_transparency: Cell::default(),
            double_click_interval: Cell::new(DOUBLE_CLICK_INTERVAL),
            pixel_snap: Cell::new(true),
            changed: Cell::default(),
        }
    }
}

impl AppSettings {
    pub(crate) const fn debug_overlay(&self) -> DebugOverlayConfig {
        self.debug_overlay.get()
    }

    pub(crate) fn set_debug_overlay(&self, overlay: DebugOverlayConfig) -> bool {
        self.write(&self.debug_overlay, overlay)
    }

    pub(crate) const fn pixel_snap(&self) -> bool {
        self.pixel_snap.get()
    }

    pub(crate) fn set_pixel_snap(&self, pixel_snap: bool) -> bool {
        self.write(&self.pixel_snap, pixel_snap)
    }

    pub(crate) const fn user_scale(&self) -> UserScale {
        self.user_scale.get()
    }

    pub(crate) fn set_user_scale(&self, scale: UserScale) -> bool {
        self.write(&self.user_scale, scale)
    }

    pub(crate) const fn preferences(&self) -> UiPreferences {
        self.preferences.get()
    }

    pub(crate) fn set_preferences(&self, preferences: UiPreferences) -> bool {
        self.write(&self.preferences, preferences)
    }

    pub(crate) const fn system_color_scheme(&self) -> Option<ColorScheme> {
        self.system_color_scheme.get()
    }

    pub(crate) fn set_system_color_scheme(&self, scheme: Option<ColorScheme>) -> bool {
        self.write(&self.system_color_scheme, scheme)
    }

    pub(crate) const fn system_motion(&self) -> Reduction {
        self.system_motion.get()
    }

    pub(crate) fn set_system_motion(&self, motion: Reduction) -> bool {
        self.write(&self.system_motion, motion)
    }

    /// The motion every window runs now: the preference resolved against the system's.
    pub(crate) const fn motion(&self) -> Reduction {
        self.preferences().motion.resolve(self.system_motion())
    }

    pub(crate) const fn system_transparency(&self) -> Reduction {
        self.system_transparency.get()
    }

    pub(crate) fn set_system_transparency(&self, transparency: Reduction) -> bool {
        self.write(&self.system_transparency, transparency)
    }

    /// The transparency every window paints now: the preference resolved against the system's.
    pub(crate) const fn transparency(&self) -> Reduction {
        self.preferences()
            .transparency
            .resolve(self.system_transparency())
    }

    pub(crate) const fn double_click_interval(&self) -> Duration {
        self.double_click_interval.get()
    }

    pub(crate) fn set_double_click_interval(&self, interval: Duration) -> bool {
        self.write(&self.double_click_interval, interval)
    }

    /// The scheme every window shows now: the preference resolved against the system's, or
    /// [`ColorScheme::FALLBACK`] where nothing reports one.
    pub(crate) const fn color_scheme(&self) -> ColorScheme {
        let system = match self.system_color_scheme() {
            Some(scheme) => scheme,
            None => ColorScheme::FALLBACK,
        };
        self.preferences().scheme.resolve(system)
    }

    /// Whether a setting moved since the last ask, which lowers the signal.
    #[cfg(any(test, feature = "winit"))]
    pub(crate) const fn take_change(&self) -> bool {
        self.changed.replace(false)
    }

    /// Hold `value`; returns whether it moved, so the window that wrote it asks for a frame, as
    /// [`Self::take_change`] asks the host for every other window.
    fn write<T: Copy + PartialEq>(&self, setting: &Cell<T>, value: T) -> bool {
        let moved = setting.replace(value) != value;
        if moved {
            self.changed.set(true);
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use crate::diagnostics::DebugOverlayConfig;
    use crate::display::user_scale::UserScale;
    use crate::primitives::preference::Preference;
    use crate::primitives::reduction::Reduction;
    use crate::theme::color_scheme::ColorScheme;
    use crate::ui::resources::app_settings::AppSettings;
    use crate::ui::ui_preferences::UiPreferences;

    /// A real move of any setting raises the one signal, which an ask lowers, and the write
    /// returns it moved; writing the held value raises nothing and returns no move.
    #[test]
    fn a_move_of_any_setting_signals_once_and_a_repeat_not_at_all() {
        let settings = AppSettings::default();
        assert!(!settings.take_change(), "nothing has written yet");
        assert_eq!(
            settings.color_scheme(),
            ColorScheme::FALLBACK,
            "nothing reports a scheme"
        );
        let writes: [&dyn Fn(&AppSettings) -> bool; 6] = [
            &|s| {
                s.set_debug_overlay(DebugOverlayConfig {
                    frame_stats: true,
                    ..DebugOverlayConfig::default()
                })
            },
            &|s| s.set_user_scale(UserScale::new(1.25).unwrap()),
            &|s| s.set_system_color_scheme(Some(ColorScheme::Dark)),
            &|s| s.set_system_motion(Reduction::Reduced),
            &|s| s.set_system_transparency(Reduction::Reduced),
            &|s| {
                s.set_preferences(UiPreferences {
                    scheme: Preference::Fixed(ColorScheme::Light),
                    motion: Preference::Fixed(Reduction::Full),
                    transparency: Preference::System,
                })
            },
        ];
        for (i, write) in writes.iter().enumerate() {
            assert!(write(&settings), "setting {i} returns its move");
            assert!(settings.take_change(), "setting {i} moved");
            assert!(!settings.take_change(), "the ask lowers the signal");
            assert!(
                !write(&settings),
                "setting {i} held: the write returns no move"
            );
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
        assert_eq!(
            settings.transparency(),
            Reduction::Reduced,
            "System follows the system"
        );
    }
}
