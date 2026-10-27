//! [`HostDisplay`]: what a host knows of the surface a frame paints onto.

use crate::display::Display;
use crate::display::user_scale::UserScale;
use glam::UVec2;

/// A [`Display`] without its user scale, which the `Ui` owns and adds at
/// [`Ui::frame_stamp`](crate::ui::Ui::frame_stamp).
#[derive(Clone, Copy, Debug)]
pub(crate) struct HostDisplay {
    pub(crate) physical: UVec2,
    pub(crate) system_scale: f32,
    pub(crate) pixel_snap: bool,
    pub(crate) refresh_millihertz: Option<u32>,
}

impl HostDisplay {
    pub(crate) const fn to_display(self, user_scale: UserScale) -> Display {
        let Self {
            physical,
            system_scale,
            pixel_snap,
            refresh_millihertz,
        } = self;
        Display {
            physical,
            system_scale,
            user_scale,
            pixel_snap,
            refresh_millihertz,
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::display::Display;
    use crate::display::host_display::HostDisplay;

    impl HostDisplay {
        /// `display` without its user scale.
        pub(crate) const fn from_display(display: Display) -> Self {
            let Display {
                physical,
                system_scale,
                user_scale: _,
                pixel_snap,
                refresh_millihertz,
            } = display;
            Self {
                physical,
                system_scale,
                pixel_snap,
                refresh_millihertz,
            }
        }
    }
}
