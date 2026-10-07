//! What Palantir needs from the device it draws on.

use crate::gpu::error::{self, UnmetRequirements};
use crate::gpu::pipeline::IMMEDIATES_BYTES;

/// The features and limits to ask an adapter for so the returned device can run
/// Palantir's pipelines.
///
/// Published because an embedding application owns its device and Palantir's
/// needs are one contributor to its request. [`negotiate`](Self::negotiate)
/// returns what to fold into a `DeviceDescriptor`; every shipped host uses it,
/// so the requirement is stated once.
#[derive(Clone, Debug)]
pub struct DeviceRequirements {
    /// Palantir's own plus the optional ones the adapter has. Union with the
    /// caller's before requesting.
    pub features: wgpu::Features,
    /// Resolved against the adapter, so requestable as they stand. Only what
    /// Palantir's pipelines consume; a caller drawing its own work raises them with
    /// `or_better_values_from`.
    pub limits: wgpu::Limits,
}

impl DeviceRequirements {
    /// Features no configuration runs without.
    pub const FEATURES: wgpu::Features = wgpu::Features::IMMEDIATES;

    /// The four features `collect_gpu_stats` asks for, as the one set that
    /// instruments the GPU timeline.
    ///
    /// Each degrades on its own: [`Self::negotiate`] intersects them with the
    /// adapter, and `WgpuBackend::new` tests each bit to decide how much
    /// attribution to offer. Every caller must ask for this same set, or the
    /// backend reads a bit nobody requested.
    pub const GPU_TIMING_FEATURES: wgpu::Features = wgpu::Features::TIMESTAMP_QUERY
        .union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES)
        .union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)
        .union(wgpu::Features::PIPELINE_STATISTICS_QUERY);

    /// What to request from `adapter`, given `optional` features the caller would
    /// take if available. Optional features are intersected with the adapter's, so
    /// each degrades alone; those in [`FEATURES`](Self::FEATURES) are mandatory and
    /// their absence is an error.
    pub fn negotiate(
        adapter: &wgpu::Adapter,
        optional: wgpu::Features,
    ) -> Result<Self, UnmetRequirements> {
        Self::against(adapter.features(), &adapter.limits(), optional)
    }

    /// The two conditions Palantir cannot draw without: its mandatory features and
    /// the immediate-region bytes the viewport rides in.
    ///
    /// Shared by [`Self::against`] and [`Self::met_by`] so a failure reports under
    /// one name.
    fn check(available: wgpu::Features, limits: &wgpu::Limits) -> Result<(), UnmetRequirements> {
        if !available.contains(Self::FEATURES) {
            return Err(UnmetRequirements::Features {
                missing: error::flag_names((Self::FEATURES - available).iter_names()),
            });
        }
        if limits.max_immediate_size < IMMEDIATES_BYTES {
            return Err(UnmetRequirements::Limit {
                name: IMMEDIATE_SIZE_LIMIT,
                required: u64::from(IMMEDIATES_BYTES),
                available: u64::from(limits.max_immediate_size),
            });
        }
        Ok(())
    }

    /// The negotiation itself, against a capability pair rather than an adapter,
    /// so it is answerable without a GPU.
    pub(crate) fn against(
        available: wgpu::Features,
        ceiling: &wgpu::Limits,
        optional: wgpu::Features,
    ) -> Result<Self, UnmetRequirements> {
        Self::check(available, ceiling)?;

        // The GLES-3 baseline rather than `Limits::default()`, which demands 16
        // inter-stage variables where `curve_pipeline/shader.wgsl`, the busiest shader,
        // declares 10; a Raspberry Pi's V3D reports 15. The pipelines clear the rest of
        // the baseline with room to spare.
        //
        // Resolution comes from the adapter: the swapchain, `GpuView` targets and
        // atlases are capped by `max_texture_dimension_2d`, and the baseline's 2048
        // would cap the window.
        let limits = wgpu::Limits {
            max_immediate_size: IMMEDIATES_BYTES,
            ..wgpu::Limits::downlevel_defaults().using_resolution(ceiling.clone())
        };

        let mut unmet = None;
        limits.check_limits_with_fail_fn(ceiling, true, |name, required, available| {
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

    /// Whether a device already in hand can run Palantir, for hosts built on a
    /// caller-supplied device.
    pub fn met_by(device: &wgpu::Device) -> Result<(), UnmetRequirements> {
        Self::check(device.features(), &device.limits())
    }
}

/// wgpu's name for the limit as `check_limits_with_fail_fn` reports it (wgpu
/// publishes no constant), so [`DeviceRequirements::check`] messages match the
/// whole-`Limits` sweep.
const IMMEDIATE_SIZE_LIMIT: &str = "max_immediate_size";

#[cfg(test)]
mod tests {
    use wgpu::{Features, Limits};

    use crate::gpu::device::device_requirements::DeviceRequirements;
    use crate::gpu::error::UnmetRequirements;
    use crate::gpu::pipeline::IMMEDIATES_BYTES;

    #[test]
    fn negotiation_holds_the_hard_line_and_lets_the_rest_degrade() {
        let ceiling = Limits {
            max_immediate_size: IMMEDIATES_BYTES,
            ..Limits::default()
        };
        let available = Features::IMMEDIATES | Features::TIMESTAMP_QUERY;
        let optional = Features::TIMESTAMP_QUERY | Features::PIPELINE_STATISTICS_QUERY;

        // Only the optional features the adapter has come along.
        let requirements =
            DeviceRequirements::against(available, &ceiling.clone(), optional).unwrap();
        assert_eq!(
            requirements.features,
            Features::IMMEDIATES | Features::TIMESTAMP_QUERY
        );
        assert_eq!(requirements.limits.max_immediate_size, IMMEDIATES_BYTES);

        let missing =
            DeviceRequirements::against(Features::empty(), &ceiling.clone(), optional).unwrap_err();
        let UnmetRequirements::Features { missing } = &missing else {
            panic!("{missing:?}");
        };
        assert!(
            missing.contains("IMMEDIATES"),
            "the message must name the feature that is absent, got {missing}"
        );

        let mut short = ceiling;
        short.max_immediate_size = IMMEDIATES_BYTES - 1;
        let unmet = DeviceRequirements::against(Features::IMMEDIATES, &short, Features::empty())
            .unwrap_err();
        assert_eq!(
            unmet,
            UnmetRequirements::Limit {
                name: "max_immediate_size",
                required: 8,
                available: 7,
            }
        );
        assert_eq!(
            unmet.to_string(),
            "graphics device limit max_immediate_size is 7, but Palantir requires 8"
        );
    }

    #[test]
    fn an_adapter_under_wgpu_defaults_still_negotiates() {
        // A Raspberry Pi's V3D: one inter-stage variable and 512 texture pixels short
        // of `Limits::default()`.
        let ceiling = Limits {
            max_texture_dimension_1d: 7680,
            max_texture_dimension_2d: 7680,
            max_texture_dimension_3d: 7680,
            max_inter_stage_shader_variables: 15,
            max_immediate_size: 256,
            ..Limits::default()
        };
        assert!(
            !Limits::default().check_limits(&ceiling),
            "fixture no longer stands for an adapter the wgpu defaults fail on"
        );

        let requirements =
            DeviceRequirements::against(Features::IMMEDIATES, &ceiling.clone(), Features::empty())
                .unwrap();

        assert!(requirements.limits.check_limits(&ceiling));
        assert_eq!(requirements.limits.max_inter_stage_shader_variables, 15);
        assert_eq!(requirements.limits.max_immediate_size, IMMEDIATES_BYTES);
        assert_eq!(requirements.limits.max_texture_dimension_2d, 7680);
        assert_eq!(requirements.limits.max_texture_dimension_3d, 7680);
    }
}
