//! What a device moves between memory and its tiles for a render pass, which decides what a
//! partial frame costs.

use crate::gpu::device::vendor;

/// What a render pass moves between memory and the device for its attachments. No API reports
/// it, so it is read off the vendor, as ANGLE classifies Vulkan devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PassTraffic {
    /// What the pass's draws touch: an immediate-mode GPU, and Apple's tile-based ones, on which
    /// a pass that keeps all of a 2560 × 1440 target but a small damage takes 32 µs on the M2
    /// (`atomic/main_pass/gpu/partial`).
    Drawn,
    /// Every tile of the attachments, loaded before the pass and stored after it: the tile-based
    /// GPUs ANGLE lists, Apple's aside. The same pass takes the Pi 5's V3D 3.8 ms.
    WholeTarget,
}

/// Arm Mali, Imagination PowerVR, Qualcomm Adreno and Broadcom VideoCore.
const WHOLE_TARGET_VENDORS: [u32; 4] = [
    vendor::ARM,
    vendor::IMAGINATION,
    vendor::QUALCOMM,
    vendor::BROADCOM,
];

impl PassTraffic {
    /// What a pass on the adapter `info` describes moves.
    pub(crate) fn of(info: &wgpu::AdapterInfo) -> Self {
        if WHOLE_TARGET_VENDORS.contains(&info.vendor) {
            Self::WholeTarget
        } else {
            Self::Drawn
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::pass_traffic::PassTraffic;

    /// Arm, Imagination, Qualcomm and Broadcom move the whole target; AMD, Nvidia, Intel, Apple
    /// and a CPU device what is drawn, as does Metal, which reports no vendor.
    #[test]
    fn the_vendor_decides_what_a_pass_moves() {
        let info = |backend, vendor| wgpu::AdapterInfo {
            vendor,
            ..wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, backend)
        };
        for (vendor, want) in [
            (0x13B5, PassTraffic::WholeTarget),
            (0x1010, PassTraffic::WholeTarget),
            (0x5143, PassTraffic::WholeTarget),
            (0x14E4, PassTraffic::WholeTarget),
            (0x1002, PassTraffic::Drawn),
            (0x10DE, PassTraffic::Drawn),
            (0x8086, PassTraffic::Drawn),
            (0x106B, PassTraffic::Drawn),
            (0x10005, PassTraffic::Drawn),
        ] {
            assert_eq!(
                PassTraffic::of(&info(wgpu::Backend::Vulkan, vendor)),
                want,
                "vendor {vendor:#x}"
            );
        }
        assert_eq!(
            PassTraffic::of(&info(wgpu::Backend::Metal, 0)),
            PassTraffic::Drawn
        );
    }
}
