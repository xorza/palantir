//! [`UiPreferences`]: the person's picks over what the system reports.

use crate::primitives::preference::Preference;
use crate::primitives::reduction::Reduction;
use crate::theme::color_scheme::ColorScheme;

/// The person's picks of the settings palantir follows from the system, each the system's
/// answer or a value held by hand. App-global: one plain value the app reads with
/// [`Ui::preferences`](crate::Ui::preferences), writes with
/// [`Ui::set_preferences`](crate::Ui::set_preferences) and persists as it likes, as egui's
/// `Context::options_mut` holds its options in one struct
/// (<https://docs.rs/egui/latest/egui/struct.Context.html#method.options_mut>). Refused: a getter
/// and a setter for each setting on the `Ui`, the surface mirrored per setting.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct UiPreferences {
    /// The scheme every window shows, from its next frame: the frame that sets it keeps its
    /// scheme, so no frame mixes two. The system's is [`ColorScheme::FALLBACK`] where nothing
    /// reports one.
    pub scheme: Preference<ColorScheme>,
    /// Whether motion is reduced, at once: a motion that starts or steps after the set runs
    /// under it ([`Ui::is_motion_reduced`](crate::Ui::is_motion_reduced)).
    pub motion: Preference<Reduction>,
    /// Whether transparency is reduced, from every window's next frame
    /// ([`Ui::is_transparency_reduced`](crate::Ui::is_transparency_reduced)).
    pub transparency: Preference<Reduction>,
    /// Whether power use is reduced, from every window's next frame
    /// ([`Ui::is_power_reduced`](crate::Ui::is_power_reduced)).
    pub power: Preference<Reduction>,
}
