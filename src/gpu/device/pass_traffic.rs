//! What a device moves between memory and its tiles for a render pass, which decides what a
//! partial frame costs.

use crate::gpu::device::tiler_family::TilerFamily;

/// What a render pass moves between memory and the device for its attachments. No API reports
/// it, so it is read off the device's family, as ANGLE classifies Vulkan devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PassTraffic {
    /// What the pass's draws touch: an immediate-mode GPU, and Apple's tile-based ones, on which
    /// a pass that keeps all of a 2560 × 1440 target but a small damage takes 32 µs on the Apple laptop
    /// (`atomic/main_pass/gpu/partial`).
    Drawn,
    /// Every tile of the attachments, loaded before the pass and stored after it: the tile-based
    /// GPUs ANGLE lists, Apple's aside. The same pass takes a VideoCore GPU 3.8 ms.
    WholeTarget,
}

impl PassTraffic {
    /// What a pass on the adapter `info` describes moves: the whole target on a
    /// [`TilerFamily`].
    pub(crate) fn of(info: &wgpu::AdapterInfo) -> Self {
        match TilerFamily::of(info) {
            Some(_) => Self::WholeTarget,
            None => Self::Drawn,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::pass_traffic::PassTraffic;

    /// Arm, Imagination, Qualcomm and Broadcom move the whole target; AMD, Nvidia, Intel, Apple
    /// and a CPU device what is drawn, as does Metal, which reports no vendor and a name of no
    /// tiler. A GL Mali, known by its name alone, moves the whole target.
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
        let gl_mali = wgpu::AdapterInfo {
            name: "Mali-G610".to_owned(),
            ..info(wgpu::Backend::Gl, 0)
        };
        assert_eq!(PassTraffic::of(&gl_mali), PassTraffic::WholeTarget);
    }
}
