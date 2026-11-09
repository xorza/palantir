use crate::diagnostics::gpu_pass_stats::{BackdropTimes, BatchKind, GpuPassStats};
use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::frame::gpu_timings::query_tracer::{
    BACKDROP_IDX, COPY_OUT_IDX, MAX_TIMESTAMPS, Publisher, QUERY_COUNT, QueryTracer, maps_landed,
};
use crate::gpu::frame::main_segment::MainSegment;
use crate::gpu::surface::map_state::MapState;
use crate::gpu::test_gpu;
use std::time::Duration;

fn ts_bytes(ticks: &[u64]) -> Vec<u8> {
    ticks.iter().flat_map(|t| t.to_le_bytes()).collect()
}

/// A publisher into `sink` at `period_ns` per tick.
fn publisher(period_ns: f32, sink: &mut GpuPassStats) -> Publisher<'_> {
    Publisher { period_ns, sink }
}

#[test]
fn per_kind_publish_distinguishes_absent_zero_and_blank() {
    let mut sink = GpuPassStats::default();

    // Frame 1: timestamps [1000, 1001, 3001, 5001] at 0.5 ns/tick: pass floor(4001 * 0.5) = 2000, quads floor(1 * 0.5) = 0, shadows 1000, text 1000 ns.
    publisher(0.5, &mut sink).timestamps(
        &ts_bytes(&[1000, 1001, 3001, 5001]),
        &[
            Some(BatchKind::Quads),
            Some(BatchKind::Shadows),
            Some(BatchKind::Text),
        ],
        4,
    );
    assert_eq!(sink.last_pass(), Some(Duration::from_micros(2)));
    assert_eq!(sink.last_kind(BatchKind::Quads), Some(Duration::ZERO));
    assert_eq!(
        sink.last_kind(BatchKind::Shadows),
        Some(Duration::from_micros(1))
    );
    assert_eq!(
        sink.last_kind(BatchKind::Text),
        Some(Duration::from_micros(1))
    );
    assert_eq!(sink.last_kind(BatchKind::Mesh), None);

    // Frame 2: begin/end only (a blank window): pass 14000 - 10000 = 4000 ns; per-kind slots clear to None.
    publisher(1.0, &mut sink).timestamps(&ts_bytes(&[10_000, 14_000]), &[], 2);
    assert_eq!(sink.last_pass(), Some(Duration::from_micros(4)));
    for kind in BatchKind::ALL {
        assert_eq!(
            sink.last_kind(kind),
            None,
            "{} stale after blank measured frame",
            kind.label(),
        );
    }

    // Frame 3: two passes of quads around a break of 500 ns: pass 700, quads 100 + 100 = 200 ns,
    // the break in no category.
    publisher(1.0, &mut sink).timestamps(
        &ts_bytes(&[0, 100, 600, 700]),
        &[Some(BatchKind::Quads), None, Some(BatchKind::Quads)],
        4,
    );
    assert_eq!(sink.last_pass(), Some(Duration::from_nanos(700)));
    assert_eq!(
        sink.last_kind(BatchKind::Quads),
        Some(Duration::from_nanos(200))
    );
    assert_eq!(sink.last_kind(BatchKind::Setup), None);
}

/// A frame that copied publishes its copy-out from the two timestamps at `COPY_OUT_IDX`; the next that copied nothing clears it. Ticks 7000 to 7600 at 0.5 ns/tick: `floor(600 * 0.5)` = 300 ns.
#[test]
fn copy_out_publishes_its_two_timestamps_or_clears() {
    let mut sink = GpuPassStats::default();
    let mut ticks = vec![0; QUERY_COUNT as usize];
    ticks[COPY_OUT_IDX as usize] = 7_000;
    ticks[COPY_OUT_IDX as usize + 1] = 7_600;
    let bytes = ts_bytes(&ticks);
    let period_ns = 0.5;
    publisher(period_ns, &mut sink).copy_out(&bytes, true);
    assert_eq!(sink.last_copy_out(), Some(Duration::from_nanos(300)));
    publisher(period_ns, &mut sink).copy_out(&bytes, false);
    assert_eq!(sink.last_copy_out(), None, "a frame with no copy clears it");
}

/// Two breaks' captures and blurs publish as sums of their spans, at 1 ns/tick: captures
/// `(110 − 100) + (230 − 200)` = 40 ns, blurs `(150 − 110) + (290 − 230)` = 100 ns; a frame
/// with none clears both.
#[test]
fn backdrops_publish_their_sums_or_clear() {
    let mut sink = GpuPassStats::default();
    let mut ticks = vec![0; QUERY_COUNT as usize];
    let base = BACKDROP_IDX as usize;
    ticks[base..base + 6].copy_from_slice(&[100, 110, 150, 200, 230, 290]);
    let bytes = ts_bytes(&ticks);
    let period_ns = 1.0;
    publisher(period_ns, &mut sink).backdrops(&bytes, 2);
    assert_eq!(
        sink.last_backdrops(),
        BackdropTimes {
            captures: Some(Duration::from_nanos(40)),
            blurs: Some(Duration::from_nanos(100)),
        }
    );
    publisher(period_ns, &mut sink).backdrops(&bytes, 0);
    assert_eq!(sink.last_backdrops(), BackdropTimes::default());
}

#[test]
fn stats_publish_in_flag_declaration_order() {
    let mut sink = GpuPassStats::default();
    publisher(1.0, &mut sink).stats(&ts_bytes(&[10, 20, 30, 40, 1, 2, 3, 4]), Some(2));
    let s = sink.last_pipeline_stats().expect("published");
    assert_eq!(s.vertex_shader_invocations, 11);
    assert_eq!(s.clipper_invocations, 22);
    assert_eq!(s.clipper_primitives_out, 33);
    assert_eq!(s.fragment_shader_invocations, 44);
    publisher(1.0, &mut sink).stats(&[], None);
    assert_eq!(sink.last_pipeline_stats(), None, "too many passes to sum");
}

#[test]
fn readback_waits_for_every_mapping() {
    let (idle, pending) = (MapState::new(), MapState::never_landing);
    let (mapped, failed) = (|| MapState::landed(true), || MapState::landed(false));
    assert!(!maps_landed(&idle, &idle), "no map began");
    assert!(maps_landed(&mapped(), &idle), "a slot with no statistics");
    assert!(!maps_landed(&pending(), &idle));
    assert!(
        !maps_landed(&mapped(), &pending()),
        "the statistics still map"
    );
    assert!(maps_landed(&mapped(), &mapped()));
    assert!(maps_landed(&failed(), &mapped()), "a failure lands too");
    assert!(maps_landed(&mapped(), &failed()));
}

/// A frame's pass time publishes two frames late: its queries resolve in the next frame, after
/// the queue said the frame is done, and read back once that frame completed. The first frame
/// alone publishes nothing; the frame after it resolves it, and its readback publishes. Skipped
/// on an adapter without timestamps.
#[test]
fn a_frame_publishes_after_the_next_frame_resolved_it() {
    let test_gpu = test_gpu::headless_test_gpu();
    let features = (test_gpu.device.features() & DeviceRequirements::GPU_TIMING_FEATURES)
        .difference(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
    if !features.contains(wgpu::Features::TIMESTAMP_QUERY) {
        return;
    }
    let gpu = &*test_gpu;
    let mut tracer = QueryTracer::new(gpu, gpu.queue.get_timestamp_period(), features);
    let target = test_gpu.target("palantir.test.query_tracer", glam::UVec2::splat(64));
    let mut sink = GpuPassStats::default();
    let frame = |tracer: &mut QueryTracer, sink: &mut GpuPassStats| {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let view = target.view();
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: tracer.pass_writes(MainSegment::Whole),
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        tracer.finish(&mut encoder);
        gpu.queue.submit([encoder.finish()]);
        tracer.after_submit(gpu, sink);
        assert_eq!(gpu.wait_until(&|| tracer.callbacks_landed()), GpuWait::Done);
        tracer.after_submit(gpu, sink);
    };
    frame(&mut tracer, &mut sink);
    assert_eq!(
        sink.last_pass(),
        None,
        "the first frame waits for a later one to resolve it"
    );
    frame(&mut tracer, &mut sink);
    assert!(
        sink.last_pass().is_some(),
        "the second frame resolved the first, which published"
    );
}

/// A main pass broken at a backdrop timestamps the break's start and end, and the segment between
/// is of no kind: First opens at 0, Quads starts at 1, the break starts at 2 and ends at 3, Text
/// starts at 4, and Last closes at 5. Each pass opens a statistics query. A break with no room
/// for its two timestamps and the last voids the kinds. Skipped on an adapter without in-pass
/// timestamps.
#[test]
fn a_break_between_passes_counts_in_no_kind() {
    let test_gpu = test_gpu::headless_test_gpu();
    let features = test_gpu.device.features() & DeviceRequirements::GPU_TIMING_FEATURES;
    if !features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES) {
        return;
    }
    let gpu = &*test_gpu;
    let mut tracer = QueryTracer::new(gpu, gpu.queue.get_timestamp_period(), features);
    let target = test_gpu.target("palantir.test.query_tracer", glam::UVec2::splat(64));
    let view = target.view();
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let segment = |encoder: &mut wgpu::CommandEncoder,
                   tracer: &mut QueryTracer,
                   segment: MainSegment,
                   kinds: &[BatchKind]| {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: tracer.pass_writes(segment),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        tracer.segment_begin(&mut pass, segment);
        for &kind in kinds {
            tracer.mark(&mut pass, kind);
        }
        tracer.segment_end(&mut pass, segment);
    };
    segment(
        &mut encoder,
        &mut tracer,
        MainSegment::First,
        &[BatchKind::Quads],
    );
    segment(
        &mut encoder,
        &mut tracer,
        MainSegment::Last,
        &[BatchKind::Quads, BatchKind::Text],
    );
    assert_eq!(
        tracer.cursor.segment_kinds,
        [
            Some(BatchKind::Setup),
            Some(BatchKind::Quads),
            None,
            Some(BatchKind::Quads),
            Some(BatchKind::Text),
        ]
    );
    assert_eq!(tracer.cursor.next_idx, 6);
    assert_eq!(tracer.cursor.stats_passes, 2);
    assert!(!tracer.cursor.kinds_void);
    tracer.finish(&mut encoder);

    segment(&mut encoder, &mut tracer, MainSegment::First, &[]);
    tracer.cursor.next_idx = MAX_TIMESTAMPS - 2;
    segment(&mut encoder, &mut tracer, MainSegment::Inner, &[]);
    assert!(tracer.cursor.kinds_void, "no room for the break");
    segment(&mut encoder, &mut tracer, MainSegment::Last, &[]);
    assert_eq!(
        tracer.cursor.next_idx, MAX_TIMESTAMPS,
        "the last lands in the last slot"
    );
    let at = tracer.writing.expect("the frame is measured");
    tracer.finish(&mut encoder);
    assert!(tracer.slots[at].segment_kinds.iter().all(Option::is_none));
    assert_eq!(tracer.slots[at].stats_passes, Some(3));
}
