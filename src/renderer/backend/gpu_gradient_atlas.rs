//! Device-side gradient LUT atlas and its shared CPU source.
//!
//! Owned by [`WgpuBackend`](crate::renderer::backend::WgpuBackend) and lent to the quad and
//! curve pipelines (both render gradient brushes). Keeping the resource
//! here — rather than on whichever pipeline happens to build first —
//! means neither pipeline owns the other's input: each takes `&bgl` at
//! build time and `&bg` at bind time.

use crate::primitives::color::ColorF16;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::texture_binding;
use crate::renderer::backend::texture_region::TextureRegion;
use crate::renderer::gradient_atlas::bake::LUT_ROW_TEXELS;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use glam::UVec2;

/// Bytes per uploaded LUT row: texture width × `Rgba16Float` texel.
/// Derived from the CPU-side `ColorF16` row store
/// (`gradient_atlas::LutRowTexels`) so the GPU upload row-pitch can't
/// silently drift from the texel type the bake writes.
const ROW_PITCH: u32 = (LUT_ROW_TEXELS * size_of::<ColorF16>()) as u32;
// `write_texture`'s `bytes_per_row` must be a multiple of
// `COPY_BYTES_PER_ROW_ALIGNMENT` (256). Guard the row pitch independently
// of the shader assert above so relaxing one can't silently break the
// upload alignment.
const _: () = assert!(
    ROW_PITCH.is_multiple_of(256),
    "gradient atlas row pitch must be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT (256)"
);

/// Shared CPU gradient source plus the texture, sampler, and bind group
/// consumed by the quad and curve pipelines. Format-independent: survives a
/// swapchain format change untouched (only the pipelines carry the target).
#[derive(Debug)]
pub(super) struct GpuGradientAtlas {
    cpu: SharedGradientAtlas,
    /// LUT atlas texture. 256 cols × N rows of `Rgba16Float`
    /// (linear, no sampler decode — the LUT bake stores linear-RGB
    /// directly via `From<Color> for ColorF16`, so the GPU sees
    /// ready-to-blend linear values; see `AGENTS.md` "Colour pipeline").
    /// f16 over 8-bit linear: dark gradient stops linearise to tiny
    /// values, and an 8-bit linear row crushes them onto a handful of
    /// levels (visible banding) — see `gradient_atlas` module docs.
    /// Uploaded each dirty frame by [`Self::upload`], which also
    /// replaces the texture when the CPU atlas grew past its height.
    texture: wgpu::Texture,
    /// Retained for bind-group rebuilds after a texture replacement —
    /// sampler state never varies with the atlas height.
    sampler: wgpu::Sampler,
    /// Group-0 layout (gradient texture + sampler). Quad and curve build
    /// their pipeline layouts against this so they can share one bind
    /// group at draw time. Height-independent, so a grown atlas leaves
    /// every pipeline built against it valid.
    pub(super) bgl: wgpu::BindGroupLayout,
    /// Group-0 bind group, bound by both pipelines at draw time.
    pub(super) bg: wgpu::BindGroup,
}

/// Allocate the LUT atlas texture at `rows` rows. The shaders read the
/// height back with `textureDimensions` rather than a baked-in
/// constant, so this is free to change across the device's lifetime.
fn create_texture(device: &wgpu::Device, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.gradient_atlas"),
        size: wgpu::Extent3d {
            width: LUT_ROW_TEXELS as u32,
            height: rows,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

impl GpuGradientAtlas {
    pub(super) fn new(device: &wgpu::Device, cpu: SharedGradientAtlas) -> Self {
        // Group 0 = gradient LUT atlas + sampler. Viewport rides
        // immediates (shared with every pipeline) — no bind-group slot.
        let bgl = texture_binding::layout(device, "palantir.gradient.bgl");

        let texture = create_texture(device, cpu.rows());
        let view = texture.create_view(&Default::default());
        // Linear filter inside a row (smooth gradient interpolation).
        // Clamp addressing — spread modes (Pad/Repeat/Reflect) are
        // applied shader-side on `t` before the sample, so the GPU
        // sampler never sees t outside 0..1.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.gradient_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let bg = texture_binding::bind_group(device, &bgl, &sampler, &view, "palantir.gradient.bg");

        Self {
            cpu,
            texture,
            sampler,
            bgl,
            bg,
        }
    }

    /// Sync the gradient LUT atlas from CPU to GPU if anything changed.
    /// Idle frames (no new gradients) hit the early `None` return in
    /// `flush_with` and do nothing. Dirty frames upload the atlas's dirty
    /// row span in a single `write_texture`, sized to the range and
    /// offset via `origin.y` — one call keeps the fixed API cost of the
    /// old whole-atlas upload while an animated gradient re-baking one
    /// row moves 2 KB per frame instead of 512 KB (see the dirty-range
    /// note in `CpuGradientAtlas`). Called from `WgpuBackend::submit`
    /// before the render pass starts.
    ///
    /// A frame that registers more distinct gradients than the atlas
    /// holds grows it, which reports a new `total_rows` here: the
    /// texture and its bind group are replaced at the new height before
    /// the upload. Growth dirties every row, so the replacement texture
    /// is refilled in the same `write_texture`, and the pipelines stay
    /// valid because they bind through the height-independent `bgl` and
    /// read the height with `textureDimensions`.
    pub(super) fn upload(&mut self, ctx: &GpuCtx<'_>) {
        // Destructured so the resize below borrows the GPU-side fields
        // while `flush_with` holds the CPU atlas.
        let Self {
            cpu,
            texture,
            sampler,
            bgl,
            bg,
        } = self;
        cpu.flush_with(|rows| {
            if texture.height() != rows.total_rows {
                *texture = create_texture(ctx.device, rows.total_rows);
                let view = texture.create_view(&Default::default());
                *bg = texture_binding::bind_group(
                    ctx.device,
                    bgl,
                    sampler,
                    &view,
                    "palantir.gradient.bg",
                );
            }
            // Whole rows by the `FlushedRows` contract, so this divides
            // exactly.
            let height = rows.bytes.len() as u32 / ROW_PITCH;
            TextureRegion {
                texture,
                first_row: rows.first_row,
                size: UVec2::new(LUT_ROW_TEXELS as u32, height),
                bytes_per_row: ROW_PITCH,
            }
            .write(ctx.queue, rows.bytes);
        });
    }
}
