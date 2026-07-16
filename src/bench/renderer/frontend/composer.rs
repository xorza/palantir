use crate::display::Display;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::record_store::RecordPayloads;
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::cmd_buffer::payload::DrawCurvePayload;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::owner::RenderOwnerId;
use criterion::{BenchmarkId, Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::hint::black_box;
use std::time::Duration;

#[derive(Debug)]
struct CurveComposeBench {
    cmds: RenderCmdBuffer,
    payloads: RecordPayloads,
    composer: Composer,
    out: RenderBuffer,
    display: Display,
}

impl CurveComposeBench {
    fn new(curve_count: usize) -> Self {
        assert!(
            curve_count > 0,
            "curve benchmark requires at least one curve"
        );
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
        Self {
            cmds,
            payloads: RecordPayloads::default(),
            composer: Composer::new(8192),
            out: RenderBuffer::new(RenderOwnerId::reserve()),
            display: Display::from_physical(UVec2::splat(128), 1.0),
        }
    }

    fn compose(&mut self) -> usize {
        self.composer
            .compose(&self.cmds, &self.payloads, self.display, &mut self.out);
        self.out.curves.len()
    }
}

pub fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("composer/curves");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for curve_count in [64, 256, 1024, 4096] {
        let mut fixture = CurveComposeBench::new(curve_count);
        assert_eq!(fixture.compose(), curve_count);
        group.throughput(Throughput::Elements(curve_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(curve_count),
            &curve_count,
            |b, _| b.iter(|| black_box(fixture.compose())),
        );
    }
    group.finish();
}
