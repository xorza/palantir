//! GPU side of user images. Mirrors [`super::mesh_pipeline::MeshPipeline`]
//! but draws textured quads — per-instance rect + tint, plus a
//! per-image bind group selected at draw time. The CPU texture bytes
//! live in [`crate::ImageRegistry`]; this module drains its pending
//! list each frame and uploads to GPU, then caches the resulting
//! `GpuImage` by [`ImageHandle`] across frames.

use super::pipeline_utils::{
    PipelineRecipe, build_pipeline, build_pipeline_layout, grow_instance_buffer,
};
use crate::primitives::image::{Image, ImageHandle, ImageRegistry};
use crate::renderer::render_buffer::ImageInstance;
use rustc_hash::FxHashMap;
use std::rc::Rc;

pub(crate) struct ImagePipeline {
    pipeline: wgpu::RenderPipeline,
    /// Group 0 (viewport uniform). Group 1 (per-image texture +
    /// sampler) is built inside [`upload`] and stored in [`cache`].
    bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    /// Stencil-test variant — lazy-built when the first rounded-clip
    /// frame uses an image.
    stencil_test: Option<wgpu::RenderPipeline>,
    /// Cached creation inputs needed to lazy-build `stencil_test`.
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    viewport_bgl: wgpu::BindGroupLayout,
    image_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// `id → bind_group`. Keyed across frames; the entry is reused
    /// until the user calls [`ImageRegistry::unregister`] (which just
    /// frees CPU bytes — GPU eviction is roadmapped, see
    /// `docs/roadmap/images.md`). Keyed by `u64` not `ImageHandle`
    /// because `ImageHandle::Hash` keys on `id` only — using the
    /// wrapper would carry the `size` lane through every lookup for
    /// nothing.
    ///
    /// `wgpu::BindGroup` holds internal Arcs to its texture + view, so
    /// dropping the wrapper here also drops the underlying GPU
    /// resources (no separate `texture` / `view` fields needed).
    cache: FxHashMap<u64, wgpu::BindGroup>,
}

impl ImagePipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport_buffer: &wgpu::Buffer,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.image.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
        });

        let viewport_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.image.viewport.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let image_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.image.tex.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.image.viewport.bg"),
            layout: &viewport_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.image.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let pipeline_layout = build_pipeline_layout(
            device,
            "palantir.image.pl",
            &[Some(&viewport_bgl), Some(&image_bgl)],
        );
        let pipeline = build_pipeline(
            device,
            PipelineRecipe {
                label: "palantir.image.pipeline",
                shader: &shader,
                layout: &pipeline_layout,
                vertex_buffers: &[instance_layout()],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                color_format: format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil: None,
            },
        );

        let instance_capacity = 16;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.image.instances"),
            size: (instance_capacity * std::mem::size_of::<ImageInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group,
            instance_buffer,
            instance_capacity,
            stencil_test: None,
            shader,
            color_format: format,
            viewport_bgl,
            image_bgl,
            sampler,
            cache: FxHashMap::default(),
        }
    }

    /// Lazy-build the stencil-test variant for rounded-clip frames.
    /// Idempotent.
    #[profiling::function]
    pub(crate) fn ensure_stencil(&mut self, device: &wgpu::Device) {
        if self.stencil_test.is_some() {
            return;
        }
        let layout = build_pipeline_layout(
            device,
            "palantir.image.pl.stencil",
            &[Some(&self.viewport_bgl), Some(&self.image_bgl)],
        );
        self.stencil_test = Some(build_pipeline(
            device,
            PipelineRecipe {
                label: "palantir.image.pipeline.stencil_test",
                shader: &self.shader,
                layout: &layout,
                vertex_buffers: &[instance_layout()],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                color_format: self.color_format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil: Some(super::stencil::stencil_test_state()),
            },
        ));
    }

    /// Drain pending images from the registry and upload them to GPU.
    /// Called once per frame from `WgpuBackend::submit` before the
    /// render pass starts. After this returns, every handle the
    /// composer routed into a `DrawImage` is guaranteed to have a
    /// `GpuImage` in the cache (or be missing from the registry, in
    /// which case the draw is silently skipped).
    #[profiling::function]
    pub(crate) fn drain_registry(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        images: &ImageRegistry,
    ) {
        for (handle, image) in images.drain_pending() {
            let bind_group = self.upload(device, queue, handle.id, &image);
            self.cache.insert(handle.id, bind_group);
        }
    }

    /// Upload a fresh RGBA8 texture for `id` and build its per-image
    /// bind group. The texture + view are held only by the returned
    /// `BindGroup` — wgpu's internal Arcs keep them alive for the
    /// bind group's lifetime; dropping the wrapper frees the GPU
    /// resources too.
    fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
        image: &Rc<Image>,
    ) -> wgpu::BindGroup {
        let size = wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };
        // Per-handle labels surface in wgpu validation traces so a
        // mis-bound image points to its source asset directly.
        let tex_label = format!("palantir.image.tex.{id:016x}");
        let bg_label = format!("palantir.image.tex.bg.{id:016x}");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&tex_label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // sRGB format: sampler decodes to linear automatically.
            // CPU bytes are sRGB-encoded straight-alpha RGBA8.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            size,
        );
        let view = texture.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&bg_label),
            layout: &self.image_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Sync the per-instance buffer. Single contiguous upload — the
    /// schedule slices by batch at draw time.
    #[profiling::function]
    pub(crate) fn upload_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[ImageInstance],
    ) {
        if instances.is_empty() {
            return;
        }
        grow_instance_buffer(
            device,
            &mut self.instance_buffer,
            &mut self.instance_capacity,
            instances.len(),
            std::mem::size_of::<ImageInstance>(),
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            "palantir.image.instances",
            16,
        );
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
    }

    /// Bind once per pass before iterating image draws.
    pub(crate) fn bind<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, stencil: bool) {
        if stencil {
            let p = self.stencil_test.as_ref().expect("ensure_stencil first");
            pass.set_pipeline(p);
        } else {
            pass.set_pipeline(&self.pipeline);
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
    }

    /// Issue one image draw. `instance` indexes into the per-frame
    /// instance buffer. `handle` selects the bind group; misses log
    /// a warning and skip — a miss means either (a) the user
    /// unregistered between record and submit (legal but exotic), or
    /// (b) the registry never saw `handle` because it was minted by a
    /// different `ImageRegistry` clone (caller bug).
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        handle: ImageHandle,
        instance: u32,
    ) {
        let Some(bg) = self.cache.get(&handle.id) else {
            tracing::warn!(
                handle_id = format!("{:016x}", handle.id),
                "ImagePipeline::draw: no GPU texture for handle (unregistered between \
                 record and submit, or handle from a different registry?). Skipping draw."
            );
            return;
        };
        pass.set_bind_group(1, bg, &[]);
        pass.draw(0..4, instance..instance + 1);
    }
}

const IMAGE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x2, // rect.min
    1 => Float32x2, // rect.size
    2 => Float32x2, // uv_min
    3 => Float32x2, // uv_size
    // `Unorm8x4` normalizes `u8/255 → 0..1`. Tint is linear straight-alpha
    // on the CPU; shader multiplies by the sampled texel and premultiplies
    // at write.
    4 => Unorm8x4,  // tint
];

fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ImageInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &IMAGE_INSTANCE_ATTRS,
    }
}
