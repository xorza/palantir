//! The offscreen host every audit that needs the renderer draws through.
//!
//! Two callers with different reasons. `gates.rs` renders the whole frame
//! bench tree to ask what a full-scale frame costs; `fixtures/renderer.rs`
//! renders one shape kind at a time to ask what that kind costs per
//! shape. Both need a device, because `Ui::frame` stops at damage and the
//! encode and compose passes live behind a `Frontend`.
//!
//! Written once because every caller has to agree on the target's format,
//! usage and clear colour. Those decide how much submission work a frame
//! owes, and a caller that differed would be measuring against a floor
//! nobody else's number shares.

use glam::UVec2;
use palantir::internals::{HeadlessTestGpuLease, RecordApp};
use palantir::{Color, OffscreenHost, Ui};

/// One offscreen host and the texture it draws into.
#[derive(Debug)]
pub(crate) struct OffscreenTarget {
    host: OffscreenHost,
    texture: wgpu::Texture,
}

impl OffscreenTarget {
    /// The public offscreen path always copies from its backbuffer, so
    /// what every caller pins excludes the direct-present path.
    pub(crate) fn new(gpu: &HeadlessTestGpuLease, label: &str, surface: UVec2) -> Self {
        let mut host = OffscreenHost::builder(gpu.device.clone(), gpu.queue.clone()).build();
        host.ui().theme_mut().window_clear = Color::TRANSPARENT;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: surface.x,
                height: surface.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Self { host, texture }
    }

    /// One frame, drained before it returns.
    ///
    /// Draining here is what puts GPU execution inside the frame that
    /// submitted it instead of the next one's window.
    pub(crate) fn frame(
        &mut self,
        gpu: &HeadlessTestGpuLease,
        dpr: f32,
        record: impl FnMut(&mut Ui),
    ) {
        self.host
            .frame_offscreen(&self.texture, dpr, &mut RecordApp::new(record));
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll");
    }
}
