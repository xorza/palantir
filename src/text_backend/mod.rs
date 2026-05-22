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

use crate::renderer::backend::GpuCtx;
use crate::renderer::render_buffer::TextRun;
use crate::text::TextShaper;
use crate::text::cosmic::RenderSplit;
use cosmic_text::SwashCache;
use std::ops::Range;
use wgpu::util::DeviceExt;

pub(crate) use atlas::GlyphAtlas;
use encode::{
    EncodeCtx, EncodedCache, EncodedRunKey, ResolvedRun, encode_batch, encode_key_for,
    try_emit_cached,
};

/// Frames an unused `EncodedCache` entry survives before being swept
/// in `post_record`. Keeps the cache from growing unboundedly under a
/// long zoom gesture while comfortably outliving any short flicker
/// (visibility toggle, hover paint) that drops a run for a frame.
const ENCODED_CACHE_KEEP_FRAMES: u64 = 120;

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

/// Uniform params: atlas sizes only. Viewport resolution comes from
/// the shared `@group(0) = viewport` binding every pipeline in the
/// main pass has bound — no need to duplicate. `encase::ShaderType`
/// handles WGSL's 16-byte struct rounding; consumers go through
/// [`Self::encode`] instead of `bytemuck::bytes_of` (same pattern as
/// `ViewportUniformData` in `renderer/backend/viewport.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, encase::ShaderType)]
pub(crate) struct Params {
    /// `[color_atlas_size, mask_atlas_size]`.
    pub(crate) atlas_px: glam::UVec2,
}

impl Params {
    const BYTES: usize = <Self as encase::ShaderSize>::SHADER_SIZE.get() as usize;

    fn encode(&self) -> [u8; Self::BYTES] {
        let mut out = [0u8; Self::BYTES];
        encase::UniformBuffer::new(&mut out[..])
            .write(self)
            .unwrap();
        out
    }
}

/// 0 = mask, 1 = color. Encoded in the high bit of `uv.u`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum ContentType {
    Mask = 0,
    Color = 1,
}

pub struct TextBackend {
    shaper: TextShaper,
    swash_cache: SwashCache,
    atlas: GlyphAtlas,

    pipelines: Vec<wgpu::RenderPipeline>,
    atlas_bgl: wgpu::BindGroupLayout,
    atlas_bg: wgpu::BindGroup,
    params_bg: wgpu::BindGroup,
    sampler: wgpu::Sampler,

    /// Pending atlas-size state. Mutated by `prepare_batch` after
    /// atlas grow and flushed alongside other belt writes when it
    /// diverges from `uploaded_params`.
    params: Params,
    /// What the GPU buffer currently holds. Reupload on mismatch.
    uploaded_params: Params,
    params_buffer: wgpu::Buffer,

    instances: Vec<GlyphInstance>,
    vbuf: wgpu::Buffer,
    vbuf_capacity: u64,

    ranges: Vec<Option<Range<u32>>>,
    pub(crate) prepared_anything: bool,

    encoded_cache: EncodedCache,
    /// Misses found in `prepare_batch`'s pass 1. Each entry pins the
    /// run index plus the already-computed cache key + origin so
    /// pass 2 doesn't repeat `encode_key_for`. Retained across calls
    /// so an all-hit frame stays alloc-free.
    misses: Vec<MissEntry>,
}

#[derive(Clone, Copy)]
struct MissEntry {
    run_idx: u32,
    run_key: EncodedRunKey,
}

impl TextBackend {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        multisample: wgpu::MultisampleState,
        depth_stencil_states: &[Option<wgpu::DepthStencilState>],
        shaper: TextShaper,
        viewport_bgl: &wgpu::BindGroupLayout,
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
                    // WGSL-aware size from `encase::ShaderSize`,
                    // rounded up to the uniform-struct minimum
                    // (16 bytes here). `size_of::<Params>()` is the
                    // *Rust* size, which is smaller.
                    min_binding_size: Some(<Params as encase::ShaderSize>::SHADER_SIZE),
                },
                count: None,
            }],
        });

        let params = Params {
            atlas_px: glam::UVec2::new(atlas.color_size(), atlas.mask_size()),
        };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("palantir text params"),
            contents: &params.encode(),
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
        // `params_bgl` is consumed by `pipeline_layout`; wgpu Arc-counts
        // bind-group layouts internally so the local goes out of scope
        // safely after pipeline construction.

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir text shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("palantir text pipeline layout"),
            // group(0) = shared viewport (the backend binds it once
            // per pass, valid across every pipeline). group(1) =
            // atlas textures + sampler. group(2) = atlas-size params.
            bind_group_layouts: &[Some(viewport_bgl), Some(&atlas_bgl), Some(&params_bgl)],
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
            encoded_cache: EncodedCache::default(),
            misses: Vec::new(),
        }
    }

    /// Append-mode prepare. Looks up cosmic buffers via the shaper,
    /// emits instances, optionally rebinds the atlas bind group if
    /// it grew. Returns true if any instance was emitted.
    #[profiling::function]
    pub(crate) fn prepare_batch(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        scale: f32,
        batch_idx: usize,
        runs: &[TextRun],
    ) -> bool {
        let start = self.instances.len() as u32;

        // Pass 1: walk every run, emit encoded-cache hits straight to
        // `instances`, collect miss entries (carrying their already-
        // computed key + origin so pass 2 doesn't re-derive). No
        // `with_render_split` — an all-hit frame never cracks the
        // RefCell or hits cosmic.
        let current_frame = self.atlas.current_frame;
        let eviction = self.atlas.eviction_count;
        self.misses.clear();
        for (i, r) in runs.iter().enumerate() {
            if r.key.is_invalid() {
                // Mono fallback emits nothing; skip both paths.
                continue;
            }
            let run_key = encode_key_for(r, scale);
            if try_emit_cached(
                &mut self.encoded_cache,
                eviction,
                current_frame,
                &run_key,
                &mut self.instances,
            ) {
                continue;
            }
            self.misses.push(MissEntry {
                run_idx: i as u32,
                run_key,
            });
        }

        // Pass 2: shape only the misses. Take `misses` so the closure
        // can borrow it without conflicting with `&mut self`.
        if !self.misses.is_empty() {
            let shaper = self.shaper.clone();
            let misses = std::mem::take(&mut self.misses);
            shaper.with_render_split(|split| {
                let RenderSplit {
                    font_system,
                    lookup,
                } = split;

                let resolved = misses.iter().filter_map(|m| {
                    let r = &runs[m.run_idx as usize];
                    lookup.get(r.key).map(|buffer| {
                        (
                            ResolvedRun {
                                buffer,
                                origin: r.origin,
                                bounds: r.bounds,
                                scale: scale * r.scale,
                                color: r.color,
                            },
                            m.run_key,
                        )
                    })
                });

                let mut ectx = EncodeCtx {
                    device: ctx.device,
                    font_system,
                    swash_cache: &mut self.swash_cache,
                    atlas: &mut self.atlas,
                    cache: &mut self.encoded_cache,
                };
                encode_batch(&mut ectx, resolved, &mut self.instances);
            });
            self.misses = misses;
        }

        let end = self.instances.len() as u32;
        let did_work = end > start;

        // Rebuild bind group if atlas grew during encode.
        if self.atlas.bind_group_dirty {
            self.atlas_bg = build_atlas_bg(
                ctx.device,
                &self.atlas_bgl,
                self.atlas.mask_view(),
                self.atlas.color_view(),
                &self.sampler,
            );
            self.atlas.bind_group_dirty = false;
        }

        // Flush params if `atlas_px` (mutated by atlas grow) diverged
        // from what's on the GPU. The comparison against
        // `uploaded_params` schedules the write.
        self.params.atlas_px = glam::UVec2::new(self.atlas.color_size(), self.atlas.mask_size());
        if self.params != self.uploaded_params {
            ctx.write(&self.params_buffer, 0, &self.params.encode());
            self.uploaded_params = self.params;
        }

        if self.ranges.len() <= batch_idx {
            self.ranges.resize(batch_idx + 1, None);
        }
        self.ranges[batch_idx] = Some(start..end);

        if did_work {
            self.prepared_anything = true;
            self.upload_vbuf(ctx, start);
        }
        did_work
    }

    /// Drain glyph-atlas uploads accumulated by `prepare_batch` into
    /// the renderer's encoder. Called once per frame, after all
    /// `prepare_batch` calls and right after the renderer creates its
    /// main command encoder — so atlas uploads share the same submit
    /// as the text draws that read from them.
    pub(crate) fn flush_atlas_uploads(&mut self, ctx: &mut GpuCtx<'_>) {
        self.atlas.flush_pending_uploads(ctx);
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
        // group(0) = shared viewport, pre-bound by the backend at
        // pass open. group(1) = atlas; group(2) = params.
        pass.set_bind_group(1, &self.atlas_bg, &[]);
        pass.set_bind_group(2, &self.params_bg, &[]);
        pass.set_vertex_buffer(0, self.vbuf.slice(..));
        pass.draw(0..4, range);
    }

    pub(crate) fn post_record(&mut self) {
        self.atlas.trim();
        self.encoded_cache
            .sweep(self.atlas.current_frame, ENCODED_CACHE_KEEP_FRAMES);
        self.instances.clear();
        self.ranges.fill(None);
        self.prepared_anything = false;
    }

    /// Upload glyph instances appended by this batch to `self.vbuf`.
    /// `start` is the `self.instances.len()` captured before this
    /// batch began emitting — so `[start..len]` is the batch's
    /// just-appended slice. On the common no-grow path we belt-write
    /// only that slice to its corresponding byte offset, leaving
    /// prior batches' bytes (already on the GPU) untouched. On the
    /// rare grow path the buffer is replaced with undefined contents,
    /// so we re-upload the full `self.instances`.
    fn upload_vbuf(&mut self, ctx: &mut GpuCtx<'_>, start: u32) {
        let stride = std::mem::size_of::<GlyphInstance>();
        let needed = (self.instances.len() * stride) as u64;
        let grew = needed > self.vbuf_capacity;
        if grew {
            let new_cap = needed.next_power_of_two().max(self.vbuf_capacity * 2);
            self.vbuf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir text vbuf"),
                size: new_cap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vbuf_capacity = new_cap;
            let bytes: &[u8] = bytemuck::cast_slice(&self.instances);
            ctx.write(&self.vbuf, 0, bytes);
        } else {
            let new_bytes: &[u8] = bytemuck::cast_slice(&self.instances[start as usize..]);
            let offset = u64::from(start) * stride as u64;
            ctx.write(&self.vbuf, offset, new_bytes);
        }
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

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    //! Bench/test reach-in surface. Exposes `TextBackend` end-to-end so
    //! `benches/text_atlas.rs` can drive prepare → flush → render
    //! without going through `Host`'s full record/measure/cascade/encode
    //! pipeline.

    use super::StencilMode;
    use crate::layout::types::align::HAlign;
    use crate::primitives::color::ColorU8;
    use crate::primitives::urect::URect;

    use crate::text::{FontFamily, TextShaper};
    use glam::{UVec2, Vec2};

    pub use super::TextBackend;
    /// Re-export the `pub(crate)` `GpuCtx` so benches can construct
    /// one to feed `prepare`/`flush`. The full path
    /// (`crate::renderer::backend::dynamic_buffer::GpuCtx`) is
    /// noisy at the call site.
    pub use crate::renderer::backend::GpuCtx;
    /// Re-export the otherwise-`pub(crate)` `TextRun` so benches can
    /// name it in their fixture slice.
    pub use crate::renderer::render_buffer::TextRun;

    impl TextBackend {
        /// Construct a single-pipeline backend with no MSAA and no
        /// depth/stencil — enough to render against an `Rgba8Unorm*`
        /// color target.
        pub fn new_for_bench(
            device: &wgpu::Device,
            format: wgpu::TextureFormat,
            shaper: TextShaper,
        ) -> Self {
            // Standalone helper — build a private viewport bgl with
            // the same shape the backend uses. (Production hosts get
            // it from `WgpuBackend::viewport_uniform.bgl`.)
            let viewport_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.text_backend.bench.viewport.bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
            Self::new(
                device,
                format,
                wgpu::MultisampleState::default(),
                &[None],
                shaper,
                &viewport_bgl,
            )
        }

        /// Append-mode prepare into batch 0.
        pub fn prepare(&mut self, ctx: &mut GpuCtx<'_>, scale: f32, runs: &[TextRun]) -> bool {
            self.prepare_batch(ctx, scale, 0, runs)
        }

        pub fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
            self.flush_atlas_uploads(ctx);
        }

        pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
            self.render_batch(0, pass, StencilMode::Plain);
        }

        pub fn end_frame(&mut self) {
            self.post_record();
        }
    }

    /// Shape `text` via `shaper` (cosmic path required — mono fallback
    /// returns the invalid sentinel that the encoder drops) and build a
    /// `TextRun` placed at `origin` inside the given physical viewport.
    #[allow(clippy::too_many_arguments)]
    pub fn make_run(
        shaper: &TextShaper,
        text: &str,
        font_size_px: f32,
        line_height_px: f32,
        origin: Vec2,
        viewport: UVec2,
        scale: f32,
        color: ColorU8,
    ) -> TextRun {
        let m = shaper.measure(
            text,
            font_size_px,
            line_height_px,
            None,
            FontFamily::Sans,
            HAlign::Auto,
        );
        TextRun {
            key: m.key,
            origin,
            bounds: URect::new(0, 0, viewport.x, viewport.y),
            color,
            scale,
        }
    }
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
    fn params_shader_size_matches_a_vec2_u32() {
        // Pinned: with a single `UVec2` member, `encase::ShaderSize`
        // reports 8 bytes — matches WGSL's `vec2<u32>` layout. If
        // this trips, the struct shape changed and `Params::encode`
        // / the shader binding need re-checking.
        assert_eq!(<Params as encase::ShaderSize>::SHADER_SIZE.get(), 8);
        assert_eq!(Params::BYTES, 8);
    }
}
