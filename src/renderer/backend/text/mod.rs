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

use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{
    PipelineRecipe, build_pipeline, build_pipeline_layout,
};
use crate::renderer::backend::viewport::ViewportPush;
use crate::renderer::render_buffer::TextRun;
use crate::text::TextShaper;
use crate::text::cosmic::RenderSplit;
use cosmic_text::SwashCache;
use std::ops::Range;

use atlas::GlyphAtlas;
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

/// Atlas-size params (text-only). Lives in the shared immediate
/// region at offset 8 (right after `ViewportPush` at offset 0).
/// `encase::ShaderType` handles WGSL alignment; `push_into` writes
/// the encoded bytes through `set_immediates`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, encase::ShaderType)]
pub(crate) struct Params {
    /// `[color_atlas_size, mask_atlas_size]`.
    pub(crate) atlas_px: glam::UVec2,
}

impl Params {
    /// Offset inside the shared immediate region. 8 = right after
    /// `ViewportPush::BYTES`. Hard-coded rather than computed so a
    /// drift between this and the shader's `Immediates` struct trips
    /// the `params_offset_follows_viewport` test, not silent mis-reads.
    const OFFSET: u32 = 8;
    const BYTES: usize = <Self as encase::ShaderSize>::SHADER_SIZE.get() as usize;

    fn encode(&self) -> [u8; Self::BYTES] {
        let mut out = [0u8; Self::BYTES];
        encase::UniformBuffer::new(&mut out[..])
            .write(self)
            .unwrap();
        out
    }

    /// Push these atlas sizes into the active pipeline's immediate
    /// region at [`Self::OFFSET`]. Caller must ensure a pipeline is
    /// already bound (wgpu's `set_immediates` validation).
    fn push_into(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_immediates(Self::OFFSET, &self.encode());
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
    sampler: wgpu::Sampler,

    /// Atlas-size params. Mutated by `prepare_batch` after atlas
    /// grow, pushed per-pass via `RenderPass::set_immediates` in
    /// `render_batch` — no uniform buffer, no bind group, no dirty
    /// flushing.
    params: Params,

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

        let params = Params {
            atlas_px: glam::UVec2::new(atlas.color_size(), atlas.mask_size()),
        };

        let atlas_bg = build_atlas_bg(
            device,
            &atlas_bgl,
            atlas.mask_view(),
            atlas.color_view(),
            &sampler,
        );

        let pipelines = Self::build_pipelines(device, &atlas_bgl, format, depth_stencil_states);

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
            sampler,
            params,
            instances: Vec::new(),
            vbuf,
            vbuf_capacity,
            ranges: Vec::new(),
            prepared_anything: false,
            encoded_cache: EncodedCache::default(),
            misses: Vec::new(),
        }
    }

    /// Build the per-stencil-config render pipelines against `format`.
    /// The shader module + pipeline layout are cheap, format-independent
    /// boilerplate (recreated each call); the glyph atlas, its bind
    /// group, and the sampler are not built here and so survive a
    /// rebuild. Shared by [`Self::new`] and [`Self::rebuild_for_format`].
    fn build_pipelines(
        device: &wgpu::Device,
        atlas_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        depth_stencil_states: &[Option<wgpu::DepthStencilState>],
    ) -> Vec<wgpu::RenderPipeline> {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.text.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        // Group 0 = atlas textures + sampler. Viewport + atlas-size
        // `Params` ride the shared immediate region.
        let layout = build_pipeline_layout(device, "palantir.text.pl", &[Some(atlas_bgl)]);

        depth_stencil_states
            .iter()
            .map(|ds| {
                let label = if ds.is_some() {
                    "palantir.text.pipeline.stencil_test"
                } else {
                    "palantir.text.pipeline"
                };
                build_pipeline(
                    device,
                    PipelineRecipe {
                        label,
                        shader: &shader,
                        layout: &layout,
                        vertex_buffers: &[glyph_instance_layout()],
                        topology: wgpu::PrimitiveTopology::TriangleStrip,
                        color_format: format,
                        fragment_entry: "fs",
                        color_writes: wgpu::ColorWrites::ALL,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        depth_stencil: ds.clone(),
                    },
                )
            })
            .collect()
    }

    /// Rebuild only the format-dependent render pipelines against
    /// `format`. The glyph atlas (every rasterized glyph in it), its
    /// bind group, the sampler, and the encoded-run cache all survive —
    /// the atlas texture's format (`R8Unorm` / `Rgba8UnormSrgb`) is
    /// independent of the swapchain color format, so **no glyph
    /// re-rasterization is needed**. Pass the same `depth_stencil_states`
    /// the backend built the text pipelines with.
    pub(crate) fn rebuild_for_format(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth_stencil_states: &[Option<wgpu::DepthStencilState>],
    ) {
        self.pipelines =
            Self::build_pipelines(device, &self.atlas_bgl, format, depth_stencil_states);
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
        self.misses.clear();
        for (i, r) in runs.iter().enumerate() {
            if r.key.is_invalid() {
                // Mono fallback emits nothing; skip both paths.
                continue;
            }
            let run_key = encode_key_for(r, scale);
            if try_emit_cached(
                &mut self.encoded_cache,
                &mut self.atlas,
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

        // Track atlas-size changes on `self.params`; `render_batch`
        // pushes the value via `set_immediates` each draw — no
        // buffer write, no bind-group rebind. Same `Params`-only
        // dirty tracking; consumers see the freshest value.
        self.params.atlas_px = glam::UVec2::new(self.atlas.color_size(), self.atlas.mask_size());

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
        viewport: &ViewportPush,
    ) {
        let Some(range) = self.ranges.get(batch_idx).cloned().flatten() else {
            return;
        };
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipelines[mode.pipeline_idx()]);
        pass.set_bind_group(0, &self.atlas_bg, &[]);
        // Both halves of the shared immediate region — write
        // viewport (offset 0) here as well as params (offset 8)
        // because text can be the very first pipeline bound in the
        // pass, so the backend hasn't pushed viewport elsewhere yet.
        // Cheap: register-mapped, no buffer round-trip.
        viewport.push_into(pass);
        self.params.push_into(pass);
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

// `pos: Sint32x2 @0`, `dim: Uint32 @8`, `uv_and_kind: Uint32 @12`,
// `color: Uint32 @16` — the per-instance `GlyphInstance` stream.
const GLYPH_INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Sint32x2,
    1 => Uint32,
    2 => Uint32,
    3 => Uint32,
];

// Compile-time guard: attribute offsets must match the struct fields they
// feed. `array_stride == size_of` alone wouldn't catch a same-size field
// reorder; `offset_of!` does. Matches the guards on the quad / mesh / image
// / curve pipelines.
const _: () = {
    use std::mem::offset_of;
    assert!(GLYPH_INSTANCE_ATTRS[0].offset == offset_of!(GlyphInstance, pos) as u64);
    assert!(GLYPH_INSTANCE_ATTRS[1].offset == offset_of!(GlyphInstance, dim) as u64);
    assert!(GLYPH_INSTANCE_ATTRS[2].offset == offset_of!(GlyphInstance, uv_and_kind) as u64);
    assert!(GLYPH_INSTANCE_ATTRS[3].offset == offset_of!(GlyphInstance, color) as u64);
};

fn glyph_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GlyphInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &GLYPH_INSTANCE_ATTRS,
    }
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    //! Bench/test reach-in surface. Exposes `TextBackend` end-to-end so
    //! `benches/text_atlas.rs` can drive prepare → flush → render
    //! without going through `Host`'s full record/measure/cascade/encode
    //! pipeline.

    use crate::layout::types::align::HAlign;
    use crate::primitives::color::ColorU8;
    use crate::primitives::urect::URect;
    use crate::renderer::backend::text::{StencilMode, ViewportPush};

    use crate::text::{FontFamily, TextShaper};
    use glam::{UVec2, Vec2};

    /// Re-export the `pub(crate)` `GpuCtx` so benches can construct
    /// one to feed `prepare`/`flush`. The full path
    /// (`crate::renderer::backend::dynamic_buffer::GpuCtx`) is
    /// noisy at the call site.
    pub use crate::renderer::backend::gpu_ctx::GpuCtx;
    /// Re-export the counting `Queue` wrapper so benches can build one
    /// to feed `GpuCtx::new`.
    pub use crate::renderer::backend::queue::Queue;
    pub use crate::renderer::backend::text::TextBackend;
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
            Self::new(device, format, &[None], shaper)
        }

        /// Append-mode prepare into batch 0.
        pub fn prepare(&mut self, ctx: &mut GpuCtx<'_>, scale: f32, runs: &[TextRun]) -> bool {
            self.prepare_batch(ctx, scale, 0, runs)
        }

        pub fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
            self.flush_atlas_uploads(ctx);
        }

        pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
            // Standalone bench helper: zero-sized viewport is fine
            // because the atlas bench doesn't read the value (we
            // don't validate visual output here).
            let viewport = ViewportPush {
                size: glam::Vec2::ZERO,
            };
            self.render_batch(0, pass, StencilMode::Plain, &viewport);
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
    use crate::renderer::backend::text::{GlyphInstance, Params};
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

    #[test]
    fn params_offset_follows_viewport() {
        // Pinned: `Params` lives in the shared immediate region right
        // after `ViewportPush` (8 bytes). If `ViewportPush` grows or
        // `Params::OFFSET` drifts, the shader's `imm.params` would
        // read the wrong bytes. Total 16 must also still fit inside
        // `IMMEDIATES_BYTES`.
        use crate::renderer::backend::IMMEDIATES_BYTES;
        use crate::renderer::backend::viewport::ViewportPush;
        assert_eq!(Params::OFFSET as usize, ViewportPush::BYTES);
        assert!(Params::OFFSET as usize + Params::BYTES <= IMMEDIATES_BYTES as usize);
    }
}

/// GPU regression coverage for the encoded-cache liveness fix. Gated
/// on `internals` (not bare `test`) so the default headless `cargo
/// test` stays GPU-free, matching the visual / atlas-bench gating.
#[cfg(feature = "internals")]
#[cfg(test)]
mod gpu_regression {
    use crate::renderer::backend::gpu_ctx::GpuCtx;
    use crate::renderer::backend::queue::Queue;
    use crate::renderer::backend::text::TextBackend;
    use crate::text::TextShaper;
    use glam::{UVec2, Vec2};
    use pollster::FutureExt;

    use crate::primitives::color::ColorU8;
    use crate::renderer::backend::text::test_support::make_run;
    use crate::renderer::render_buffer::TextRun;

    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
    const PHYSICAL: UVec2 = UVec2::new(640, 480);

    fn device_queue() -> (wgpu::Device, Queue) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .block_on()
            .expect("request adapter (headless)");
        let mut limits = wgpu::Limits::default();
        limits.max_immediate_size = limits.max_immediate_size.max(16);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("palantir.text_regression.device"),
                required_features: wgpu::Features::IMMEDIATES,
                required_limits: limits,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .expect("request device");
        (device, Queue::new(queue))
    }

    fn run_one_frame(
        device: &wgpu::Device,
        queue: &Queue,
        backend: &mut TextBackend,
        scale: f32,
        runs: &[TextRun],
    ) {
        let mut belt = wgpu::util::StagingBelt::new(device.clone(), 1 << 16);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut ctx = GpuCtx::new(device, queue, &mut belt, &mut encoder);
            backend.prepare(&mut ctx, scale, runs);
            backend.flush(&mut ctx);
        }
        belt.finish();
        queue.submit([encoder.finish()]);
        belt.recall();
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("poll");
    }

    /// A run that hits the encoded cache must still refresh the LRU
    /// `last_use` of every atlas slot it rides. Before the fix the
    /// fast path emitted cached uv coords without touching the slots,
    /// so a steadily-cached run's slots froze at their rasterization
    /// frame and `evict_one` (which fires under zoom's many-sizes
    /// atlas pressure) would reclaim a still-live slot and overwrite it
    /// with a different glyph — garbled text.
    #[test]
    fn cached_run_keeps_its_atlas_slots_live() {
        let (device, queue) = device_queue();
        let shaper = TextShaper::with_bundled_fonts();
        let mut backend = TextBackend::new_for_bench(&device, FORMAT, shaper.clone());

        let runs = [make_run(
            &shaper,
            "File",
            14.0,
            14.0 * 1.2,
            Vec2::new(20.0, 20.0),
            PHYSICAL,
            1.0,
            ColorU8::rgba(240, 240, 240, 255),
        )];

        // Frame 1: encoded-cache miss → rasterize + cache. Slots get
        // last_use == current_frame.
        run_one_frame(&device, &queue, &mut backend, 2.0, &runs);
        backend.end_frame();
        let evictions_after_warmup = backend.atlas.eviction_count;
        assert!(
            !backend.atlas.cache.is_empty(),
            "warmup should have rasterized at least one glyph",
        );

        // Frame 2: same run → encoded-cache hit (no cosmic walk, no new
        // rasterization). The hit must still bump every slot's
        // last_use to the now-current frame.
        run_one_frame(&device, &queue, &mut backend, 2.0, &runs);

        let cf = backend.atlas.current_frame;
        let stale: Vec<u64> = backend
            .atlas
            .cache
            .values()
            .map(|s| s.last_use)
            .filter(|&lu| lu != cf)
            .collect();
        assert!(
            stale.is_empty(),
            "cache-hit frame left slots stale: last_use {stale:?} != current_frame {cf}",
        );
        // The second frame was a pure hit — nothing should have been
        // re-rasterized or evicted.
        assert_eq!(
            backend.atlas.eviction_count, evictions_after_warmup,
            "a pure cache-hit frame must not evict",
        );
    }
}
