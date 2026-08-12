//! What Palantir needs from the device it draws on.

use crate::host::error::UnmetRequirements;

/// The features and limits to ask an adapter for, so that the device it
/// returns can run Palantir's pipelines.
///
/// Published rather than applied privately because an embedding application
/// owns its device: it has its own features to request, and Palantir's are
/// only one contributor to that set. [`negotiate`](Self::negotiate) hands back
/// what to fold into a `DeviceDescriptor`; every host Palantir ships builds
/// its device through it, so there is one statement of the requirement rather
/// than one per host that drifts from the others.
#[derive(Clone, Debug)]
pub struct DeviceRequirements {
    /// Ask for exactly these.
    pub features: wgpu::Features,
    /// Ask for exactly these.
    pub limits: wgpu::Limits,
}

impl DeviceRequirements {
    /// Features no configuration runs without.
    pub const FEATURES: wgpu::Features = wgpu::Features::IMMEDIATES;

    /// Immediate-region bytes Palantir needs. This covers
    /// `renderer::backend::text::Params` (a `vec2<u32>`) with WGSL's 16-byte
    /// uniform-struct rounding.
    pub const IMMEDIATE_SIZE: u32 = 16;

    /// What to request from `adapter`, given the `optional` features the
    /// caller would take if they happen to be available.
    ///
    /// Optional features are intersected with what the adapter advertises, so
    /// each degrades on its own rather than failing the request; the ones in
    /// [`FEATURES`](Self::FEATURES) are not negotiable and their absence is an
    /// error.
    pub fn negotiate(
        adapter: &wgpu::Adapter,
        optional: wgpu::Features,
    ) -> Result<Self, UnmetRequirements> {
        Self::against(adapter.features(), adapter.limits(), optional)
    }

    /// The negotiation itself, against a capability pair rather than an
    /// adapter — which is what makes it answerable without a GPU present.
    pub(crate) fn against(
        available: wgpu::Features,
        ceiling: wgpu::Limits,
        optional: wgpu::Features,
    ) -> Result<Self, UnmetRequirements> {
        if !available.contains(Self::FEATURES) {
            return Err(UnmetRequirements::Features {
                required: Self::FEATURES,
                available,
            });
        }

        let mut limits = wgpu::Limits::default().using_resolution(ceiling.clone());
        limits.max_immediate_size = limits.max_immediate_size.max(Self::IMMEDIATE_SIZE);

        let mut unmet = None;
        limits.check_limits_with_fail_fn(&ceiling, true, |name, required, available| {
            unmet = Some(UnmetRequirements::Limit {
                name,
                required,
                available,
            });
        });
        if let Some(unmet) = unmet {
            return Err(unmet);
        }

        Ok(Self {
            features: Self::FEATURES | (available & optional),
            limits,
        })
    }

    /// Whether a device already in hand can run Palantir.
    ///
    /// For hosts built on a caller-supplied device, where the request has
    /// already happened and the only question left is whether it asked for
    /// enough.
    pub fn met_by(device: &wgpu::Device) -> Result<(), UnmetRequirements> {
        let available = device.features();
        if !available.contains(Self::FEATURES) {
            return Err(UnmetRequirements::Features {
                required: Self::FEATURES,
                available,
            });
        }
        let immediate = device.limits().max_immediate_size;
        if immediate < Self::IMMEDIATE_SIZE {
            return Err(UnmetRequirements::Limit {
                name: "max_immediate_size",
                required: u64::from(Self::IMMEDIATE_SIZE),
                available: u64::from(immediate),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use wgpu::{Features, Limits};

    use crate::host::device_requirements::DeviceRequirements;
    use crate::host::error::UnmetRequirements;

    #[test]
    fn negotiation_holds_the_hard_line_and_lets_the_rest_degrade() {
        let ceiling = Limits {
            max_immediate_size: DeviceRequirements::IMMEDIATE_SIZE,
            ..Limits::default()
        };
        let available = Features::IMMEDIATES | Features::TIMESTAMP_QUERY;
        let optional = Features::TIMESTAMP_QUERY | Features::PIPELINE_STATISTICS_QUERY;

        // Only the optional features the adapter actually has come along; the
        // one it lacks is dropped rather than failing the request.
        let requirements =
            DeviceRequirements::against(available, ceiling.clone(), optional).unwrap();
        assert_eq!(
            requirements.features,
            Features::IMMEDIATES | Features::TIMESTAMP_QUERY
        );
        assert_eq!(
            requirements.limits.max_immediate_size,
            DeviceRequirements::IMMEDIATE_SIZE
        );

        // The non-negotiable one is not dropped.
        let missing =
            DeviceRequirements::against(Features::empty(), ceiling.clone(), optional).unwrap_err();
        assert!(
            matches!(
                missing,
                UnmetRequirements::Features { required, available }
                    if required == Features::IMMEDIATES && available.is_empty()
            ),
            "{missing:?}"
        );

        let mut short = ceiling;
        short.max_immediate_size = DeviceRequirements::IMMEDIATE_SIZE - 1;
        let unmet = DeviceRequirements::against(Features::IMMEDIATES, short, Features::empty())
            .unwrap_err();
        assert_eq!(
            unmet,
            UnmetRequirements::Limit {
                name: "max_immediate_size",
                required: 16,
                available: 15,
            }
        );
        assert_eq!(
            unmet.to_string(),
            "graphics device limit max_immediate_size is 15, but Palantir requires 16"
        );
    }
}
