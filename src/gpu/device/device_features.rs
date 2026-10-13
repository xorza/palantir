//! The device features Palantir requires, by Palantir's own names.

use std::fmt;
use std::fmt::{Display, Formatter};

use crate::gpu::flag_names;

flag_set! {
    /// The device features Palantir cannot draw without, so an
    /// [`UnmetRequirements`](crate::UnmetRequirements) names the missing ones
    /// without a graphics-API type. `ALL` is the whole requirement.
    ///
    /// `Display` writes the graphics API's names, joined by `, `.
    pub struct DeviceFeatures {
        /// Immediate data: the viewport rides in it.
        const IMMEDIATES = 1 << 0;
    }
}

impl DeviceFeatures {
    /// Each flag beside the wgpu feature it stands for; every declared flag appears once.
    const WGPU: [(Self, wgpu::Features); 1] = [(Self::IMMEDIATES, wgpu::Features::IMMEDIATES)];

    pub(crate) const fn to_wgpu(self) -> wgpu::Features {
        let mut features = wgpu::Features::empty();
        let mut i = 0;
        while i < Self::WGPU.len() {
            let (flag, feature) = Self::WGPU[i];
            if self.contains(flag) {
                features = features.union(feature);
            }
            i += 1;
        }
        features
    }

    /// The flags whose features `features` holds; features Palantir does not require drop.
    pub(crate) const fn from_wgpu_truncate(features: wgpu::Features) -> Self {
        let mut set = Self::NONE;
        let mut i = 0;
        while i < Self::WGPU.len() {
            let (flag, feature) = Self::WGPU[i];
            if features.contains(feature) {
                set.insert(flag);
            }
            i += 1;
        }
        set
    }
}

impl Display for DeviceFeatures {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        flag_names::write(f, self.to_wgpu().iter_names())
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::device_features::DeviceFeatures;

    #[test]
    fn maps_to_wgpu_both_ways_and_displays_wgpu_names() {
        assert_eq!(
            DeviceFeatures::ALL.to_wgpu(),
            wgpu::Features::IMMEDIATES,
            "every declared flag maps"
        );
        assert_eq!(DeviceFeatures::NONE.to_wgpu(), wgpu::Features::empty());
        assert_eq!(
            DeviceFeatures::from_wgpu_truncate(
                wgpu::Features::IMMEDIATES | wgpu::Features::TIMESTAMP_QUERY
            ),
            DeviceFeatures::IMMEDIATES,
            "a feature Palantir does not require drops"
        );
        assert_eq!(
            DeviceFeatures::from_wgpu_truncate(wgpu::Features::TIMESTAMP_QUERY),
            DeviceFeatures::NONE
        );

        assert_eq!(DeviceFeatures::IMMEDIATES.to_string(), "IMMEDIATES");
        assert_eq!(DeviceFeatures::NONE.to_string(), "");
    }
}
