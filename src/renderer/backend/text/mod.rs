//! Aperture-native glyph atlas + text render pipeline.
//!
//! Built to Aperture's contracts:
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
//! - **Batched glyph uploads on cache miss.** Rasterized pixels queue
//!   into a retained staging buffer and flush as one belt write + N
//!   `copy_buffer_to_texture` commands on the main encoder, recorded
//!   *after* any grow blit — encoder ordering is load-bearing
//!   (`queue.write_texture` runs before all encoder commands in a
//!   submit, so it could be clobbered by the blit).
//! - **20-byte instances** (vs glyphon's 24). content_type packed
//!   into uv high bit.
//! - **No `Viewport` object.** Atlas sizes ride the shared immediate
//!   region as two `u32`s, pushed per batch — no uniform buffer.

pub(crate) mod atlas;
pub(crate) mod encode;

use crate::primitives::span::Span;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{ColorVariantSpec, StencilVariant};
use crate::renderer::backend::viewport::ViewportPush;
use crate::renderer::render_buffer::text::TextRun;
use crate::text::TextShaper;
use crate::text::cosmic::RenderSplit;
use cosmic_text::SwashCache;

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

/// One per-instance vertex record. 20 bytes, `Pod`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GlyphInstance {
    pub(crate) pos: [i32; 2],
    pub(crate) dim: u32,
    pub(crate) uv_and_kind: u32,
    pub(crate) color: u32,
}

/// `[color_atlas_size, mask_atlas_size]` follows `ViewportPush` in the
/// shared immediate region.
const PARAMS_OFFSET: u32 = 8;
const PARAMS_BYTES: usize = std::mem::size_of::<[u32; 2]>();
const _: () = assert!(PARAMS_BYTES == 8);

/// 0 = mask, 1 = color. Encoded in the high bit of `uv.u`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum ContentType {
    Mask = 0,
    Color = 1,
}

impl ContentType {
    fn format(self) -> wgpu::TextureFormat {
        match self {
            Self::Mask => wgpu::TextureFormat::R8Unorm,
            Self::Color => wgpu::TextureFormat::Rgba8UnormSrgb,
        }
    }

    fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Mask => 1,
            Self::Color => 4,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Mask => "aperture text mask atlas",
            Self::Color => "aperture text color atlas",
        }
    }
}

pub(crate) struct TextBackend {
    shaper: TextShaper,
    swash_cache: SwashCache,
    atlas: GlyphAtlas,

    /// Text shader module — format-independent; [`Self::build_variants`]
    /// reads it to build each format's pipelines.
    shader: wgpu::ShaderModule,

    /// Group-0 layout (atlas textures + sampler). Format-independent;
    /// [`Self::build_variants`] composes each format's pipeline layout
    /// against it. The pipelines themselves live in `FormatPipelines`,
    /// keyed by swapchain format, and are passed into
    /// [`Self::render_batch`].
    atlas_bgl: wgpu::BindGroupLayout,
    atlas_bg: wgpu::BindGroup,
    sampler: wgpu::Sampler,

    /// `[color_atlas_size, mask_atlas_size]`, updated only when an atlas grows.
    atlas_px: [u32; 2],

    /// Drawable glyph instances accumulated across this frame's batches.
    pub(crate) instances: Vec<GlyphInstance>,
    vbuf: DynamicBuffer<GlyphInstance>,

    /// Per-batch slice of `instances`; empty span = nothing to draw.
    ranges: Vec<Span>,

    encoded_cache: EncodedCache,
    /// Misses found in `prepare_batch`'s pass 1. Each entry pins the
    /// run index plus the already-computed cache key + origin so
    /// pass 2 doesn't repeat `encode_key_for`. Retained across calls
    /// so an all-hit frame stays alloc-free.
    misses: Vec<MissEntry>,
}

#[derive(Clone, Copy, Debug)]
struct MissEntry {
    run_idx: u32,
    run_key: EncodedRunKey,
}

// Manual: `TextShaper` (whose `ShaperInner` holds `CosmicMeasure`)
// isn't `Debug`.
impl std::fmt::Debug for TextBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextBackend")
            .field("atlas", &self.atlas)
            .field("atlas_px", &self.atlas_px)
            .field("instances", &self.instances.len())
            .finish_non_exhaustive()
    }
}

impl TextBackend {
    /// Build the format-independent text resources (glyph atlas, shaper,
    /// caches, shader, vertex buffer). The render pipelines are built per
    /// format by [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variants`].
    pub(crate) fn new(device: &wgpu::Device, shaper: TextShaper) -> Self {
        let atlas = GlyphAtlas::new(device);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aperture.text.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aperture text sampler"),
            min_filter: wgpu::FilterMode::Nearest,
            mag_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aperture text atlas layout"),
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

        let bindings = atlas.bindings();
        let atlas_px = bindings.atlas_px;

        let atlas_bg = build_atlas_bg(
            device,
            &atlas_bgl,
            bindings.mask_view,
            bindings.color_view,
            &sampler,
        );

        let vbuf = DynamicBuffer::<GlyphInstance>::vertex(device, "aperture text vbuf", 4096);

        Self {
            shaper,
            swash_cache: SwashCache::new(),
            atlas,
            shader,
            atlas_bgl,
            atlas_bg,
            sampler,
            atlas_px,
            instances: Vec::new(),
            vbuf,
            ranges: Vec::new(),
            encoded_cache: EncodedCache::default(),
            misses: Vec::new(),
        }
    }

    /// Build the base + stencil-test render pipelines against `format`,
    /// reading the format-independent `shader`. The glyph atlas, its bind
    /// group, and the sampler are not built here and so survive a format
    /// change. Called by `FormatPipelines` per format; matches the
    /// `build_variants` shape of the quad / mesh / image / curve pipelines.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Group 0 = atlas textures + sampler. Viewport + atlas sizes
        // ride the shared immediate region.
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "aperture.text.pipeline",
                stencil_label: "aperture.text.pipeline.stencil_test",
                layout_label: "aperture.text.pl",
                shader: &self.shader,
                bind_group_layouts: &[Some(&self.atlas_bgl)],
                vertex_buffers: &[Some(glyph_instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Append-mode prepare. Looks up cosmic buffers via the shaper,
    /// emits instances, optionally rebinds the atlas bind group if
    /// it grew.
    #[profiling::function]
    pub(crate) fn prepare_batch(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        scale: f32,
        batch_idx: usize,
        runs: &[TextRun],
    ) {
        assert_eq!(
            batch_idx,
            self.ranges.len(),
            "text batches must be prepared once in contiguous order",
        );
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

        // Pass 2: shape only the misses.
        if !self.misses.is_empty() {
            let Self {
                shaper,
                swash_cache,
                atlas,
                instances,
                encoded_cache,
                misses,
                ..
            } = self;
            shaper.with_render_split(|split| {
                let RenderSplit {
                    font_system,
                    lookup,
                } = split;

                let resolved = misses.iter().map(|m| {
                    let r = &runs[m.run_idx as usize];
                    let buffer = lookup
                        .get(r.key)
                        .expect("valid text key missing from pinned render lookup");
                    ResolvedRun {
                        buffer,
                        origin: r.origin,
                        bounds: r.bounds,
                        scale: scale * r.scale,
                        color: r.color,
                        run_key: m.run_key,
                    }
                });

                let mut ectx = EncodeCtx {
                    device: ctx.device,
                    font_system,
                    swash_cache,
                    atlas,
                    cache: encoded_cache,
                };
                encode_batch(&mut ectx, resolved, instances);
            });
        }

        let end = self.instances.len() as u32;

        // Rebuild bind group if atlas grew during encode.
        if self.atlas.bind_group_dirty {
            let bindings = self.atlas.bindings();
            self.atlas_bg = build_atlas_bg(
                ctx.device,
                &self.atlas_bgl,
                bindings.mask_view,
                bindings.color_view,
                &self.sampler,
            );
            self.atlas_px = bindings.atlas_px;
            self.atlas.bind_group_dirty = false;
        }

        self.ranges.push(Span::new(start, end - start));
    }

    /// Upload this frame's accumulated glyph instances in one belt
    /// write, then drain queued glyph-atlas uploads (grow blits +
    /// per-glyph texture copies) onto the renderer's encoder. Called
    /// once per frame, after every `prepare_batch` and before any pass
    /// draws — so atlas uploads share the same submit as the text
    /// draws that read from them. Deferring instances to a single
    /// write replaces N per-batch belt suballocations + copy commands
    /// for disjoint tails of the same Vec, and a mid-frame grow's full
    /// re-upload happens at most once; batch `ranges` index into the
    /// shared buffer, so per-batch draws are unaffected.
    pub(crate) fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
        self.vbuf.upload_instances(ctx, &self.instances);
        self.atlas.flush_pending_uploads(ctx);
    }

    pub(crate) fn render_batch<'a>(
        &'a self,
        batch_idx: usize,
        pass: &mut wgpu::RenderPass<'a>,
        pipelines: &'a StencilVariant,
        use_stencil: bool,
        viewport: &ViewportPush,
    ) {
        let &span = self
            .ranges
            .get(batch_idx)
            .expect("render schedule referenced an unprepared text batch");
        if span.len == 0 {
            return;
        }
        pass.set_pipeline(pipelines.select(use_stencil));
        pass.set_bind_group(0, &self.atlas_bg, &[]);
        // Both halves of the shared immediate region — write
        // viewport (offset 0) here as well as params (offset 8)
        // because text can be the very first pipeline bound in the
        // pass, so the backend hasn't pushed viewport elsewhere yet.
        // Cheap: register-mapped, no buffer round-trip.
        viewport.push_into(pass);
        pass.set_immediates(PARAMS_OFFSET, bytemuck::bytes_of(&self.atlas_px));
        pass.set_vertex_buffer(0, self.vbuf.buffer.slice(..));
        pass.draw(0..4, span.start..span.start + span.len);
    }

    pub(crate) fn post_record(&mut self) {
        if self.ranges.is_empty() {
            assert!(self.instances.is_empty());
            return;
        }
        self.atlas.end_frame();
        self.encoded_cache
            .sweep(self.atlas.current_frame, ENCODED_CACHE_KEEP_FRAMES);
        self.instances.clear();
        self.ranges.clear();
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
        label: Some("aperture text atlas bg"),
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
// `color: Unorm8x4 @16` — the per-instance `GlyphInstance` stream.
// Color rides as `Unorm8x4` so the vertex fetch normalizes the
// linear-u8 bytes to `vec4<f32>` in hardware (spec-exact `x/255`) —
// same convention as the mesh / image tint attributes.
const GLYPH_INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Sint32x2,
    1 => Uint32,
    2 => Uint32,
    3 => Unorm8x4,
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

#[cfg(feature = "internals")]
pub mod test_support {
    //! Bench/test reach-in surface for driving prepare → flush → render
    //! without `WindowRenderer`'s full record/measure/cascade/encode pipeline.

    use crate::layout::types::align::HAlign;
    use crate::primitives::color::ColorU8;
    use crate::primitives::urect::URect;
    use crate::renderer::backend::gpu_ctx::test_support::GpuCtx;
    use crate::renderer::backend::pipeline_utils::StencilVariant;
    use crate::renderer::backend::text::TextBackend;
    use crate::renderer::backend::text::ViewportPush;
    use crate::renderer::render_buffer::text::TextRun as InnerTextRun;
    use crate::renderer::render_buffer::text::test_support::TextRun;

    use crate::text::{FontFamily, FontWeight, ShapeParams, TextShaper};
    use glam::{UVec2, Vec2};

    /// Standalone bench harness for the text backend: a [`TextBackend`]
    /// (its glyph atlas, shaper, caches) plus the single no-stencil
    /// pipeline it would otherwise reach for in `FormatPipelines`. Lets
    /// the atlas bench drive prepare → flush → draw without the full
    /// `WgpuBackend` / `FormatPipelines` machinery.
    #[derive(Debug)]
    pub struct BenchText {
        backend: TextBackend,
        pipelines: StencilVariant,
    }

    impl BenchText {
        /// Builds both base + stencil-test pipelines (the bench draws with
        /// the base, `use_stencil = false`) against an `Rgba8Unorm*` color
        /// target.
        pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, shaper: TextShaper) -> Self {
            let backend = TextBackend::new(device, shaper);
            let pipelines = backend.build_variants(device, format);
            Self { backend, pipelines }
        }

        /// Append-mode prepare into batch 0.
        pub fn prepare(&mut self, ctx: &mut GpuCtx<'_>, scale: f32, runs: &[TextRun]) {
            self.backend
                .prepare_batch(&mut ctx.0, scale, 0, TextRun::inner_slice(runs));
        }

        pub fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
            self.backend.flush(&mut ctx.0);
        }

        pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
            // Standalone bench helper: zero-sized viewport is fine
            // because the atlas bench doesn't read the value (we
            // don't validate visual output here).
            let viewport = ViewportPush {
                size: glam::Vec2::ZERO,
            };
            self.backend
                .render_batch(0, pass, &self.pipelines, false, &viewport);
        }

        pub fn end_frame(&mut self) {
            self.backend.post_record();
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
        TextRun::new(make_inner_run(
            shaper,
            text,
            font_size_px,
            line_height_px,
            origin,
            viewport,
            scale,
            color,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn make_inner_run(
        shaper: &TextShaper,
        text: &str,
        font_size_px: f32,
        line_height_px: f32,
        origin: Vec2,
        viewport: UVec2,
        scale: f32,
        color: ColorU8,
    ) -> InnerTextRun {
        let m = shaper.measure(
            text,
            ShapeParams {
                font_size_px,
                line_height_px,
                max_width_px: None,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                halign: HAlign::Auto,
            },
        );
        InnerTextRun {
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
    use crate::renderer::backend::text::{GlyphInstance, PARAMS_BYTES, PARAMS_OFFSET};
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
    fn params_bytes_match_a_vec2_u32() {
        assert_eq!(PARAMS_BYTES, 8);
    }

    #[test]
    fn params_offset_follows_viewport() {
        // Pinned: atlas sizes live in the shared immediate region right
        // after `ViewportPush` (8 bytes). If `ViewportPush` grows or
        // `PARAMS_OFFSET` drifts, the shader's `imm.params` would
        // read the wrong bytes. Total 16 must also still fit inside
        // `IMMEDIATES_BYTES`.
        use crate::renderer::backend::IMMEDIATES_BYTES;
        use crate::renderer::backend::viewport::ViewportPush;
        assert_eq!(PARAMS_OFFSET as usize, ViewportPush::BYTES);
        assert!(PARAMS_OFFSET as usize + PARAMS_BYTES <= IMMEDIATES_BYTES as usize);
    }
}

/// GPU regression coverage for the text backend caches (encoded-cache
/// liveness + clipping, atlas empty-entry sweep). Gated on `internals`
/// (not bare `test`) so the default headless `cargo test` stays
/// GPU-free, matching the visual / atlas-bench gating.
#[cfg(feature = "internals")]
#[cfg(test)]
mod gpu_regression {
    use wgpu::util::StagingBelt;

    use crate::renderer::backend::gpu_ctx::GpuCtx;
    use crate::renderer::backend::queue::Queue;
    use crate::renderer::backend::text::TextBackend;
    use crate::text::TextShaper;
    use glam::{UVec2, Vec2};
    use pollster::FutureExt;

    use crate::primitives::color::ColorU8;
    use crate::primitives::span::Span;
    use crate::primitives::urect::URect;
    use crate::renderer::backend::text::test_support::make_inner_run;
    use crate::renderer::render_buffer::text::TextRun;

    const PHYSICAL: UVec2 = UVec2::new(640, 480);

    #[derive(Debug)]
    struct TestGpu {
        device: wgpu::Device,
        queue: Queue,
    }

    fn device_queue() -> TestGpu {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .block_on()
            .expect("request adapter (headless)");
        let mut limits = wgpu::Limits::default();
        limits.max_immediate_size = limits.max_immediate_size.max(16);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("aperture.text_regression.device"),
                required_features: wgpu::Features::IMMEDIATES,
                required_limits: limits,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .expect("request device");
        TestGpu {
            device,
            queue: Queue::new(queue),
        }
    }

    fn run_one_frame(
        device: &wgpu::Device,
        queue: &Queue,
        backend: &mut TextBackend,
        scale: f32,
        runs: &[TextRun],
    ) {
        let mut belt = StagingBelt::new(device.clone(), 1 << 16);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut ctx = GpuCtx::new(device, queue, &mut belt, &mut encoder);
            backend.prepare_batch(&mut ctx, scale, 0, runs);
            backend.flush(&mut ctx);
        }
        belt.finish_and_recall_on_submit(&encoder);
        queue.submit([encoder.finish()]);
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
        let TestGpu { device, queue } = device_queue();
        let shaper = TextShaper::with_bundled_fonts();
        let mut backend = TextBackend::new(&device, shaper.clone());

        let runs = [make_inner_run(
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
        backend.post_record();
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
            .map(|&i| backend.atlas.slots[i as usize].last_use)
            .filter(|&lu| lu != cf)
            .collect();
        assert!(
            stale.is_empty(),
            "cache-hit frame left slots stale: last_use {stale:?} != current_frame {cf}",
        );
        // The refresh must have gone through the entry's *recorded*
        // slab indices — the exact path the hot loop writes.
        for entry in backend.encoded_cache.map.values() {
            for glyph in &backend.encoded_cache.arena[entry.span.range()] {
                let idx = glyph.atlas_slot;
                assert_eq!(
                    backend.atlas.slots[idx as usize].last_use, cf,
                    "recorded slab index {idx} not refreshed on hit",
                );
            }
        }
        // The second frame was a pure hit — nothing should have been
        // re-rasterized or evicted.
        assert_eq!(
            backend.atlas.eviction_count, evictions_after_warmup,
            "a pure cache-hit frame must not evict",
        );

        // Frame 3, after a (simulated) eviction: the count mismatch
        // must reject the entry and force a full re-encode — the old
        // span goes dead in the arena and the rebuilt entry latches
        // the new eviction count.
        let arena_after_hit = backend.encoded_cache.arena.len();
        backend.post_record();
        backend.atlas.eviction_count += 1;
        run_one_frame(&device, &queue, &mut backend, 2.0, &runs);
        assert_eq!(
            backend.encoded_cache.arena.len(),
            2 * arena_after_hit,
            "eviction must invalidate the entry and re-encode (old span left dead)",
        );
        assert_eq!(backend.encoded_cache.map.len(), 1);
        let entry = backend.encoded_cache.map.values().next().unwrap();
        assert_eq!(
            entry.eviction_at, backend.atlas.eviction_count,
            "rebuilt entry must latch the bumped eviction count",
        );
    }

    /// Two batches prepared in one frame ride a single deferred vbuf
    /// write (`TextBackend::flush` after all `prepare_batch` calls). The
    /// per-batch `ranges` must partition the shared instance vec and
    /// each batch's glyphs must keep their own color/placement — same
    /// text at a different origin/color pins this glyph-by-glyph: same
    /// atlas uv + dim, x identical, y shifted by exactly the origin
    /// delta (40 px, integer so subpixel bins match), colors distinct.
    #[test]
    fn deferred_upload_keeps_batches_distinct() {
        let TestGpu { device, queue } = device_queue();
        let shaper = TextShaper::with_bundled_fonts();
        let mut backend = TextBackend::new(&device, shaper.clone());

        let color_a = ColorU8::rgba(240, 240, 240, 255);
        let color_b = ColorU8::rgba(200, 100, 50, 255);
        let run_a = make_inner_run(
            &shaper,
            "File",
            14.0,
            16.8,
            Vec2::new(20.0, 20.0),
            PHYSICAL,
            1.0,
            color_a,
        );
        let run_b = make_inner_run(
            &shaper,
            "File",
            14.0,
            16.8,
            Vec2::new(20.0, 60.0),
            PHYSICAL,
            1.0,
            color_b,
        );

        let mut belt = StagingBelt::new(device.clone(), 1 << 16);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut ctx = GpuCtx::new(&device, &queue, &mut belt, &mut encoder);
            backend.prepare_batch(&mut ctx, 1.0, 0, std::slice::from_ref(&run_a));
            backend.prepare_batch(&mut ctx, 1.0, 1, std::slice::from_ref(&run_b));
            backend.flush(&mut ctx);
        }
        belt.finish_and_recall_on_submit(&encoder);
        queue.submit([encoder.finish()]);

        // Same text → same glyph count n per batch; ranges partition
        // the vec as [0..n] + [n..2n].
        let n = backend.instances.len() / 2;
        assert!(n > 0, "'File' must emit glyphs");
        assert_eq!(backend.ranges[0], Span::new(0, n as u32));
        assert_eq!(backend.ranges[1], Span::new(n as u32, n as u32));

        let a: u32 = bytemuck::cast(color_a);
        let b: u32 = bytemuck::cast(color_b);
        for (ga, gb) in backend.instances[..n]
            .iter()
            .zip(&backend.instances[n..2 * n])
        {
            assert_eq!(ga.color, a);
            assert_eq!(gb.color, b);
            // Identical glyph, identical atlas slot, shifted 40 px down.
            assert_eq!(gb.uv_and_kind, ga.uv_and_kind);
            assert_eq!(gb.dim, ga.dim);
            assert_eq!(gb.pos, [ga.pos[0], ga.pos[1] + 40]);
        }
        backend.post_record();
    }

    /// A run whose lines are partially y-culled by its bounds must not
    /// populate the encoded cache: `EncodedKey` omits bounds, so after
    /// integer-pixel scrolling the same key would replay the truncated
    /// template and newly revealed lines would stay blank forever.
    #[test]
    fn partially_culled_run_is_not_cached() {
        let TestGpu { device, queue } = device_queue();
        let shaper = TextShaper::with_bundled_fonts();
        let mut backend = TextBackend::new(&device, shaper.clone());

        // Three 3-glyph lines at line_height 16 px, origin (0, 0):
        // line tops sit at 0 / 16 / 32.
        let mut run = make_inner_run(
            &shaper,
            "abc\ndef\nxyz",
            14.0,
            16.0,
            Vec2::ZERO,
            PHYSICAL,
            1.0,
            ColorU8::rgba(240, 240, 240, 255),
        );
        // Clip to the first line: the pre-cull keeps lines with
        // line_top <= bounds_bot, so h = 10 keeps line 0 (top 0) and
        // drops lines 1-2 (tops 16, 32).
        run.bounds = URect::new(0, 0, PHYSICAL.x, 10);

        // Frame 1: clipped encode → 1 line * 3 glyphs = 3 instances,
        // and no cache entry.
        run_one_frame(
            &device,
            &queue,
            &mut backend,
            1.0,
            std::slice::from_ref(&run),
        );
        assert_eq!(
            backend.instances.len(),
            3,
            "only line 0's 3 glyphs survive the cull"
        );
        assert!(
            backend.encoded_cache.map.is_empty(),
            "a culled encode must not become a cache template",
        );
        backend.post_record();

        // Frame 2, same clipped run: still a miss, re-encodes to the
        // same 3 instances, still nothing cached.
        run_one_frame(
            &device,
            &queue,
            &mut backend,
            1.0,
            std::slice::from_ref(&run),
        );
        assert_eq!(backend.instances.len(), 3);
        assert!(backend.encoded_cache.map.is_empty());
        backend.post_record();

        // Frame 3, unclipped: 3 lines * 3 glyphs = 9 instances, and
        // the full encode is cached (same key as the clipped frames —
        // that's exactly why the clipped ones must not insert).
        run.bounds = URect::new(0, 0, PHYSICAL.x, PHYSICAL.y);
        run_one_frame(
            &device,
            &queue,
            &mut backend,
            1.0,
            std::slice::from_ref(&run),
        );
        assert_eq!(backend.instances.len(), 9);
        assert_eq!(backend.encoded_cache.map.len(), 1);
        assert_eq!(backend.encoded_cache.arena.len(), 9);
        backend.post_record();

        // Frame 4 replays the cached template: same 9 instances with
        // no re-encode (the arena didn't grow).
        run_one_frame(
            &device,
            &queue,
            &mut backend,
            1.0,
            std::slice::from_ref(&run),
        );
        assert_eq!(backend.instances.len(), 9);
        assert_eq!(backend.encoded_cache.map.len(), 1);
        assert_eq!(
            backend.encoded_cache.arena.len(),
            9,
            "a hit must not re-encode"
        );
    }

    /// A zero-area glyph entry (whitespace) swept by the periodic
    /// empty-entry sweep must re-insert cleanly through `insert_empty`
    /// on next use.
    #[test]
    fn swept_empty_glyph_reinserts() {
        let TestGpu { device, queue } = device_queue();
        let shaper = TextShaper::with_bundled_fonts();
        let mut backend = TextBackend::new(&device, shaper.clone());

        let runs = [make_inner_run(
            &shaper,
            " ",
            14.0,
            16.0,
            Vec2::new(2.0, 2.0),
            PHYSICAL,
            1.0,
            ColorU8::rgba(240, 240, 240, 255),
        )];
        let empties = |b: &TextBackend| {
            b.atlas
                .cache
                .values()
                .filter(|&&i| b.atlas.slots[i as usize].alloc.is_none())
                .count()
        };

        run_one_frame(&device, &queue, &mut backend, 1.0, &runs);
        assert!(
            backend.instances.is_empty(),
            "whitespace prepares a text batch without drawable glyphs",
        );
        assert_eq!(
            empties(&backend),
            1,
            "the space rasterizes to one zero-area entry"
        );
        let first_frame = backend.atlas.current_frame;
        backend.post_record();
        assert_eq!(
            backend.atlas.current_frame,
            first_frame + 1,
            "a prepared zero-instance batch must still advance cache aging",
        );

        // The space's last_use is frame 1. The sweep at frame 512 keeps
        // it (cutoff 512 - 512 = 0 <= 1); the one at frame 1024 drops
        // it (cutoff 512 > 1). Advance prepared text frames that don't
        // touch the space to there.
        let mut belt = StagingBelt::new(device.clone(), 1 << 16);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        while backend.atlas.current_frame < 1024 {
            let mut ctx = GpuCtx::new(&device, &queue, &mut belt, &mut encoder);
            backend.prepare_batch(&mut ctx, 1.0, 0, &[]);
            backend.post_record();
        }
        assert_eq!(
            empties(&backend),
            0,
            "stale empty entry swept at frame 1024"
        );

        // Re-encoding the same run re-inserts the empty entry (the
        // encoded cache was itself swept after 120 idle frames, so this
        // is a full walk through rasterize_and_insert → insert_empty).
        run_one_frame(&device, &queue, &mut backend, 1.0, &runs);
        assert_eq!(
            empties(&backend),
            1,
            "swept empty glyph re-inserts on next use"
        );
        backend.post_record();
    }
}
