use crate::display::Display;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::cmd_buffer::payload::{
    DrawCurvePayload, DrawImagePayload, DrawMeshPayload,
};
use crate::renderer::frontend::composer::Composer;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::owner::RenderOwnerId;
use crate::renderer::texture_id::TextureId;
use crate::scene::record_store::RecordPayloads;
use criterion::{BenchmarkId, Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::hint::black_box;
use std::time::Duration;

#[derive(Debug)]
struct ComposeBench {
    cmds: RenderCmdBuffer,
    payloads: RecordPayloads,
    composer: Composer,
    out: RenderBuffer,
    display: Display,
}

impl ComposeBench {
    fn new(cmds: RenderCmdBuffer) -> Self {
        Self {
            cmds,
            payloads: RecordPayloads::default(),
            composer: Composer::new(8192),
            out: RenderBuffer::new(RenderOwnerId::reserve()),
            display: Display::from_physical(UVec2::splat(128), 1.0),
        }
    }

    fn curves(curve_count: usize) -> Self {
        assert!(curve_count > 0);
        let mut cmds = RenderCmdBuffer::default();
        for _ in 0..curve_count {
            cmds.draw_curve(DrawCurvePayload {
                bbox: Rect::new(16.0, 63.0, 96.0, 2.0),
                origin: Vec2::ZERO,
                p0: Vec2::new(16.0, 64.0),
                p1: Vec2::new(48.0, 64.0),
                p2: Vec2::new(80.0, 64.0),
                p3: Vec2::new(112.0, 64.0),
                color: Color::WHITE.into(),
                width: 2.0,
                ..bytemuck::Zeroable::zeroed()
            });
        }
        Self::new(cmds)
    }

    fn compose(&mut self) -> usize {
        self.composer
            .compose(&self.cmds, &self.payloads, self.display, &mut self.out);
        self.out.meshes.len() + self.out.images.len() + self.out.curves.len()
    }
}

#[derive(Clone, Copy, Debug)]
enum HigherKindCase {
    SameTierMesh,
    SameTierImage,
    MixedOverlap,
    MixedNonOverlap,
}

impl HigherKindCase {
    const ALL: [Self; 4] = [
        Self::SameTierMesh,
        Self::SameTierImage,
        Self::MixedOverlap,
        Self::MixedNonOverlap,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::SameTierMesh => "same_tier_mesh",
            Self::SameTierImage => "same_tier_image",
            Self::MixedOverlap => "mixed_overlap",
            Self::MixedNonOverlap => "mixed_non_overlap",
        }
    }

    fn fixture(self, draw_count: usize) -> ComposeBench {
        assert!(draw_count > 0);
        let mut cmds = RenderCmdBuffer::default();
        let overlap = Rect::new(16.0, 16.0, 32.0, 32.0);
        let disjoint = Rect::new(80.0, 80.0, 32.0, 32.0);
        match self {
            Self::SameTierMesh => {
                for _ in 0..draw_count {
                    push_mesh(&mut cmds, overlap);
                }
            }
            Self::SameTierImage => {
                for _ in 0..draw_count {
                    push_image(&mut cmds, overlap);
                }
            }
            Self::MixedOverlap | Self::MixedNonOverlap => {
                assert!(draw_count.is_multiple_of(2));
                let mesh_rect = if matches!(self, Self::MixedOverlap) {
                    overlap
                } else {
                    disjoint
                };
                for _ in 0..draw_count / 2 {
                    push_image(&mut cmds, overlap);
                    push_mesh(&mut cmds, mesh_rect);
                }
            }
        }
        ComposeBench::new(cmds)
    }

    fn expected_groups(self, draw_count: usize) -> usize {
        match self {
            Self::MixedOverlap => draw_count / 2 + 1,
            Self::SameTierMesh | Self::SameTierImage | Self::MixedNonOverlap => 1,
        }
    }
}

fn push_mesh(cmds: &mut RenderCmdBuffer, bbox: Rect) {
    cmds.draw_mesh(DrawMeshPayload {
        bbox,
        origin: Vec2::ZERO,
        tint: Color::WHITE.into(),
        v_start: 0,
        v_len: 3,
        i_start: 0,
        i_len: 3,
        ..bytemuck::Zeroable::zeroed()
    });
}

fn push_image(cmds: &mut RenderCmdBuffer, rect: Rect) {
    cmds.draw_image(DrawImagePayload::image(
        rect,
        Vec2::ZERO,
        Vec2::ONE,
        Color::WHITE.into(),
        TextureId(1),
        0,
    ));
}

pub fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("composer/curves");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for curve_count in [64, 256, 1024, 4096] {
        let mut fixture = ComposeBench::curves(curve_count);
        assert_eq!(fixture.compose(), curve_count);
        group.throughput(Throughput::Elements(curve_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(curve_count),
            &curve_count,
            |b, _| b.iter(|| black_box(fixture.compose())),
        );
    }
    group.finish();

    let mut group = c.benchmark_group("composer/higher_kind_overlap");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for case in HigherKindCase::ALL {
        for draw_count in [64, 256, 1024, 4096] {
            let mut fixture = case.fixture(draw_count);
            assert_eq!(fixture.compose(), draw_count);
            assert_eq!(fixture.out.groups.len(), case.expected_groups(draw_count));
            group.throughput(Throughput::Elements(draw_count as u64));
            group.bench_with_input(
                BenchmarkId::new(case.label(), draw_count),
                &draw_count,
                |b, _| b.iter(|| black_box(fixture.compose())),
            );
        }
    }
    group.finish();
}
