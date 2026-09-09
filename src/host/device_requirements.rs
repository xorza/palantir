//! What Palantir needs from the device it draws on.

use crate::host::error::UnmetRequirements;
use crate::renderer::backend::IMMEDIATES_BYTES;
use std::num::NonZeroU32;

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
    /// Palantir's own, plus whichever optional ones the adapter turned out to
    /// have. Union with the caller's before requesting.
    pub features: wgpu::Features,
    /// Already resolved against the adapter, so these are requestable as they
    /// stand. They are only what Palantir's own pipelines consume, so a caller
    /// that draws its own work on the same device raises them to its needs
    /// with `or_better_values_from` before requesting.
    pub limits: wgpu::Limits,
}

impl DeviceRequirements {
    /// Features no configuration runs without.
    pub const FEATURES: wgpu::Features = wgpu::Features::IMMEDIATES;

    /// The three features `collect_gpu_stats` asks for, as the one set they
    /// mean something as: instrument the GPU timeline.
    ///
    /// Each degrades on its own — [`Self::negotiate`] intersects them with
    /// what the adapter advertises, and `WgpuBackend::new` tests each bit to
    /// decide how much attribution it can offer. What is shared is the
    /// *asking*, and every caller that asks spells the same three or the
    /// backend reads a bit nobody requested.
    pub const GPU_TIMING_FEATURES: wgpu::Features = wgpu::Features::TIMESTAMP_QUERY
        .union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES)
        .union(wgpu::Features::PIPELINE_STATISTICS_QUERY);

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

    /// The two conditions Palantir cannot draw without: its non-negotiable
    /// features, and immediate-region bytes for the text pipeline.
    ///
    /// Both entry points answer it — [`Self::against`] before folding the
    /// rest of a request around it, [`Self::met_by`] on a device where the
    /// request has already happened. Written twice, the immediate-size floor
    /// was named by a string literal on one side and by
    /// `check_limits_with_fail_fn` on the other, so the same failure reported
    /// under two names depending on which door it came through.
    fn check(available: wgpu::Features, limits: &wgpu::Limits) -> Result<(), UnmetRequirements> {
        if !available.contains(Self::FEATURES) {
            return Err(UnmetRequirements::Features {
                required: Self::FEATURES,
                available,
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

    /// The negotiation itself, against a capability pair rather than an
    /// adapter — which is what makes it answerable without a GPU present.
    pub(crate) fn against(
        available: wgpu::Features,
        ceiling: wgpu::Limits,
        optional: wgpu::Features,
    ) -> Result<Self, UnmetRequirements> {
        Self::check(available, &ceiling)?;

        // The GLES-3 baseline rather than `Limits::default()`: the default
        // demands 16 inter-stage shader variables where `curve.wgsl`, the
        // busiest shader here, declares 10 — and a Raspberry Pi's V3D reports
        // 15, so the default cost a device that draws Palantir fine. The
        // pipelines clear the rest of the baseline with room to spare: no
        // compute pass, no storage or uniform buffer, one bind group, two
        // vertex buffers, twelve vertex attributes, one colour attachment.
        //
        // Resolution is the exception and comes from the adapter, since the
        // swapchain, a `GpuView` target and the atlases are all capped by
        // `max_texture_dimension_2d` — the baseline's 2048 would cap the
        // window rather than describe a need.
        let limits = wgpu::Limits {
            max_immediate_size: IMMEDIATES_BYTES,
            ..wgpu::Limits::downlevel_defaults().using_resolution(ceiling.clone())
        };

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

    /// The device's `max_texture_dimension_2d`, which every host has to
    /// read and hand on: it is the ceiling on a registered image, on a
    /// `GpuView` target, and on the glyph and gradient atlases.
    ///
    /// Here rather than at each host, because both spelled the same
    /// `NonZeroU32::new(..).expect(..)` with the same message — and this
    /// is the type that already answers what the device owes Palantir.
    /// A zero limit is not a device Palantir can draw on at all, so it
    /// panics rather than joining [`Self::met_by`]'s `Result`: no adapter
    /// reports one, and threading an error for it would put a match on
    /// every host's startup path for a case that cannot arise.
    pub(crate) fn max_texture_dim(device: &wgpu::Device) -> NonZeroU32 {
        NonZeroU32::new(device.limits().max_texture_dimension_2d)
            .expect("device texture dimension limit is zero")
    }

    /// Whether a device already in hand can run Palantir.
    ///
    /// For hosts built on a caller-supplied device, where the request has
    /// already happened and the only question left is whether it asked for
    /// enough.
    pub fn met_by(device: &wgpu::Device) -> Result<(), UnmetRequirements> {
        Self::check(device.features(), &device.limits())
    }
}

/// wgpu's own name for the limit, as `check_limits_with_fail_fn` reports it.
/// wgpu publishes no constant for it, so this one spelling is what keeps a
/// message from [`DeviceRequirements::check`] reading the same as one from
/// the whole-`Limits` sweep beside it.
const IMMEDIATE_SIZE_LIMIT: &str = "max_immediate_size";

#[cfg(test)]
mod tests {
    use wgpu::{Features, Limits};

    use crate::host::device_requirements::DeviceRequirements;
    use crate::host::error::UnmetRequirements;
    use crate::renderer::backend::IMMEDIATES_BYTES;

    #[test]
    fn negotiation_holds_the_hard_line_and_lets_the_rest_degrade() {
        let ceiling = Limits {
            max_immediate_size: IMMEDIATES_BYTES,
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
        assert_eq!(requirements.limits.max_immediate_size, IMMEDIATES_BYTES);

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
        short.max_immediate_size = IMMEDIATES_BYTES - 1;
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

    #[test]
    fn an_adapter_under_wgpu_defaults_still_negotiates() {
        // A Raspberry Pi's V3D, which is one inter-stage variable and 512
        // texture pixels short of `Limits::default()` and meets every other
        // default.
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
            DeviceRequirements::against(Features::IMMEDIATES, ceiling.clone(), Features::empty())
                .unwrap();

        assert!(requirements.limits.check_limits(&ceiling));
        assert_eq!(requirements.limits.max_inter_stage_shader_variables, 15);
        assert_eq!(requirements.limits.max_immediate_size, IMMEDIATES_BYTES);
        // Resolution is the adapter's, not the baseline's 2048 and 256.
        assert_eq!(requirements.limits.max_texture_dimension_2d, 7680);
        assert_eq!(requirements.limits.max_texture_dimension_3d, 7680);
    }
}
