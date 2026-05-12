//! Shared viewport uniform buffer. `QuadPipeline` and `MeshPipeline`
//! each reference this single buffer in their (otherwise distinct)
//! bind groups, so one `queue.write_buffer` per frame syncs both.

use encase::{ShaderSize, ShaderType, UniformBuffer};
use glam::Vec2;
use wgpu::util::DeviceExt;

#[derive(Copy, Clone, Debug, ShaderType)]
struct ViewportUniformData {
    size: Vec2,
}

impl ViewportUniformData {
    const BYTES: usize = Self::SHADER_SIZE.get() as usize;

    fn encode(&self) -> [u8; Self::BYTES] {
        let mut out = [0u8; Self::BYTES];
        UniformBuffer::new(&mut out[..]).write(self).unwrap();
        out
    }
}

pub(crate) struct ViewportUniform {
    buffer: wgpu::Buffer,
    /// Last size uploaded. The uniform is initialized to `Vec2::ZERO`
    /// at construction; the first non-zero `write` will mismatch and
    /// upload. Tracking this avoids a per-frame `queue.write_buffer`
    /// when the viewport hasn't actually changed (steady state).
    last: Vec2,
}

impl ViewportUniform {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("palantir.viewport"),
            contents: &ViewportUniformData { size: Vec2::ZERO }.encode(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            buffer,
            last: Vec2::ZERO,
        }
    }

    pub(crate) fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub(crate) fn write(&mut self, queue: &wgpu::Queue, size: Vec2) {
        if self.last == size {
            return;
        }
        queue.write_buffer(&self.buffer, 0, &ViewportUniformData { size }.encode());
        self.last = size;
    }
}
