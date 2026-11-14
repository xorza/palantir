//! The system's settings palantir follows, as a [`SystemPreferences`], from two sources: `mundy`
//! on every OS, read once before the first window and then followed, and the platform's
//! power-saver watch ([`PowerSaverWatch`]), as `mundy` 0.2.3 reads no power state. Refused:
//! winit's `ThemeChanged`, silent on X11 and Wayland and on macOS while a window's appearance is
//! pinned; `dark-light`, which reads the scheme alone and polls macOS; waiting for `mundy` to read
//! the power state, which leaves the preference with no source. Facts and sources:
//! `.notes/research/2026-10-31-system-preferences.md`.

use std::fmt;
use std::time::Duration;

use mundy::{Interest, Preferences, Subscription};

use crate::host::system_preferences::SystemPreferences;
use crate::host::winit::platform::{self, PowerSaverWatch};
use crate::primitives::reduction::{Reduction, Reductions};
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

/// `mundy`'s settings as the system holds them now, waiting at most [`FIRST_READ_TIMEOUT`], and no
/// power-saver mode, which its watch reports once it starts; on the main thread, as `mundy` asks
/// on macOS.
pub(super) fn read() -> SystemPreferences {
    Preferences::once_blocking(INTEREST, FIRST_READ_TIMEOUT).map_or(UNREAD, |preferences| {
        from_mundy(preferences, Reduction::Unreduced)
    })
}

/// What one source of the system's settings reports, from a thread of its own.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SystemReport {
    /// `mundy`'s answers: every setting but the power-saver mode.
    Desktop(Preferences),
    /// The platform's power-saver mode.
    PowerSaver(Reduction),
}

/// The winit host's following of the system's settings: both sources, which stop when it drops,
/// and the settings as their last reports left them, which each report updates in its own part.
/// Refused: each source sending a whole [`SystemPreferences`], whose other part would undo the
/// other source's last report.
pub(super) struct SystemFollow {
    held: SystemPreferences,
    _desktop: Subscription,
    _power_saver: PowerSaverWatch,
}

impl fmt::Debug for SystemFollow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemFollow")
            .field("held", &self.held)
            .finish_non_exhaustive()
    }
}

impl SystemFollow {
    /// Follow both sources from `held`, the settings [`read`] gave; `send` runs on each source's
    /// thread with its first report and each change after. On the main thread, as `mundy` asks on
    /// macOS.
    pub(super) fn start(
        held: SystemPreferences,
        send: impl Fn(SystemReport) + Clone + Send + Sync + 'static,
    ) -> Self {
        let desktop = {
            let send = send.clone();
            Preferences::subscribe(INTEREST, move |preferences| {
                send(SystemReport::Desktop(preferences));
            })
        };
        let power_saver =
            PowerSaverWatch::follow(move |power| send(SystemReport::PowerSaver(power)));
        Self {
            held,
            _desktop: desktop,
            _power_saver: power_saver,
        }
    }

    /// The settings once `report` replaces its source's part.
    pub(super) fn fold(&mut self, report: SystemReport) -> SystemPreferences {
        self.held = folded(self.held, report);
        self.held
    }
}

/// `held` with `report`'s part replaced and the other source's part kept.
fn folded(held: SystemPreferences, report: SystemReport) -> SystemPreferences {
    match report {
        SystemReport::Desktop(preferences) => from_mundy(preferences, held.reductions.power),
        SystemReport::PowerSaver(power) => SystemPreferences {
            reductions: Reductions {
                power,
                ..held.reductions
            },
            ..held
        },
    }
}

/// What a read that timed out gives: no scheme, so the fallback shows until the subscription
/// answers, and the platform's fallback double-click time, as a read with no answer has.
const UNREAD: SystemPreferences = SystemPreferences {
    double_click: platform::DOUBLE_CLICK_FALLBACK,
    ..SystemPreferences::NONE
};

/// `mundy`'s answers, beside `power`, which `mundy` does not read.
fn from_mundy(preferences: Preferences, power: Reduction) -> SystemPreferences {
    SystemPreferences {
        reductions: Reductions {
            transparency: Reduction::of(
                preferences.reduced_transparency == mundy::ReducedTransparency::Reduce,
            ),
            motion: Reduction::of(preferences.reduced_motion == mundy::ReducedMotion::Reduce),
            power,
        },
        ..from_answers(
            preferences.color_scheme,
            preferences.double_click_interval.0,
        )
    }
}

/// The settings from `mundy`'s scheme and double-click answers, with no reduction asked, which
/// [`from_mundy`] replaces. No preference reports no scheme, so `System` shows the dark fallback:
/// `mundy` answers it for a failed read too, as for a Linux portal that fails while the host runs,
/// which must not turn the app light. Cost: GNOME writes it for its own Light setting, so GNOME's
/// light users see dark until they pick Light. A double-click time of zero, or none, is the
/// platform's fallback.
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
    use crate::host::winit::system_preferences::SystemReport;
    use crate::host::winit::{platform, system_preferences};
    use crate::primitives::reduction::{Reduction, Reductions};
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
                    reductions: Reductions::NONE,
                },
                "{scheme:?}, {double_click:?}"
            );
        }
    }

    /// Each source's report replaces its own part and keeps the other's: `mundy`'s answers, light
    /// and less motion, leave the power-saver mode on, and the mode's report then leaves the scheme
    /// and the motion as they were.
    #[test]
    fn each_report_replaces_its_own_part() {
        let mut desktop = mundy::Preferences::default();
        desktop.color_scheme = mundy::ColorScheme::Light;
        desktop.reduced_motion = mundy::ReducedMotion::Reduce;
        let saving = SystemPreferences {
            reductions: Reductions {
                power: Reduction::Reduced,
                ..Reductions::NONE
            },
            ..SystemPreferences::NONE
        };
        let light = system_preferences::folded(saving, SystemReport::Desktop(desktop));
        assert_eq!(
            light,
            SystemPreferences {
                color_scheme: Some(ColorScheme::Light),
                double_click: platform::DOUBLE_CLICK_FALLBACK,
                reductions: Reductions {
                    transparency: Reduction::Unreduced,
                    motion: Reduction::Reduced,
                    power: Reduction::Reduced,
                },
            }
        );
        assert_eq!(
            system_preferences::folded(light, SystemReport::PowerSaver(Reduction::Unreduced)),
            SystemPreferences {
                reductions: Reductions {
                    power: Reduction::Unreduced,
                    ..light.reductions
                },
                ..light
            }
        );
    }
}
