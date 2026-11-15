//! A window's latest GPU instrumentation sample, which the backend publishes into on the
//! window's submits and the overlay and benches read through the window's `Ui`. Each value is
//! set independently as feature support permits.

use std::time::Duration;

/// Categories of work the per-batch timestamp marker distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BatchKind {
    /// Work before the first drawing step (binds, scissor).
    Setup = 0,
    /// Per-rect clear quad of a Partial pass.
    PreClear = 1,
    /// The quad pipeline.
    Quads = 2,
    /// Drop and inset shadows.
    Shadows = 3,
    /// Text batches.
    Text = 4,
    /// Mesh replay.
    Mesh = 5,
    /// Image replay.
    Image = 6,
    /// Curve replay.
    Curve = 7,
    /// Icon replay.
    Icon = 8,
    /// Effect replay.
    Effect = 9,
}

impl BatchKind {
    /// How many kinds there are.
    pub(crate) const COUNT: usize = 10;

    /// Every kind, in discriminant order.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Setup,
        Self::PreClear,
        Self::Quads,
        Self::Shadows,
        Self::Text,
        Self::Mesh,
        Self::Image,
        Self::Curve,
        Self::Icon,
        Self::Effect,
    ];

    pub(crate) const fn idx(self) -> usize {
        self as u8 as usize
    }

    /// Lowercased variant name, for overlays and reporters.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::PreClear => "preclear",
            Self::Quads => "quads",
            Self::Shadows => "shadows",
            Self::Text => "text",
            Self::Mesh => "mesh",
            Self::Image => "image",
            Self::Curve => "curve",
            Self::Icon => "icon",
            Self::Effect => "effect",
        }
    }
}

// `ALL` lists every discriminant once, in order, so `idx` indexes it.
const _: () = {
    let mut i = 0;
    while i < BatchKind::COUNT {
        assert!(
            BatchKind::ALL[i].idx() == i,
            "BatchKind::ALL must list every discriminant in order",
        );
        i += 1;
    }
    assert!(BatchKind::Effect.idx() + 1 == BatchKind::COUNT);
};

/// Counters surfaced by [`GpuPassStats::last_pipeline_stats`], in
/// `wgpu::PipelineStatisticsTypes` order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipelineStats {
    /// Vertex stage invocations.
    pub vertex_shader_invocations: u64,
    /// Primitives the clipper was handed.
    pub clipper_invocations: u64,
    /// Primitives the clipper emitted.
    pub clipper_primitives_out: u64,
    /// Fragment stage invocations.
    pub fragment_shader_invocations: u64,
}

/// The last frame's backdrop work on the device: the sums over its breaks of their captures and
/// of their blurs, `None` when it timed none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BackdropTimes {
    pub(crate) captures: Option<Duration>,
    pub(crate) blurs: Option<Duration>,
}

/// The calls the last frame's main pass recorded, summed over the passes it breaks into, each
/// counted where the recorder issued it after eliding a repeat: what a change to batching moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    not(feature = "internals"),
    expect(
        unreachable_pub,
        reason = "`pub` for the `internals` re-export, which a build without it lacks"
    )
)]
pub struct PassCounts {
    /// Draws, indexed or not.
    pub draws: u32,
    /// Pipelines set.
    pub pipelines: u32,
    /// Bind groups set.
    pub groups: u32,
    /// Scissor rects set.
    pub scissors: u32,
    /// The viewport pushed into immediate data, again at each change of pipeline layout.
    pub viewports: u32,
    /// Vertex buffers set.
    pub vertex_buffers: u32,
    /// Index buffers set.
    pub index_buffers: u32,
}

impl PassCounts {
    /// No call.
    pub(crate) const ZERO: Self = Self {
        draws: 0,
        pipelines: 0,
        groups: 0,
        scissors: 0,
        viewports: 0,
        vertex_buffers: 0,
        index_buffers: 0,
    };

    /// Add `other`'s calls to these.
    pub(crate) const fn add(&mut self, other: Self) {
        self.draws += other.draws;
        self.pipelines += other.pipelines;
        self.groups += other.groups;
        self.scissors += other.scissors;
        self.viewports += other.viewports;
        self.vertex_buffers += other.vertex_buffers;
        self.index_buffers += other.index_buffers;
    }
}

/// A window's latest GPU sample, each value `None` until the first readback that sets it. A
/// clone is a snapshot.
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuPassStats {
    /// [`Self::last_pass`].
    pub(crate) pass: Option<Duration>,
    /// [`Self::last_kind`], by [`BatchKind::idx`].
    pub(crate) kinds: [Option<Duration>; BatchKind::COUNT],
    /// [`Self::last_pipeline_stats`].
    pub(crate) stats: Option<PipelineStats>,
    /// [`Self::last_copy_out`].
    pub(crate) copy_out: Option<Duration>,
    /// The backdrop captures and blurs of the last frame that painted.
    pub(crate) backdrops: BackdropTimes,
    /// [`Self::last_main_pass_cpu`].
    pub(crate) main_pass_cpu: Option<Duration>,
    /// The calls the last frame's main pass recorded; zero before the first frame that painted.
    pub(crate) pass_counts: PassCounts,
}

impl GpuPassStats {
    /// Whole-pass duration; `None` until the first resolve or without
    /// `TIMESTAMP_QUERY`. On a frame whose backdrops capture, the main pass
    /// breaks at each, and the duration spans the captures and blurs between.
    pub const fn last_pass(&self) -> Option<Duration> {
        self.pass
    }

    /// Per-category duration; `None` without `TIMESTAMP_QUERY_INSIDE_PASSES` or if
    /// the category did not run last frame. The captures and blurs between the
    /// passes the main pass breaks into count in no category, and a frame of more
    /// breaks than its timestamps hold publishes none.
    pub const fn last_kind(&self, kind: BatchKind) -> Option<Duration> {
        self.kinds[kind.idx()]
    }

    /// Pipeline-statistics counters around the main pass, summed over the passes
    /// it breaks into at backdrops; `None` without `PIPELINE_STATISTICS_QUERY`,
    /// or when the last frame broke it into more than 16 passes.
    pub const fn last_pipeline_stats(&self) -> Option<PipelineStats> {
        self.stats
    }

    /// Backbuffer-to-target copy after the main pass in the last frame that
    /// painted; `None` if it copied nothing or `TIMESTAMP_QUERY_INSIDE_ENCODERS` is
    /// unavailable.
    pub const fn last_copy_out(&self) -> Option<Duration> {
        self.copy_out
    }

    /// Host CPU time the last frame spent recording the main render pass; no
    /// adapter feature gates it, and it scales with draw-step count, not pixels.
    pub const fn last_main_pass_cpu(&self) -> Option<Duration> {
        self.main_pass_cpu
    }
}

#[cfg(test)]
mod tests {
    use crate::diagnostics::gpu_pass_stats::*;

    #[test]
    fn starts_uninit() {
        let s = GpuPassStats::default();
        assert_eq!(s.last_pass(), None);
        assert_eq!(s.last_kind(BatchKind::Quads), None);
        assert_eq!(s.last_pipeline_stats(), None);
        assert_eq!(s.last_copy_out(), None);
        assert_eq!(s.last_main_pass_cpu(), None);
    }

    #[test]
    fn labels_match_lowercased_variant_names() {
        for kind in BatchKind::ALL {
            assert_eq!(kind.label(), format!("{kind:?}").to_lowercase(), "{kind:?}");
        }
    }
}
