//! Palantir-native glyph atlas + text render pipeline.
//!
//! Replaces the vendored glyphon. Built to Palantir's contracts:
//!
//! - **Linear-premul end to end.** `ColorU8` is straight-linear-u8 in,
//!   shader writes `vec4(rgb*a, a)`, blend is
//!   `PREMULTIPLIED_ALPHA_BLENDING`. No sRGB encode/decode round-trip.
//! - **Scissor does the clipping.** No per-glyph CPU clip; composer
//!   group scissor crops; cheap y-range pre-cull keeps off-screen
//!   lines out of the atlas cache.
//! - **One bind group, one atlas struct.** Color + mask textures
//!   side by side; content_type bit selects in the shader.
//! - **GPU-blit on atlas grow.** `copy_texture_to_texture` from old
//!   to new; etagere preserves rects so the cache map stays intact —
//!   no re-rasterization.
//! - **Per-glyph `queue.write_texture` on cache miss.** wgpu batches
//!   internally.
//! - **20-byte instances** (vs glyphon's 24). content_type packed
//!   into uv high bit.
//! - **No `Viewport` object.** Params buffer lives here and is
//!   rewritten when resolution or atlas sizes change.

pub(crate) mod atlas;
pub(crate) mod encode;

use crate::renderer::render_buffer::TextRun;
use crate::text::TextShaper;
use crate::text::cosmic::RenderSplit;
use cosmic_text::SwashCache;
use glam::UVec2;
use std::num::NonZeroU64;
use std::ops::Range;
use wgpu::util::DeviceExt;

pub(crate) use atlas::GlyphAtlas;
use encode::{ResolvedRun, encode_batch};

/// Selects which pipeline a `prepare_batch` / `render_batch` call
/// targets. Same as the existing wrapper's `StencilMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StencilMode {
    Plain,
    Stencil,
}

impl StencilMode {
    fn pipeline_idx(self) -> usize {
        match self {
            Self::Plain => 0,
            Self::Stencil => 1,
        }
    }
}

/// One per-instance vertex record. 20 bytes, `Pod`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GlyphInstance {
    pub(crate) pos: [i32; 2],
    pub(crate) dim: u32,
    pub(crate) uv_and_kind: u32,
    pub(crate) color: u32,
}

/// Uniform params: viewport resolution + atlas sizes. 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Params {
    pub(crate) screen_px: UVec2,
    /// `[color_atlas_size, mask_atlas_size]`.
    pub(crate) atlas_px: [u32; 2],
}

/// 0 = mask, 1 = color. Encoded in the high bit of `uv.u`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum ContentType {
    Mask = 0,
    Color = 1,
}

pub(crate) struct TextBackend {
    shaper: TextShaper,
    swash_cache: SwashCache,
    atlas: GlyphAtlas,

    pipelines: Vec<wgpu::RenderPipeline>,
    atlas_bgl: wgpu::BindGroupLayout,
    #[allow(dead_code)] // Held only for the lifetime of `pipelines`.
    params_bgl: wgpu::BindGroupLayout,
    atlas_bg: wgpu::BindGroup,
    params_bg: wgpu::BindGroup,
    sampler: wgpu::Sampler,

    /// Pending state. Mutated by `update_viewport` (screen size) and
    /// by `prepare_batch` after atlas grow (atlas sizes).
    params: Params,
    /// What the GPU buffer currently holds. Reupload on mismatch.
    uploaded_params: Params,
    params_buffer: wgpu::Buffer,

    instances: Vec<GlyphInstance>,
    vbuf: wgpu::Buffer,
    vbuf_capacity: u64,

    ranges: Vec<Option<Range<u32>>>,
    prepared_anything: bool,
}

impl TextBackend {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        multisample: wgpu::MultisampleState,
        depth_stencil_states: &[Option<wgpu::DepthStencilState>],
        shaper: TextShaper,
    ) -> Self {
        assert!(
            !depth_stencil_states.is_empty(),
            "TextBackend needs at least one pipeline config",
        );

        let atlas = GlyphAtlas::new(device);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir text sampler"),
            min_filter: wgpu::FilterMode::Nearest,
            mag_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir text atlas layout"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let params_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir text params layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<Params>() as u64),
                },
                count: None,
            }],
        });

        let params = Params {
            screen_px: UVec2::ZERO,
            atlas_px: [atlas.color_size(), atlas.mask_size()],
        };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("palantir text params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let atlas_bg = build_atlas_bg(
            device,
            &atlas_bgl,
            atlas.mask_view(),
            atlas.color_view(),
            &sampler,
        );
        let params_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir text params bg"),
            layout: &params_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir text shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("palantir text pipeline layout"),
            bind_group_layouts: &[Some(&atlas_bgl), Some(&params_bgl)],
            immediate_size: 0,
        });

        let pipelines = depth_stencil_states
            .iter()
            .map(|ds| {
                build_pipeline(
                    device,
                    &shader,
                    &pipeline_layout,
                    format,
                    multisample,
                    ds.clone(),
                )
            })
            .collect();

        let vbuf_capacity = (std::mem::size_of::<GlyphInstance>() as u64) * 4096;
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir text vbuf"),
            size: vbuf_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            shaper,
            swash_cache: SwashCache::new(),
            atlas,
            pipelines,
            atlas_bgl,
            params_bgl,
            atlas_bg,
            params_bg,
            sampler,
            params,
            uploaded_params: params,
            params_buffer,
            instances: Vec::new(),
            vbuf,
            vbuf_capacity,
            ranges: Vec::new(),
            prepared_anything: false,
        }
    }

    /// Set the viewport resolution. Lazy-flushed in `prepare_batch`
    /// alongside atlas-size changes.
    pub(crate) fn update_viewport(&mut self, viewport_phys: UVec2) {
        self.params.screen_px = viewport_phys;
    }

    pub(crate) fn has_prepared(&self) -> bool {
        self.prepared_anything
    }

    /// Append-mode prepare. Looks up cosmic buffers via the shaper,
    /// emits instances, optionally rebinds the atlas bind group if
    /// it grew. Returns true if any instance was emitted.
    #[profiling::function]
    pub(crate) fn prepare_batch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scale: f32,
        batch_idx: usize,
        runs: &[TextRun],
    ) -> bool {
        let start = self.instances.len() as u32;

        let shaper = self.shaper.clone();
        shaper.with_render_split(|split| {
            let RenderSplit {
                font_system,
                lookup,
            } = split;

            let resolved = runs.iter().filter_map(|r| {
                lookup.get(r.key).map(|buffer| ResolvedRun {
                    buffer,
                    origin: r.origin,
                    bounds: r.bounds,
                    scale: scale * r.scale,
                    color: r.color,
                })
            });

            encode_batch(
                device,
                queue,
                font_system,
                &mut self.swash_cache,
                &mut self.atlas,
                resolved,
                &mut self.instances,
            );
        });

        let end = self.instances.len() as u32;
        let did_work = end > start;

        // Rebuild bind group if atlas grew during encode.
        if self.atlas.bind_group_dirty {
            self.atlas_bg = build_atlas_bg(
                device,
                &self.atlas_bgl,
                self.atlas.mask_view(),
                self.atlas.color_view(),
                &self.sampler,
            );
            self.atlas.bind_group_dirty = false;
        }

        // Flush params if pending state diverged from what's on the
        // GPU. `screen_px` is mutated by `update_viewport` and
        // `atlas_px` by atlas grow; both write through `self.params`
        // and rely on this comparison against `uploaded_params` to
        // schedule the write.
        self.params.atlas_px = [self.atlas.color_size(), self.atlas.mask_size()];
        if self.params != self.uploaded_params {
            queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&self.params));
            self.uploaded_params = self.params;
        }

        if self.ranges.len() <= batch_idx {
            self.ranges.resize(batch_idx + 1, None);
        }
        self.ranges[batch_idx] = Some(start..end);

        if did_work {
            self.prepared_anything = true;
            self.upload_vbuf(device, queue);
        }
        did_work
    }

    pub(crate) fn render_batch(
        &self,
        batch_idx: usize,
        pass: &mut wgpu::RenderPass<'_>,
        mode: StencilMode,
    ) {
        let Some(range) = self.ranges.get(batch_idx).cloned().flatten() else {
            return;
        };
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipelines[mode.pipeline_idx()]);
        pass.set_bind_group(0, &self.atlas_bg, &[]);
        pass.set_bind_group(1, &self.params_bg, &[]);
        pass.set_vertex_buffer(0, self.vbuf.slice(..));
        pass.draw(0..4, range);
    }

    pub(crate) fn post_record(&mut self) {
        self.atlas.trim();
        self.instances.clear();
        self.ranges.fill(None);
        self.prepared_anything = false;
    }

    fn upload_vbuf(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let bytes: &[u8] = bytemuck::cast_slice(&self.instances);
        let needed = bytes.len() as u64;
        if needed > self.vbuf_capacity {
            let new_cap = needed.next_power_of_two().max(self.vbuf_capacity * 2);
            self.vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir text vbuf"),
                size: new_cap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vbuf_capacity = new_cap;
        }
        queue.write_buffer(&self.vbuf, 0, bytes);
    }
}

fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
        },
        count: None,
    }
}

fn build_atlas_bg(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    mask_view: &wgpu::TextureView,
    color_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("palantir text atlas bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(color_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn build_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    multisample: wgpu::MultisampleState,
    depth_stencil: Option<wgpu::DepthStencilState>,
) -> wgpu::RenderPipeline {
    let stride = std::mem::size_of::<GlyphInstance>() as wgpu::BufferAddress;
    let vertex_buffer_layout = wgpu::VertexBufferLayout {
        array_stride: stride,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Sint32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 8,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 12,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 16,
                shader_location: 3,
            },
        ],
    };

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("palantir text pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[vertex_buffer_layout],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::default(),
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil,
        multisample,
        cache: None,
        multiview_mask: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{GlyphInstance, Params};
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn glyph_instance_is_20_bytes() {
        assert_eq!(size_of::<GlyphInstance>(), 20);
        assert_eq!(align_of::<GlyphInstance>(), 4);
        assert_eq!(offset_of!(GlyphInstance, pos), 0);
        assert_eq!(offset_of!(GlyphInstance, dim), 8);
        assert_eq!(offset_of!(GlyphInstance, uv_and_kind), 12);
        assert_eq!(offset_of!(GlyphInstance, color), 16);
    }

    #[test]
    fn params_is_16_bytes() {
        assert_eq!(size_of::<Params>(), 16);
        assert_eq!(align_of::<Params>(), 4);
    }
}
