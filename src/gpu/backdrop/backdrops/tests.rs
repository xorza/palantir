use crate::display::Display;
use crate::gpu::backdrop::backdrops::{BackdropFrame, BackdropSlot, Backdrops, Kept, held_clear};
use crate::gpu::backdrop::tests::CLEAR;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
use crate::gpu::surface::render_target::TargetFormat;
use crate::gpu::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::{BackdropRow, PaneSource};
use crate::renderer::render_owner_id::{RenderOwnerId, RenderOwners};
use crate::shape::paint::blur_plan::BlurPlan;
use glam::UVec2;
use std::iter;
use std::mem::offset_of;
use wgpu::util::StagingBelt;

const SURFACE: UVec2 = UVec2::new(200, 100);

fn key(i: u32) -> PaneKey {
    PaneKey {
        owner: WidgetId::from_hash("pane"),
        shape: i,
    }
}

/// Pane `i` over x `10 + 40 i ..`, at σ 2, taking its result from `source`.
fn row(i: u32, source: PaneSource) -> BackdropRow {
    BackdropRow {
        source,
        ..BackdropRow::fixture(i, URect::new(10 + 40 * i, 10, 30, 30), BlurPlan::new(2.0))
    }
}

/// [`row`] capturing after `i + 1` groups.
fn captured(i: u32) -> BackdropRow {
    row(i, PaneSource::Capture { barrier: i + 1 })
}

/// Prepare one frame of `owner` holding `rows`.
fn prepare(
    backdrops: &mut Backdrops,
    gpu: &HeadlessTestGpuLease,
    owner: RenderOwnerId,
    rows: &[BackdropRow],
) {
    let mut buffer = RenderBuffer::new();
    buffer.display = Display::from_physical(SURFACE, 1.0);
    buffer.backdrops.extend_from_slice(rows);
    buffer.live_panes.extend(rows.iter().map(|row| row.key));
    let handles = gpu.handles();
    let mut belt = StagingBelt::new(gpu.device.clone(), 1 << 12);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut ctx = GpuCtx::new(&handles, &mut belt, &mut encoder);
        backdrops.prepare(
            &mut ctx,
            BackdropFrame {
                buffer: &buffer,
                owner,
                format: TargetFormat::RGBA8_SRGB,
                clear: CLEAR.to_f16(),
                backbuffer: None,
            },
        );
    }
    belt.finish();
    gpu.queue.submit(iter::once(encoder.finish()));
    belt.recall();
}

/// The atlas slot of `owner`'s pane `i`, if its result is one.
fn slot(backdrops: &Backdrops, owner: RenderOwnerId, i: u32) -> Option<URect> {
    match backdrops.cache.get(&(owner, key(i)))?.kept {
        Kept::Slot(slot) => Some(slot),
        Kept::Clear(_) => None,
    }
}

/// Two windows' panes under one key keep a result each, in slots apart: a frame of one leaves the
/// other's, its panes leaving drops only its own, and retiring a window drops the rest of its.
/// Frames with no pane make no resources; the first with one makes it, and it stays.
#[test]
fn each_window_keeps_its_own_results() {
    let gpu = headless_test_gpu();
    let owners = RenderOwners::default();
    let (a, b) = (owners.reserve(), owners.reserve());
    let mut backdrops = Backdrops::new(&gpu.device);
    prepare(&mut backdrops, &gpu, a, &[]);
    assert!(backdrops.resources.is_none(), "no pane, no resources");
    prepare(&mut backdrops, &gpu, a, &[captured(0)]);
    assert!(backdrops.resources.is_some());
    prepare(&mut backdrops, &gpu, b, &[captured(0)]);
    let (at_a, at_b) = (
        slot(&backdrops, a, 0).unwrap(),
        slot(&backdrops, b, 0).unwrap(),
    );
    assert!(!at_a.intersects(at_b), "{at_a:?} and {at_b:?} apart");
    assert_eq!(backdrops.blurs(), 1);

    prepare(&mut backdrops, &gpu, a, &[row(0, PaneSource::Held)]);
    assert_eq!(backdrops.blurs(), 0);
    assert_eq!(slot(&backdrops, a, 0), Some(at_a), "held");
    prepare(&mut backdrops, &gpu, a, &[]);
    assert!(!backdrops.cache.contains_key(&(a, key(0))), "left");
    assert_eq!(slot(&backdrops, b, 0), Some(at_b), "the other window's");
    backdrops.retire(b);
    assert!(backdrops.cache.is_empty(), "retired");
    assert!(backdrops.resources.is_some(), "kept for the next pane");
}

/// A pane that shows the clear holds the clear as the target holds it and frees its slot, which
/// the next capture of the same size takes; a held pane of the clear keeps it without a blur.
#[test]
fn a_pane_of_the_clear_frees_its_slot() {
    let gpu = headless_test_gpu();
    let owner = RenderOwners::default().reserve();
    let mut backdrops = Backdrops::new(&gpu.device);
    prepare(&mut backdrops, &gpu, owner, &[captured(0)]);
    let first = slot(&backdrops, owner, 0).unwrap();
    prepare(&mut backdrops, &gpu, owner, &[row(0, PaneSource::Clear)]);
    assert_eq!(backdrops.blurs(), 0);
    let held = held_clear(CLEAR.to_f16(), TargetFormat::RGBA8_SRGB);
    assert!(matches!(
        backdrops.cache[&(owner, key(0))].kept,
        Kept::Clear(clear) if clear == held
    ));
    prepare(
        &mut backdrops,
        &gpu,
        owner,
        &[row(0, PaneSource::Held), captured(1)],
    );
    assert_eq!(backdrops.blurs(), 1);
    assert!(matches!(
        backdrops.cache[&(owner, key(0))].kept,
        Kept::Clear(_)
    ));
    assert_eq!(slot(&backdrops, owner, 1).map(|s| s.min), Some(first.min));
}

/// An 8-bit sRGB target holds the clear as its byte decoded, and a float one as the `f16` clear:
/// each rounded to `f16`, as the blur stores its levels.
#[test]
fn the_clear_a_pane_shows_is_the_one_the_target_holds() {
    let byte = RgbaF32::from(RgbaF32::from(SrgbaU8::rgb(51, 115, 178)).to_f16());
    assert_eq!(
        held_clear(CLEAR.to_f16(), TargetFormat::RGBA8_SRGB),
        [byte.r, byte.g, byte.b]
    );
    let float = RgbaF32::from(CLEAR.to_f16());
    assert_ne!(float.r, byte.r, "the byte rounds the clear");
    assert_eq!(
        held_clear(
            CLEAR.to_f16(),
            TargetFormat::new(wgpu::TextureFormat::Rgba16Float)
        ),
        [float.r, float.g, float.b]
    );
}

/// `BackdropSlot` lies as the quad shader declares it, which a storage binding reads unchecked:
/// the clear, a `vec3` the shader aligns to 16 bytes, starts after the lens at 32.
#[test]
fn a_backdrop_slot_lies_as_the_shader_reads_it() {
    QuadPipeline::SHADER.assert_struct_layout(
        "BackdropSlot",
        size_of::<BackdropSlot>(),
        &[
            ("offset", offset_of!(BackdropSlot, offset)),
            ("lattice", offset_of!(BackdropSlot, lattice)),
            ("highlight", offset_of!(BackdropSlot, highlight)),
            ("lens", offset_of!(BackdropSlot, lens)),
            ("clear", offset_of!(BackdropSlot, clear)),
            ("max_luminance", offset_of!(BackdropSlot, max_luminance)),
        ],
    );
}

/// At its ceiling the shared atlas evicts the least recently used result no pane of the frame
/// reads, from any window, and lists it for that window's ledger. A 64² atlas holds four 32²
/// slots (30² results rounded up): window a takes two, window b three, so b's third evicts one of
/// a's. A result past the ceiling finds no room at all: its pane shows the clear, blurs nothing,
/// and is listed for its own ledger, which then captures it again.
#[test]
fn a_full_atlas_evicts_the_least_recently_used_result() {
    let gpu = headless_test_gpu();
    let owners = RenderOwners::default();
    let (a, b) = (owners.reserve(), owners.reserve());
    let mut backdrops = Backdrops::with_ceiling(&gpu.device, 64);
    prepare(&mut backdrops, &gpu, a, &[captured(0), captured(1)]);
    assert_eq!(backdrops.evicted(a).count(), 0);
    prepare(
        &mut backdrops,
        &gpu,
        b,
        &[captured(0), captured(1), captured(2)],
    );
    assert_eq!(backdrops.blurs(), 3);
    assert_eq!(backdrops.evicted(a).count(), 1, "one of a's went");
    assert_eq!(
        backdrops.evicted(b).count(),
        0,
        "b's own frame keeps all of b's"
    );
    backdrops.forget_evicted(a);
    assert_eq!(backdrops.evicted(a).count(), 0);

    let huge = BackdropRow {
        source: PaneSource::Capture { barrier: 1 },
        ..BackdropRow::fixture(9, URect::new(0, 0, 100, 100), BlurPlan::new(2.0))
    };
    prepare(&mut backdrops, &gpu, b, &[huge]);
    assert_eq!(backdrops.blurs(), 0);
    assert_eq!(backdrops.slots[0].lattice, 0.0, "it shows the clear");
    assert_eq!(backdrops.evicted(b).collect::<Vec<_>>(), [key(9)]);
}
