//! Schedule-walk benchmark: CPU cost of turning one `RenderBuffer` into the ordered `RenderStep`
//! stream `WgpuBackend::render_groups` dispatches, over frames with many mixed groups.
//!
//! The walk emits `SetScissor` / `SetStencilRef` as transitions, only where the requested state
//! differs from what is established. Each workload isolates one shape of that:
//!
//! - `distinct_scissors`: every group has its own rect, so nothing can be elided; the control for
//!   what tracking costs when it never pays off.
//! - `shared_scissor`: every group has the same rect (an unclipped frame, or one clip split across
//!   quad-budget flushes).
//! - `quads_then_image`: each group draws quads and an image batch with no text between them, so
//!   the group re-requests the scissor it already holds.
//! - `text_then_image`: the same plus a text batch whose wider scissor really moves the rect;
//!   every request is a genuine transition, so the stream must not shrink.
//! - `rounded_chain` — the stencil path with one shared mask chain, where
//!   the clear / re-stamp elision already applies and the scissor
//!   requests around it collapse too.
//!
//! Walks run at full repaint (`damage = None`): the damage opener adds a fixed two steps per walk and
//! would only dilute the per-group signal. Step and scissor counts print once per workload as the
//! secondary metric; wall time is the decision metric.

use crate::bench::backends::Backend;
use crate::bench::kind::{Kind, Scale};
use crate::common::span::Span;
use crate::display::Display;
use crate::gpu::frame::schedule::internals::Walk;
use crate::gpu::pipeline::stencil_variant::StencilMode;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::urect::URect;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::group_batch::GroupBatch;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::rounded_clip::RoundedClip;
use crate::renderer::render_buffer::text_batch::TextBatch;
use criterion::{Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::hint::black_box;

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

    const fn stencil(self) -> StencilMode {
        match self {
            Self::RoundedChain => StencilMode::Test,
            _ => StencilMode::Off,
        }
    }

    /// One rect per group. Distinct rects walk down the viewport in 8px bands (wrapping well inside it);
    /// the shared variants hand every group the same band so consecutive requests match.
    const fn scissor(self, group: usize) -> URect {
        match self {
            Self::DistinctScissors | Self::QuadsThenImage | Self::TextThenImage => {
                URect::new(0, (group as u32 * 8) % (VIEWPORT - 8), VIEWPORT, 8)
            }
            Self::SharedScissor | Self::RoundedChain => URect::new(0, 0, VIEWPORT, 8),
        }
    }

    /// The scissor requests a walk over `groups` groups emits: one per group where every group
    /// has its own rect, one in all where they share it, and three per group where a wider
    /// text batch sits between a group's quads and its image, each a genuine transition.
    const fn scissors(self, groups: usize) -> usize {
        match self {
            Self::DistinctScissors | Self::QuadsThenImage => groups,
            Self::SharedScissor | Self::RoundedChain => 1,
            Self::TextThenImage => 3 * groups,
        }
    }

    fn fixture(self, groups: usize) -> RenderBuffer {
        assert!(groups > 0);
        let mut buffer = RenderBuffer::new();
        buffer.display = Display::from_physical(UVec2::splat(VIEWPORT), 1.0);
        buffer.quads = vec![Quad::default(); groups];
        if self.stencil() == StencilMode::Test {
            buffer.rounded_clips = vec![RoundedClip {
                mask_rect: Rect {
                    min: Vec2::ZERO,
                    size: Size::new(VIEWPORT as f32, 8.0),
                },
                corners: Corners::all(4.0),
            }];
        }
        let chain = if self.stencil() == StencilMode::Test {
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
                    buffer.batches_mut(PaintTier::Image).push(GroupBatch {
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

/// `atomic/schedule/cpu/walk/<workload>/<groups>`: one render buffer turned into its step
/// stream, the scissor requests checked first. Per group.
pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "schedule", Scale::Sample);
    for workload in Workload::ALL {
        for groups in [64, 512] {
            group.throughput(Throughput::Elements(groups as u64));
            let case = format!("walk/{}/{groups}", workload.label());
            group.bench_function(Backend::Cpu.case(&case), |b| {
                let buffer = workload.fixture(groups);
                let walk = Walk::new(&buffer);
                let counts = walk.run(&buffer, None, workload.stencil());
                assert_eq!(counts.scissors, workload.scissors(groups), "{case}");
                b.iter(|| black_box(walk.run(&buffer, None, workload.stencil())));
            });
        }
    }
    group.finish();
}
