//! Schedule-walk benchmark: CPU cost of turning one `RenderBuffer` into
//! the ordered `RenderStep` stream `WgpuBackend::render_groups`
//! dispatches, over frames with many mixed groups.
//!
//! The walk emits `SetScissor` / `SetStencilRef` as *transitions* — one
//! only where the requested state differs from what it already
//! established. Each workload below isolates one shape of that:
//!
//! - `distinct_scissors` — every group carries its own rect, so nothing
//!   can be elided. The control: it measures what the tracking costs
//!   when it never pays off.
//! - `shared_scissor` — every group carries the same rect (an unclipped
//!   frame, or one clip split across quad-budget flushes).
//! - `quads_then_image` — each group draws quads and an image batch with
//!   no text between them, so the group re-requests the scissor it
//!   already holds. The case the renderer review flagged.
//! - `text_then_image` — the same, plus a text batch whose wider scissor
//!   really does move the rect. Counter-control: every request here is a
//!   genuine transition, so the stream must not shrink.
//! - `rounded_chain` — the stencil path with one shared mask chain, where
//!   the clear / re-stamp elision already applies and the scissor
//!   requests around it collapse too.
//!
//! Walks run at full repaint (`damage = None`): the damage opener adds a
//! fixed two steps per walk and would only dilute the per-group signal.
//! Step and scissor counts print once per workload as the secondary
//! metric; wall time is the decision metric.

use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::backend::schedule::internals::Walk;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::batch::{DrawGroup, GroupBatch, TextBatch};
use crate::renderer::render_buffer::{RenderBuffer, RoundedClip};
use criterion::{BenchmarkId, Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::hint::black_box;
use std::time::Duration;

const VIEWPORT: u32 = 1024;

#[derive(Clone, Copy, Debug)]
enum Workload {
    DistinctScissors,
    SharedScissor,
    QuadsThenImage,
    TextThenImage,
    RoundedChain,
}

impl Workload {
    const ALL: [Self; 5] = [
        Self::DistinctScissors,
        Self::SharedScissor,
        Self::QuadsThenImage,
        Self::TextThenImage,
        Self::RoundedChain,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::DistinctScissors => "distinct_scissors",
            Self::SharedScissor => "shared_scissor",
            Self::QuadsThenImage => "quads_then_image",
            Self::TextThenImage => "text_then_image",
            Self::RoundedChain => "rounded_chain",
        }
    }

    const fn use_stencil(self) -> bool {
        matches!(self, Self::RoundedChain)
    }

    /// One rect per group. Distinct rects walk down the viewport in
    /// 8px bands (wrapping well inside it); the shared variants hand
    /// every group the same band so consecutive requests match.
    fn scissor(self, group: usize) -> URect {
        match self {
            Self::DistinctScissors | Self::QuadsThenImage | Self::TextThenImage => {
                URect::new(0, (group as u32 * 8) % (VIEWPORT - 8), VIEWPORT, 8)
            }
            Self::SharedScissor | Self::RoundedChain => URect::new(0, 0, VIEWPORT, 8),
        }
    }

    fn fixture(self, groups: usize) -> RenderBuffer {
        assert!(groups > 0);
        let mut buffer = RenderBuffer::new();
        buffer.viewport_phys = UVec2::splat(VIEWPORT);
        buffer.viewport_phys_f = Vec2::splat(VIEWPORT as f32);
        buffer.scale = 1.0;
        buffer.quads = vec![Quad::default(); groups];
        if self.use_stencil() {
            buffer.rounded_clips = vec![RoundedClip {
                mask_rect: Rect {
                    min: Vec2::ZERO,
                    size: Size::new(VIEWPORT as f32, 8.0),
                },
                corners: Corners::all(4.0),
            }];
        }
        let chain = if self.use_stencil() {
            Span::new(0, 1)
        } else {
            Span::default()
        };
        for group in 0..groups {
            let scissor = self.scissor(group);
            buffer.groups.push(DrawGroup {
                scissor: Some(scissor),
                rounded_clips: chain,
                quads: Span::new(group as u32, 1),
            });
            match self {
                Self::QuadsThenImage | Self::TextThenImage => {
                    buffer.image_batches.push(GroupBatch {
                        items: Span::new(group as u32, 1),
                        last_group: group as u32,
                    });
                }
                Self::DistinctScissors | Self::SharedScissor | Self::RoundedChain => {}
            }
            if matches!(self, Self::TextThenImage) {
                // Wider than the group's band, the way a composer text
                // batch's bounds union exceeds a clipped owner's rect.
                buffer.text_batches.push(TextBatch {
                    texts: Span::new(group as u32, 1),
                    last_group: group as u32,
                    scissor: URect::new(0, 0, VIEWPORT, VIEWPORT),
                    rounded_clips: chain,
                });
            }
        }
        buffer
    }
}

pub(crate) fn bench(c: &mut Criterion, _: crate::bench::Run<'_>) {
    let mut group = c.benchmark_group("schedule/walk");
    group.sample_size(50);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for workload in Workload::ALL {
        for groups in [64, 512] {
            let buffer = workload.fixture(groups);
            let walk = Walk::new(&buffer);
            let counts = walk.run(&buffer, None, workload.use_stencil());
            eprintln!(
                "[schedule] {}/{groups} steps={} scissors={} stencil_refs={}",
                workload.label(),
                counts.steps,
                counts.scissors,
                counts.stencil_refs,
            );
            group.throughput(Throughput::Elements(groups as u64));
            group.bench_with_input(
                BenchmarkId::new(workload.label(), groups),
                &groups,
                |b, _| {
                    b.iter(|| black_box(walk.run(&buffer, None, workload.use_stencil())));
                },
            );
        }
    }
    group.finish();
}
