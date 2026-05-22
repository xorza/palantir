//! Viewport: CPU damage-rect → physical scissor math, plus the GPU
//! uniform buffer shared by quad + mesh pipelines.
//! No GPU handles in the math; the uniform is a thin wrapper around
//! `wgpu::Buffer` that skips redundant writes when size hasn't changed.

use super::GpuCtx;
use crate::primitives::rect::Rect;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::ui::damage::region::DAMAGE_RECT_CAP;
use crate::ui::frame_report::RenderPlan;
use encase::{ShaderSize, ShaderType, UniformBuffer};
use glam::Vec2;
use wgpu::util::DeviceExt;

/// Pad the damage scissor by this many physical pixels on every
/// side. Quads and glyphs may anti-alias slightly outside their
/// nominal rect (SDF rounded-rect AA, italic descenders); without
/// padding the scissor would clip the AA fringe and leave a
/// 1-px-hard edge along the damage boundary.
const DAMAGE_AA_PADDING: u32 = 2;

/// Convert a logical-px damage rect to a physical-px scissor, padded
/// by [`DAMAGE_AA_PADDING`] on every side and clamped to the viewport.
/// Returns `None` if the result clamps to zero area — callers degrade
/// that case to "loaded but not drawn" inside the pass.
pub(super) fn logical_rect_to_phys_scissor(r: Rect, buffer: &RenderBuffer) -> Option<URect> {
    let phys = r.scaled_by(buffer.scale, true);
    let pad = DAMAGE_AA_PADDING as f32;
    let mins_x = (phys.min.x - pad).max(0.0) as u32;
    let mins_y = (phys.min.y - pad).max(0.0) as u32;
    let maxs_x = ((phys.min.x + phys.size.w + pad).max(0.0) as u32).min(buffer.viewport_phys.x);
    let maxs_y = ((phys.min.y + phys.size.h + pad).max(0.0) as u32).min(buffer.viewport_phys.y);
    if maxs_x > mins_x && maxs_y > mins_y {
        Some(URect::new(mins_x, mins_y, maxs_x - mins_x, maxs_y - mins_y))
    } else {
        None
    }
}

/// Fill `out` with the per-rect physical-px scissors for this frame.
/// `Full` and `Skip` leave it empty; `Partial(region)` produces one
/// entry per rect after physical-px scaling, AA padding, and viewport
/// clamping — rects that clamp to zero area are filtered out. If every
/// rect clamps to zero, the list ends up empty and the caller degrades
/// the frame to a Full repaint (correct, just wasteful — won't happen
/// in practice unless damage lies entirely outside the surface).
#[profiling::function]
pub(super) fn build_damage_scissors(
    out: &mut tinyvec::ArrayVec<[URect; DAMAGE_RECT_CAP]>,
    plan: RenderPlan,
    buffer: &RenderBuffer,
) {
    out.clear();
    if let RenderPlan::Partial { region, .. } = plan {
        for r in region.iter_rects() {
            if let Some(s) = logical_rect_to_phys_scissor(r, buffer) {
                out.push(s);
            }
        }
    }
}

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

/// Shared viewport uniform — buffer + the single `BindGroupLayout` /
/// `BindGroup` every pipeline references as `@group(0)`. Built once
/// at backend construction; pipelines borrow `bgl` for their pipeline
/// layouts and the backend binds `bg` once per main pass instead of
/// each pipeline calling `set_bind_group(0)` on its own clone.
pub(crate) struct ViewportUniform {
    pub(crate) buffer: wgpu::Buffer,
    /// Last size uploaded. The uniform is initialized to `Vec2::ZERO`
    /// at construction; the first non-zero `write` will mismatch and
    /// upload. Tracking this avoids a per-frame `queue.write_buffer`
    /// when the viewport hasn't actually changed (steady state).
    last: Vec2,
    pub(crate) bgl: wgpu::BindGroupLayout,
    pub(crate) bg: wgpu::BindGroup,
}

impl ViewportUniform {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("palantir.viewport"),
            contents: &ViewportUniformData { size: Vec2::ZERO }.encode(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.viewport.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // Visible to both stages so it serves quad/curve's
                // fragment math too (gradient brushes sample fragment-
                // side and read `vp.size` for normalization).
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.viewport.bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            buffer,
            last: Vec2::ZERO,
            bgl,
            bg,
        }
    }

    pub(crate) fn write(&mut self, ctx: &mut GpuCtx<'_>, size: Vec2) {
        if self.last == size {
            return;
        }
        ctx.write(&self.buffer, 0, &ViewportUniformData { size }.encode());
        self.last = size;
    }
}
