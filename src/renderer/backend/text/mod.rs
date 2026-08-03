//! Palantir-native glyph atlas + text render pipeline.
//!
//! Built to Palantir's contracts:
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

mod atlas;
#[cfg(feature = "internals")]
pub(crate) mod bench;
mod encode;

use crate::primitives::interned_str::InternedText;
use crate::primitives::span::Span;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{ColorVariantSpec, StencilVariant};
use crate::renderer::backend::viewport::ViewportPush;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::render::RunPlacement;
use crate::text::shaper::TextShaper;

use encode::{TextEncoder, encode_key_for};

/// One per-instance vertex record. 20 bytes, `Pod`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GlyphInstance {
    pos: [i32; 2],
    dim: u32,
    uv_and_kind: u32,
    color: u32,
}

/// `[color_atlas_size, mask_atlas_size]` follows `ViewportPush` in the
/// shared immediate region.
const PARAMS_OFFSET: u32 = 8;
const PARAMS_BYTES: usize = std::mem::size_of::<[u32; 2]>();
const _: () = assert!(PARAMS_BYTES == 8);

/// 0 = mask, 1 = color. Encoded in the high bit of `uv.u`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum ContentType {
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
            Self::Mask => "palantir text mask atlas",
            Self::Color => "palantir text color atlas",
        }
    }
}

#[derive(Debug)]
pub(crate) struct TextBackend {
    shaper: TextShaper,
    encoder: TextEncoder,

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

    vbuf: DynamicBuffer<GlyphInstance>,

    /// Per-batch slice of the encoder's `instances`; empty span =
    /// nothing to draw.
    ranges: Vec<Span>,
}

impl TextBackend {
    /// Build the format-independent text resources (glyph atlas, shaper,
    /// caches, shader, vertex buffer). The render pipelines are built per
    /// format by [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variants`].
    pub(crate) fn new(device: &wgpu::Device, shaper: TextShaper) -> Self {
        let encoder = TextEncoder::new(device);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.text.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

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

        let bindings = encoder.atlas.bindings();
        let atlas_px = bindings.atlas_px;

        let atlas_bg = build_atlas_bg(
            device,
            &atlas_bgl,
            bindings.mask_view,
            bindings.color_view,
            &sampler,
        );

        let vbuf = DynamicBuffer::<GlyphInstance>::vertex(device, "palantir text vbuf", 4096);

        Self {
            shaper,
            encoder,
            shader,
            atlas_bgl,
            atlas_bg,
            sampler,
            atlas_px,
            vbuf,
            ranges: Vec::new(),
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
                label: "palantir.text.pipeline",
                stencil_label: "palantir.text.pipeline.stencil_test",
                layout_label: "palantir.text.pl",
                shader: &self.shader,
                bind_group_layouts: &[Some(&self.atlas_bgl)],
                vertex_buffers: &[Some(glyph_instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Append-mode prepare. Encoded-cache hits bypass shaping; the
    /// first miss opens the exclusive shaper session, and each miss
    /// extracts and rasterizes its glyphs in place. Rebinds the atlas
    /// bind group if it grew.
    #[profiling::function]
    pub(crate) fn prepare_batch(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        scale: f32,
        batch_idx: usize,
        runs: &[TextDrawRow],
        interned_text: &InternedText<'_>,
    ) {
        debug_assert_eq!(
            batch_idx,
            self.ranges.len(),
            "text batches must be prepared once in contiguous order",
        );
        let start = self.encoder.instances.len() as u32;

        // One walk: hits emit straight to `instances`; misses encode
        // through the lazily-opened session. An all-hit frame never
        // cracks the RefCell or hits cosmic.
        let mut session = None;
        for r in runs {
            if r.text.key.is_invalid() {
                // Backstop: the encoder already drops runs with no shaped
                // buffer, so nothing production-emitted reaches this.
                continue;
            }
            let run_key = encode_key_for(r, scale);
            if self.encoder.try_emit_cached(&run_key) {
                continue;
            }
            let session = session.get_or_insert_with(|| self.shaper.render_session());
            self.encoder.encode_run(
                ctx.device,
                session,
                r.text.resolve_request(interned_text),
                RunPlacement {
                    origin: r.origin,
                    scale: scale * r.scale,
                    bounds: r.bounds,
                },
                run_key,
            );
        }
        drop(session);

        let end = self.encoder.instances.len() as u32;

        // Rebuild bind group if atlas grew during encode.
        if self.encoder.atlas.bind_group_dirty {
            let bindings = self.encoder.atlas.bindings();
            self.atlas_bg = build_atlas_bg(
                ctx.device,
                &self.atlas_bgl,
                bindings.mask_view,
                bindings.color_view,
                &self.sampler,
            );
            self.atlas_px = bindings.atlas_px;
            self.encoder.atlas.bind_group_dirty = false;
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
        self.vbuf.upload_instances(ctx, &self.encoder.instances);
        self.encoder.atlas.flush_pending_uploads(ctx);
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

    /// Frame teardown, run for every submit — including one that
    /// prepared no text batch at all.
    ///
    /// `end_frame`, not `post_record`: this runs as the last step of
    /// `WgpuBackend::submit`, nowhere near a record pass, and the crate
    /// spends `post_record` on the record half of a frame
    /// (`FrameCycle`, `Forest`, `Tree`). It belongs with the other
    /// frame-boundary teardowns instead — `TextSystem::end_frame` is its
    /// opposite number on the record side.
    ///
    /// It used to return early on an empty `ranges`, which froze this
    /// side's clock on any frame whose damage happened to miss every
    /// text run while the shaper's kept advancing. Both caches now age
    /// against the shaper's clock
    /// ([`TextShaper::frame`](crate::text::shaper::TextShaper::frame)),
    /// so there is nothing to skip: sweeping a text-free frame is what
    /// keeps `ENCODED_CACHE_KEEP_FRAMES` a bound on retention rather
    /// than a bound on text-bearing frames.
    pub(crate) fn end_frame(&mut self) {
        debug_assert!(
            !self.ranges.is_empty() || self.encoder.instances.is_empty(),
            "instances were emitted without a batch range to draw them",
        );
        self.encoder.end_frame(self.shaper.frame());
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

// `internals` only, not `any(test, …)`: both consumers — the `text_atlas`
// benchmark and the GPU regression suite in `tests.rs` — are gated on
// that feature, so a plain `cargo test` build has no caller.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::renderer::backend::text::TextBackend;

    impl TextBackend {
        /// One frame boundary the way a window drives it: advance the
        /// shared text clock — owned by the record pass in production,
        /// where `TextSystem::end_frame` ticks it before the submit —
        /// then sweep this side against it.
        ///
        /// Harnesses that drive a `TextBackend` with no `Ui` behind it
        /// have no other way to age these caches, since
        /// [`TextBackend::end_frame`] only *reads* the clock.
        pub(crate) fn tick_frame(&mut self) {
            self.shaper.tick_frame();
            self.end_frame();
        }
    }
}

#[cfg(test)]
mod tests;
