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
use std::cell::RefCell;
use std::rc::Rc;

/// Default GPU image cache budget — 256 MB. Holds ~16 full 4K
/// RGBA8 images, or thousands of UI-sized icons. Override at construction
/// via `Host::with_text_and_image_budget`.
pub const DEFAULT_IMAGE_BUDGET_BYTES: u64 = 256 * 1024 * 1024;

/// One uploaded image's GPU footprint. `bind_group` holds internal Arcs
/// to the texture + view; dropping the `GpuImage` frees them.
struct GpuImage {
    bind_group: wgpu::BindGroup,
    /// `width * height * 4` (sRGB RGBA8). `u32` caps each entry at
    /// 4 GB which is well past anything sane.
    bytes: u32,
    /// Frame counter at last successful `draw()`. Initialised to the
    /// frame the entry was uploaded so freshly-uploaded entries can't
    /// be evicted before the user has a chance to draw them.
    last_used_frame: u32,
    /// Stored so eviction can re-form an `ImageHandle` for
    /// `ImageRegistry::mark_pending` without round-tripping through
    /// the registry to look up the image.
    size: glam::U16Vec2,
}

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
    /// `id → GpuImage`. Keyed across frames; entries are reused until
    /// (a) the user calls [`ImageRegistry::unregister`] (CPU only —
    /// GPU side ages out naturally via LRU below), or (b) the
    /// frames-since-used LRU evicts to stay under [`budget_bytes`].
    /// Keyed by `u64` not `ImageHandle` because `ImageHandle::Hash`
    /// keys on `id` only.
    cache: FxHashMap<u64, GpuImage>,
    /// Bumped once per frame in `drain_registry`. Drives the
    /// `last_used_frame` stamps consumed by `end_of_frame_evict`.
    /// `u32` wraps every ~2 years at 60 fps; the sort is ascending and
    /// the only equality check is `last_used_frame == current_frame`,
    /// so a wrapped frame just appears "ancient" and evicts first —
    /// no correctness hazard. Not worth a `u64`.
    frame_id: u32,
    /// Eviction budget. When `total_bytes > budget_bytes` after the
    /// frame's render pass, the oldest entries are evicted until
    /// `total_bytes <= budget_bytes`.
    budget_bytes: u64,
    /// Sum of `bytes` over every entry in [`cache`]. Maintained
    /// incrementally on insert / evict.
    total_bytes: u64,
    /// Per-frame scratch: ids touched by `draw()` since the last
    /// `end_of_frame_evict`. `RefCell` because `draw` takes `&self`
    /// (the schedule walk holds `&self` on the whole backend); the
    /// borrow is hot-loop-shaped but uncontended within a frame
    /// (single-threaded). Drained + applied at end-of-frame.
    touched: RefCell<Vec<u64>>,
}

impl ImagePipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport_buffer: &wgpu::Buffer,
        budget_bytes: u64,
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
            frame_id: 0,
            budget_bytes,
            total_bytes: 0,
            touched: RefCell::new(Vec::new()),
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
        self.frame_id = self.frame_id.wrapping_add(1);
        for (handle, image) in images.drain_pending() {
            let bind_group = self.upload(device, queue, handle.id, &image);
            let bytes = image.width.saturating_mul(image.height).saturating_mul(4);
            let entry = GpuImage {
                bind_group,
                bytes,
                last_used_frame: self.frame_id,
                size: handle.size,
            };
            if let Some(prev) = self.cache.insert(handle.id, entry) {
                self.total_bytes = self.total_bytes.saturating_sub(prev.bytes as u64);
            }
            self.total_bytes = self.total_bytes.saturating_add(bytes as u64);
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
        let Some(entry) = self.cache.get(&handle.id) else {
            tracing::warn!(
                handle_id = format!("{:016x}", handle.id),
                "ImagePipeline::draw: no GPU texture for handle (unregistered between \
                 record and submit, or handle from a different registry?). Skipping draw."
            );
            return;
        };
        // Deferred mark — applied in `end_of_frame_evict`. Direct
        // mutation would need `&mut self`, which the schedule walk
        // can't provide. See `touched` doc.
        self.touched.borrow_mut().push(handle.id);
        pass.set_bind_group(1, &entry.bind_group, &[]);
        pass.draw(0..4, instance..instance + 1);
    }

    /// Apply this frame's `draw`-time touches and evict
    /// least-recently-used entries until `total_bytes <= budget_bytes`.
    /// Call exactly once per frame, *after* `queue.submit` finishes the
    /// render pass (otherwise we could evict a handle this frame's
    /// draws still need).
    ///
    /// Evicted entries are re-queued on the registry via
    /// `mark_pending` so the next sighting re-uploads from the
    /// retained `Rc<Image>`. Entries touched this frame are never
    /// evicted — that would just force an immediate re-upload next
    /// frame for zero memory benefit.
    #[profiling::function]
    pub(crate) fn end_of_frame_evict(&mut self, images: &ImageRegistry) {
        for id in self.touched.borrow_mut().drain(..) {
            if let Some(entry) = self.cache.get_mut(&id) {
                entry.last_used_frame = self.frame_id;
            }
        }
        if self.total_bytes <= self.budget_bytes {
            return;
        }
        let over_before = self.total_bytes;
        let evictions = pick_evictions(
            self.cache
                .iter()
                .map(|(id, e)| (*id, e.last_used_frame, e.bytes)),
            self.frame_id,
            self.total_bytes,
            self.budget_bytes,
        );
        if evictions.is_empty() {
            // Every cached entry was drawn this frame — releasing one
            // would force a same-frame re-upload next frame, so we
            // refuse. Frame stays over budget; the host is holding more
            // live image data than the budget allows. Surface it so
            // hosts notice instead of silently leaking VRAM.
            tracing::error!(
                total_bytes = over_before,
                budget_bytes = self.budget_bytes,
                over_by = over_before - self.budget_bytes,
                live_entries = self.cache.len(),
                "ImagePipeline: over GPU image budget but every cached image was \
                 drawn this frame — nothing evictable. Raise the budget at \
                 Host construction (Host::with_text_and_image_budget) or reduce \
                 concurrent image draws."
            );
            return;
        }
        for id in evictions {
            if let Some(entry) = self.cache.remove(&id) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.bytes as u64);
                images.mark_pending(ImageHandle {
                    id,
                    size: entry.size,
                });
            }
        }
        if self.total_bytes > self.budget_bytes {
            // Evicted everything we could (all untouched entries) and
            // still over budget — the touched-this-frame set alone
            // exceeds the budget. Same remediation as the empty-
            // evictions branch above.
            tracing::error!(
                total_bytes = self.total_bytes,
                budget_bytes = self.budget_bytes,
                over_by = self.total_bytes - self.budget_bytes,
                "ImagePipeline: evicted all untouched entries but still over \
                 budget — touched-this-frame set alone exceeds budget. Raise \
                 the budget at Host construction or reduce concurrent draws."
            );
        }
    }
}

/// Pure eviction policy. Picks ids to drop, oldest-first, skipping any
/// entry touched this frame (`last_used_frame == current_frame`), until
/// projected total drops at or below `budget`. Returned in eviction
/// order. Pulled out as a free fn so it can be unit-tested without a
/// GPU device.
fn pick_evictions(
    entries: impl Iterator<Item = (u64, u32, u32)>,
    current_frame: u32,
    total_bytes: u64,
    budget: u64,
) -> Vec<u64> {
    if total_bytes <= budget {
        return Vec::new();
    }
    let mut candidates: Vec<(u32, u32, u64)> = entries
        .filter(|(_, last, _)| *last != current_frame)
        .map(|(id, last, bytes)| (last, bytes, id))
        .collect();
    // Ascending by last_used_frame (oldest first); tie-break by bytes
    // desc so each eviction frees more.
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut freed: u64 = 0;
    let need = total_bytes - budget;
    let mut out = Vec::new();
    for (_, bytes, id) in candidates {
        if freed >= need {
            break;
        }
        freed = freed.saturating_add(bytes as u64);
        out.push(id);
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: u64, last: u32, bytes: u32) -> (u64, u32, u32) {
        (id, last, bytes)
    }

    #[test]
    fn under_budget_evicts_nothing() {
        let entries = vec![e(1, 5, 100), e(2, 6, 100)];
        let out = pick_evictions(entries.into_iter(), 10, 200, 1024);
        assert!(out.is_empty());
    }

    #[test]
    fn evicts_oldest_first_until_under_budget() {
        let entries = vec![e(1, 1, 100), e(2, 2, 100), e(3, 3, 100)];
        let out = pick_evictions(entries.into_iter(), 10, 300, 150);
        // need to free 150; oldest is id=1 (100), then id=2 (100) → frees 200, done.
        assert_eq!(out, vec![1, 2]);
    }

    #[test]
    fn skips_entries_touched_this_frame() {
        // id=1 is oldest by frame stamp, but matches current_frame → skip.
        let entries = vec![e(1, 10, 100), e(2, 2, 100), e(3, 3, 100)];
        let out = pick_evictions(entries.into_iter(), 10, 300, 150);
        assert_eq!(out, vec![2, 3]);
    }

    #[test]
    fn tie_break_prefers_larger_entry() {
        let entries = vec![e(1, 5, 50), e(2, 5, 200)];
        // Both same age; need 100 freed → pick the larger (id=2).
        let out = pick_evictions(entries.into_iter(), 10, 250, 150);
        assert_eq!(out, vec![2]);
    }

    #[test]
    fn refuses_to_evict_only_touched_entries() {
        // Every entry was touched this frame; eviction yields nothing
        // even though we're over budget. Caller is over budget for one
        // frame — acceptable, next frame's draws may not touch all.
        let entries = vec![e(1, 10, 1_000_000), e(2, 10, 1_000_000)];
        let out = pick_evictions(entries.into_iter(), 10, 2_000_000, 100);
        assert!(out.is_empty());
    }
}
