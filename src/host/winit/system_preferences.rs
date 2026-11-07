//! The system's settings palantir follows, as a
//! [`SystemPreferences`], read through
//! `mundy` on every OS, once before the first window and then followed. Refused: winit's `ThemeChanged`,
//! silent on X11 and Wayland and on macOS while a window's appearance is pinned; `dark-light`,
//! which reads the scheme alone and polls macOS. Facts and sources:
//! `.notes/research/2026-10-31-system-preferences.md`.

use std::time::Duration;

use mundy::{Interest, Preferences, Subscription};

use crate::host::system_preferences::SystemPreferences;
use crate::host::winit::platform;
use crate::primitives::reduction::Reduction;
use crate::theme::color_scheme::ColorScheme;

/// How long the host waits at startup for the first read, before it opens a window in the
/// fallback and lets the subscription correct it. A read measured 20 ms cold and 1 ms warm on a
/// Linux desktop through the Settings portal
/// (`.notes/research/2026-10-31-system-preferences.md`); five cold reads bound a portal that is
/// still starting, so a launch waits no longer.
const FIRST_READ_TIMEOUT: Duration = Duration::from_millis(100);

/// The settings palantir follows: every one the `mundy` features in `Cargo.toml` turn on, the
/// color scheme, the double-click time and the reduced-motion and reduced-transparency
/// preferences.
const INTEREST: Interest = Interest::All;

/// The settings as the system holds them now, waiting at most [`FIRST_READ_TIMEOUT`]; on the
/// main thread, as `mundy` asks on macOS.
pub(super) fn read() -> SystemPreferences {
    Preferences::once_blocking(INTEREST, FIRST_READ_TIMEOUT).map_or(UNREAD, from_mundy)
}

/// Follow the settings: `on_change` runs on `mundy`'s thread with their first values and each
/// change after, until the subscription drops. On the main thread, as `mundy` asks on macOS.
pub(super) fn subscribe(
    on_change: impl Fn(SystemPreferences) + Send + Sync + 'static,
) -> Subscription {
    Preferences::subscribe(INTEREST, move |preferences| {
        on_change(from_mundy(preferences));
    })
}

/// What a read that timed out gives: no scheme, so the fallback shows until the subscription
/// answers, and the platform's fallback double-click time, as a read with no answer has.
const UNREAD: SystemPreferences = SystemPreferences {
    double_click: platform::DOUBLE_CLICK_FALLBACK,
    ..SystemPreferences::NONE
};

fn from_mundy(preferences: Preferences) -> SystemPreferences {
    SystemPreferences {
        motion: Reduction::of(preferences.reduced_motion == mundy::ReducedMotion::Reduce),
        transparency: Reduction::of(
            preferences.reduced_transparency == mundy::ReducedTransparency::Reduce,
        ),
        ..from_answers(
            preferences.color_scheme,
            preferences.double_click_interval.0,
        )
    }
}

/// The settings from `mundy`'s scheme and double-click answers, no less motion or transparency
/// asked and no power-saver mode on. No preference reports no scheme, so `System` shows the dark
/// fallback: `mundy` answers it for a failed read too, as for a Linux portal that fails while the
/// host runs, which must not turn the app light. Cost: GNOME writes it for its own Light setting, so GNOME's light users see
/// dark until they pick Light. A double-click time of zero, or none, is the platform's fallback.
fn from_answers(scheme: mundy::ColorScheme, double_click: Option<Duration>) -> SystemPreferences {
    let color_scheme = match scheme {
        mundy::ColorScheme::Light => Some(ColorScheme::Light),
        mundy::ColorScheme::Dark => Some(ColorScheme::Dark),
        mundy::ColorScheme::NoPreference => None,
    };
    let double_click = double_click
        .filter(|window| !window.is_zero())
        .or(platform::DOUBLE_CLICK_FALLBACK);
    SystemPreferences {
        color_scheme,
        double_click,
        ..SystemPreferences::NONE
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::host::system_preferences::SystemPreferences;
    use crate::host::winit::{platform, system_preferences};
    use crate::primitives::reduction::Reduction;
    use crate::theme::color_scheme::ColorScheme;

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
                system_preferences::from_answers(scheme, double_click),
                SystemPreferences {
                    color_scheme: want_scheme,
                    double_click: want_double_click,
                    motion: Reduction::Full,
                    transparency: Reduction::Full,
                    power: Reduction::Full,
                },
                "{scheme:?}, {double_click:?}"
            );
        }
    }
}
