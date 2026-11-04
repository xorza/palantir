//! [`SystemPreferences`]: what the system's settings say palantir follows, as every host reports
//! them: the winit host reads them from the desktop, a headless host takes them from its builder
//! and its setters.

use crate::primitives::reduce_preference::Reduction;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::resources::UiResources;
use std::time::Duration;

/// The system's color scheme and double-click time, each `None` where the system gives none, and
/// whether it asks for less motion and less transparency. The one input of the system's settings
/// every host writes through [`Self::apply`]. Refused: a headless host writing each setting
/// through its own `Ui` door beside the winit host's value, two routes that drifted, the
/// double-click time reaching only one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SystemPreferences {
    pub(crate) color_scheme: Option<ColorScheme>,
    pub(crate) double_click: Option<Duration>,
    pub(crate) motion: Reduction,
    pub(crate) transparency: Reduction,
}

impl SystemPreferences {
    /// What a system that reports nothing says: no scheme, so the fallback shows, the default
    /// double-click time, full motion and transparency.
    pub(crate) const NONE: Self = Self {
        color_scheme: None,
        double_click: None,
        motion: Reduction::Full,
        transparency: Reduction::Full,
    };

    /// Write the settings into the host's shared cells: the scheme, the motion and the
    /// transparency always, as an absent scheme is an answer; the double-click time only when the
    /// system gave one. Returns whether any moved; the host's signal
    /// ([`AppSettings::take_change`](crate::ui::resources::app_settings::AppSettings::take_change))
    /// tells every window too.
    pub(crate) fn apply(self, resources: &UiResources) -> bool {
        let settings = resources.settings();
        let scheme = settings.set_system_color_scheme(self.color_scheme);
        let motion = settings.set_system_motion(self.motion);
        let transparency = settings.set_system_transparency(self.transparency);
        let double_click = self
            .double_click
            .is_some_and(|interval| settings.set_double_click_interval(interval));
        scheme || motion || transparency || double_click
    }
}

#[cfg(test)]
mod tests {
    use crate::host::system_preferences::SystemPreferences;
    use crate::input_state::capture::DOUBLE_CLICK_INTERVAL;
    use crate::primitives::reduce_preference::Reduction;
    use crate::theme::color_scheme::ColorScheme;
    use crate::ui::resources::UiResources;
    use std::time::Duration;

    /// Applying writes the scheme, the motion and the transparency always, an absent scheme
    /// included, and a double-click time only when one came: a report with none keeps the time an
    /// earlier one set. Each apply answers whether it moved a setting, and the same report again
    /// moves none.
    #[test]
    fn applying_writes_the_scheme_and_keeps_an_unreported_double_click_time() {
        let resources = UiResources::isolated_mono();
        assert_eq!(
            resources.settings().double_click_interval(),
            DOUBLE_CLICK_INTERVAL
        );
        let ms = Duration::from_millis;
        let reported = SystemPreferences {
            color_scheme: Some(ColorScheme::Light),
            double_click: Some(ms(250)),
            motion: Reduction::Reduced,
            transparency: Reduction::Reduced,
        };
        assert!(reported.apply(&resources));
        assert!(!reported.apply(&resources), "the same report again");
        assert_eq!(
            resources.settings().system_color_scheme(),
            Some(ColorScheme::Light)
        );
        assert_eq!(resources.settings().double_click_interval(), ms(250));
        assert_eq!(resources.settings().system_motion(), Reduction::Reduced);
        assert_eq!(
            resources.settings().system_transparency(),
            Reduction::Reduced
        );

        assert!(SystemPreferences::NONE.apply(&resources));
        assert_eq!(resources.settings().system_color_scheme(), None);
        assert_eq!(resources.settings().double_click_interval(), ms(250));
        assert_eq!(resources.settings().system_motion(), Reduction::Full);
        assert_eq!(resources.settings().system_transparency(), Reduction::Full);
    }
}
