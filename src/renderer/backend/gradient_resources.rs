//! Shared gradient LUT atlas GPU resources — the texture, its sampler,
//! and the group-0 bind group every gradient-aware pipeline samples.
//!
//! Owned by [`WgpuBackend`](crate::renderer::backend::WgpuBackend) and lent to the quad and
//! curve pipelines (both render gradient brushes). Keeping the resource
//! here — rather than on whichever pipeline happens to build first —
//! means neither pipeline owns the other's input: each takes `&bgl` at
//! build time and `&bg` at bind time.

use crate::primitives::color::ColorF16;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::texture_sampler_bgl;
use crate::renderer::gradient_atlas::GradientAtlas;

/// Side of the gradient LUT atlas texture (square: 256 × 256). Must
/// equal `ATLAS_ROWS_F` in `quad.wgsl` — the shader divides the row
/// index by this constant to compute the sample `v` coord.
const GRADIENT_ATLAS_SIDE: u32 = 256;
const _: () = assert!(
    GRADIENT_ATLAS_SIDE == 256,
    "shader ATLAS_ROWS_F is hardcoded to 256.0; update quad.wgsl if you change this"
);
/// Bytes per atlas texel: `Rgba16Float` = 4 × f16 = 8 bytes. Derived
/// from the CPU-side `ColorF16` row store (`gradient_atlas::LutRowTexels`)
/// so the GPU upload row-pitch can't silently drift from the texel type
/// the bake writes.
const GRADIENT_ATLAS_TEXEL_BYTES: u32 = size_of::<ColorF16>() as u32;
// `write_texture`'s `bytes_per_row` must be a multiple of
// `COPY_BYTES_PER_ROW_ALIGNMENT` (256). Guard the row pitch independently
// of the shader assert above so relaxing one can't silently break the
// upload alignment.
const _: () = assert!(
    (GRADIENT_ATLAS_SIDE * GRADIENT_ATLAS_TEXEL_BYTES).is_multiple_of(256),
    "gradient atlas row pitch must be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT (256)"
);

/// Gradient LUT atlas texture + sampler + bind group, shared by the
/// quad and curve pipelines. Format-independent: survives a swapchain
/// format change untouched (only the pipelines carry the color target).
pub(crate) struct GradientResources {
    /// LUT atlas texture. 256 cols × 256 rows of `Rgba16Float`
    /// (linear, no sampler decode — the LUT bake stores linear-RGB
    /// directly via `From<Color> for ColorF16`, so the GPU sees
    /// ready-to-blend linear values; see `CLAUDE.md` "Colour pipeline").
    /// f16 over 8-bit linear: dark gradient stops linearise to tiny
    /// values, and an 8-bit linear row crushes them onto a handful of
    /// levels (visible banding) — see `gradient_atlas` module docs.
    /// Uploaded each dirty frame by [`Self::upload`].
    texture: wgpu::Texture,
    /// Group-0 layout (gradient texture + sampler). Quad and curve build
    /// their pipeline layouts against this so they can share one bind
    /// group at draw time.
    pub(crate) bgl: wgpu::BindGroupLayout,
    /// Group-0 bind group, bound by both pipelines at draw time.
    pub(crate) bg: wgpu::BindGroup,
}

impl GradientResources {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        // Group 0 = gradient LUT atlas + sampler. Viewport rides
        // immediates (shared with every pipeline) — no bind-group slot.
        let bgl = texture_sampler_bgl(device, "palantir.gradient.bgl");

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.gradient_atlas"),
            size: wgpu::Extent3d {
                width: GRADIENT_ATLAS_SIDE,
                height: GRADIENT_ATLAS_SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
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

        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.gradient.bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self { texture, bgl, bg }
    }

    /// Sync the gradient LUT atlas from CPU to GPU if anything changed.
    /// Idle frames (no new gradients) hit the early `None` return in
    /// `flush_with` and do nothing. Dirty frames upload the entire
    /// 512 KB atlas in a single `write_texture` — see the dirty-tracking
    /// note in `GradientCpuAtlas` for why per-row uploads aren't worth
    /// the API overhead. Called from `WgpuBackend::submit` before the
    /// render pass starts.
    #[profiling::function]
    pub(crate) fn upload(&self, ctx: &GpuCtx<'_>, atlas: &GradientAtlas) {
        atlas.flush_with(|bytes| {
            ctx.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(GRADIENT_ATLAS_SIDE * GRADIENT_ATLAS_TEXEL_BYTES),
                    rows_per_image: Some(GRADIENT_ATLAS_SIDE),
                },
                wgpu::Extent3d {
                    width: GRADIENT_ATLAS_SIDE,
                    height: GRADIENT_ATLAS_SIDE,
                    depth_or_array_layers: 1,
                },
            );
        });
    }
}
