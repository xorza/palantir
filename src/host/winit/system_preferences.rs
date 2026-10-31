//! [`SystemPreferences`]: what the system's settings say palantir follows, read through `mundy`
//! on every OS, once before the first window and then followed. Refused: winit's `ThemeChanged`,
//! silent on X11 and Wayland and on macOS while a window's appearance is pinned; `dark-light`,
//! which reads the scheme alone and polls macOS. Facts and sources:
//! `.notes/research/2026-10-31-system-preferences.md`.

use std::time::Duration;

use mundy::{Interest, Preferences, Subscription};

use crate::host::winit::platform;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::resources::UiResources;

/// How long the host waits at startup for the first read, before it opens a window in the
/// fallback and lets the subscription correct it. A read measured 20 ms cold and 1 ms warm on a
/// KDE desktop through the Settings portal
/// (`.notes/research/2026-10-31-system-preferences.md`); five cold reads bound a portal that is
/// still starting, so a launch waits no longer.
const FIRST_READ_TIMEOUT: Duration = Duration::from_millis(100);

/// The settings palantir follows: every one the `mundy` features in `Cargo.toml` turn on, the
/// color scheme, the double-click time and the reduced-motion preference.
const INTEREST: Interest = Interest::All;

/// The system's color scheme and double-click time, each `None` where the system gives none, and
/// whether it asks for less motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SystemPreferences {
    pub(super) color_scheme: Option<ColorScheme>,
    pub(super) double_click: Option<Duration>,
    pub(super) motion_reduced: bool,
}

impl SystemPreferences {
    /// The settings as the system holds them now, waiting at most [`FIRST_READ_TIMEOUT`]; on the
    /// main thread, as `mundy` asks on macOS.
    pub(super) fn read() -> Self {
        Preferences::once_blocking(INTEREST, FIRST_READ_TIMEOUT)
            .map_or(Self::UNREAD, Self::from_mundy)
    }

    /// Follow the settings: `on_change` runs on `mundy`'s thread with their first values and
    /// each change after, until the subscription drops. On the main thread, as `mundy` asks on
    /// macOS.
    pub(super) fn subscribe(on_change: impl Fn(Self) + Send + Sync + 'static) -> Subscription {
        Preferences::subscribe(INTEREST, move |preferences| {
            on_change(Self::from_mundy(preferences));
        })
    }

    /// What a read that timed out gives: no scheme, so the fallback shows until the subscription
    /// answers, and the platform's fallback double-click time, as a read with no answer has.
    const UNREAD: Self = Self {
        color_scheme: None,
        double_click: platform::DOUBLE_CLICK_FALLBACK,
        motion_reduced: false,
    };

    fn from_mundy(preferences: Preferences) -> Self {
        Self {
            motion_reduced: preferences.reduced_motion == mundy::ReducedMotion::Reduce,
            ..Self::from_answers(
                preferences.color_scheme,
                preferences.double_click_interval.0,
            )
        }
    }

    /// The settings from `mundy`'s scheme and double-click answers, no less motion asked. No
    /// preference reports no scheme, so `System` shows the dark fallback: `mundy` answers it for
    /// a failed read too, as for a Linux portal that fails while the host runs, which must not
    /// turn the app light. Cost: GNOME writes it for its own Light setting, so GNOME's light
    /// users see dark until they pick Light. A double-click time of zero, or none, is the
    /// platform's fallback.
    fn from_answers(scheme: mundy::ColorScheme, double_click: Option<Duration>) -> Self {
        let color_scheme = match scheme {
            mundy::ColorScheme::Light => Some(ColorScheme::Light),
            mundy::ColorScheme::Dark => Some(ColorScheme::Dark),
            mundy::ColorScheme::NoPreference => None,
        };
        let double_click = double_click
            .filter(|window| !window.is_zero())
            .or(platform::DOUBLE_CLICK_FALLBACK);
        Self {
            color_scheme,
            double_click,
            motion_reduced: false,
        }
    }

    /// Write the settings into the host's shared cells: the scheme and the motion always, as an
    /// absent scheme is an answer; the double-click time only when the system gave one.
    pub(super) fn apply(self, resources: &UiResources) {
        resources.system_color_scheme().set(self.color_scheme);
        resources.motion_reduced().set(self.motion_reduced);
        if let Some(window) = self.double_click {
            resources.double_click_window().set(window);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::host::winit::platform;
    use crate::host::winit::system_preferences::SystemPreferences;
    use crate::input_state::capture::DOUBLE_CLICK_WINDOW;
    use crate::theme::color_scheme::ColorScheme;
    use crate::ui::resources::UiResources;

    /// A stated scheme passes through; no preference reports none; a zero or missing double-click
    /// time falls back to the platform's.
    #[test]
    fn mundy_answers_map_to_the_settings_palantir_follows() {
        let ms = Duration::from_millis;
        let table = [
            (
                mundy::ColorScheme::Light,
                Some(ms(250)),
                Some(ColorScheme::Light),
                Some(ms(250)),
            ),
            (
                mundy::ColorScheme::Dark,
                Some(ms(600)),
                Some(ColorScheme::Dark),
                Some(ms(600)),
            ),
            (
                mundy::ColorScheme::NoPreference,
                None,
                None,
                platform::DOUBLE_CLICK_FALLBACK,
            ),
            (
                mundy::ColorScheme::Dark,
                Some(Duration::ZERO),
                Some(ColorScheme::Dark),
                platform::DOUBLE_CLICK_FALLBACK,
            ),
        ];
        for (scheme, double_click, want_scheme, want_double_click) in table {
            assert_eq!(
                SystemPreferences::from_answers(scheme, double_click),
                SystemPreferences {
                    color_scheme: want_scheme,
                    double_click: want_double_click,
                    motion_reduced: false,
                },
                "{scheme:?}, {double_click:?}"
            );
        }
    }

    /// Applying writes the scheme and the motion always, an absent scheme included, and a
    /// double-click time only when one came: a report with none keeps the time an earlier one set.
    #[test]
    fn applying_writes_the_scheme_and_keeps_an_unreported_double_click_time() {
        let resources = UiResources::isolated_mono();
        assert_eq!(resources.double_click_window().get(), DOUBLE_CLICK_WINDOW);
        let ms = Duration::from_millis;
        SystemPreferences {
            color_scheme: Some(ColorScheme::Light),
            double_click: Some(ms(250)),
            motion_reduced: true,
        }
        .apply(&resources);
        assert_eq!(
            resources.system_color_scheme().get(),
            Some(ColorScheme::Light)
        );
        assert_eq!(resources.double_click_window().get(), ms(250));
        assert!(resources.motion_reduced().get());

        SystemPreferences {
            color_scheme: None,
            double_click: None,
            motion_reduced: false,
        }
        .apply(&resources);
        assert_eq!(resources.system_color_scheme().get(), None);
        assert_eq!(resources.double_click_window().get(), ms(250));
        assert!(!resources.motion_reduced().get());
    }
}
