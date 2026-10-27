//! The device features Palantir requires, by Palantir's own names.

use crate::gpu::flag_names::wgpu_mirror;

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

wgpu_mirror!(DeviceFeatures => wgpu::Features {
    IMMEDIATES => wgpu::Features::IMMEDIATES,
});

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
