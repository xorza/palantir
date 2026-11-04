//! [`PowerPreference`]: which GPU a host asks for.

/// Which GPU a host asks for where a machine has two, as a hybrid laptop's integrated and discrete
/// ones. A palantir type, as [`Vsync`](crate::Vsync) and [`TargetFormat`](crate::TargetFormat)
/// are, so a host builder names no wgpu type; `crate::gpu` converts it to wgpu's. Refused: wgpu's
/// `None`, which leaves the pick to wgpu, where a host says what it wants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PowerPreference {
    /// The GPU that draws least power, an integrated one; a window's default, as a UI idles most
    /// of its life.
    #[default]
    LowPower,
    /// The fastest GPU, a discrete one; the headless and bench paths' pick, as each times or
    /// renders as fast as it can.
    HighPerformance,
}

impl PowerPreference {
    /// Where an adapter of `device_type` stands in this preference, lowest first: the order
    /// wgpu-core 30's `request_adapter` sorts by (`instance.rs`, `get_order`), the GPU the
    /// preference names, then the other kind, then `Other`, which OpenGL reports for any device,
    /// then a virtual GPU, then the CPU.
    pub(crate) const fn rank(self, device_type: wgpu::DeviceType) -> u8 {
        match (self, device_type) {
            (Self::LowPower, wgpu::DeviceType::IntegratedGpu)
            | (Self::HighPerformance, wgpu::DeviceType::DiscreteGpu) => 1,
            (_, wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu) => 2,
            (_, wgpu::DeviceType::Other) => 3,
            (_, wgpu::DeviceType::VirtualGpu) => 4,
            (_, wgpu::DeviceType::Cpu) => 5,
        }
    }
}

impl From<PowerPreference> for wgpu::PowerPreference {
    fn from(preference: PowerPreference) -> Self {
        match preference {
            PowerPreference::LowPower => Self::LowPower,
            PowerPreference::HighPerformance => Self::HighPerformance,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::power_preference::PowerPreference;
    use wgpu::DeviceType;

    /// Each preference ranks its own GPU first, then the other, then OpenGL's `Other`, a virtual
    /// GPU and the CPU, as wgpu-core's sort does.
    #[test]
    fn each_preference_ranks_its_own_gpu_first_and_the_cpu_last() {
        let order = [
            DeviceType::DiscreteGpu,
            DeviceType::IntegratedGpu,
            DeviceType::Other,
            DeviceType::VirtualGpu,
            DeviceType::Cpu,
        ];
        let ranks = |preference: PowerPreference| order.map(|kind| preference.rank(kind));
        assert_eq!(ranks(PowerPreference::HighPerformance), [1, 2, 3, 4, 5]);
        assert_eq!(ranks(PowerPreference::LowPower), [2, 1, 3, 4, 5]);
    }
}
