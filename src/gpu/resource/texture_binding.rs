//! [`TextureBinding`]: the texture-plus-sampler group every texture-sampling shader binds, and the
//! group of an effect's images.

use std::array;

use crate::effect::MAX_IMAGE_SLOTS;
use crate::gpu::device::gpu::Gpu;

/// How a shader reads a texture: through a filtering sampler, or texel by texel, which a format
/// that no filter reads needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Filtering {
    Filtered,
    Loaded,
}

/// The layout every sampled texture binds through, with its sampler: registered images, `GpuView` targets, and the backbuffer when drawn onto a no-copy target; and the layouts of the frame group, an effect's images and a backdrop's composite.
///
/// Built once per device, by the host as it opens the backend, which hands a copy to the image store. `Clone` shares `wgpu`'s ref-counted handles, so every pipeline composes over the same layout and a group built for one binds in any.
#[derive(Clone, Debug)]
pub(crate) struct TextureBinding {
    layout: wgpu::BindGroupLayout,
    /// An effect's image group for each count of slots from 1: the sampler at binding 0, then each
    /// image at the binding after, as `effect/contract.rs` declares them. One group for all of a
    /// draw's images, as a material's textures share one (Toji, "WebGPU Bind Group best
    /// practices"), so a draw binds them at once and an effect needs two groups of the four the
    /// baseline grants. Refused: a group per image, which needs five.
    images: [wgpu::BindGroupLayout; MAX_IMAGE_SLOTS as usize],
    /// The group a backdrop's composite reads its result through, a material's and an app's
    /// backdrop effect's alike: the texture its result lies in, a sampler, the slot table and the
    /// spectrum a lens spreads.
    backdrops: wgpu::BindGroupLayout,
    /// The frame group every color pipeline reads at
    /// [`FRAME_GROUP`](crate::primitives::wgsl::FRAME_GROUP): the gradient rows, its sampler and
    /// the frame's rounded clips (`primitives/wgsl/frame.wgsl`).
    frame: wgpu::BindGroupLayout,
    /// Linear within a mip, nearest between, clamped on all axes. Safe because no user passes a coordinate outside `0..1`: the gradient shader applies [`Spread`](crate::primitives::paint::brush::gradient::Spread) to `t` first and the image shader `fract`s uv under `FLAG_TILED`. Nearest image filtering is a shader-side snap, so all filters ride this sampler.
    sampler: wgpu::Sampler,
}

impl TextureBinding {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.texture.bgl"),
                entries: &[texture_entry(0, Filtering::Filtered), sampler_entry(1)],
            });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.texture.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let images = array::from_fn(|idx| {
            let entries: Vec<wgpu::BindGroupLayoutEntry> = [sampler_entry(0)]
                .into_iter()
                .chain(
                    (1..=idx as u32 + 1).map(|binding| texture_entry(binding, Filtering::Filtered)),
                )
                .collect();
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("palantir.effect.images.bgl"),
                    entries: &entries,
                })
        });
        let backdrops = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.backdrop.composite.bgl"),
                entries: &[
                    texture_entry(0, Filtering::Filtered),
                    sampler_entry(1),
                    storage_entry(2),
                    storage_entry(3),
                ],
            });
        let frame = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.frame.bgl"),
                entries: &[
                    texture_entry(0, Filtering::Filtered),
                    sampler_entry(1),
                    storage_entry(2),
                ],
            });
        Self {
            layout,
            images,
            backdrops,
            frame,
            sampler,
        }
    }

    /// The layout of the frame group.
    pub(crate) const fn frame_layout(&self) -> &wgpu::BindGroupLayout {
        &self.frame
    }

    /// The frame group over the gradient rows' `atlas` view and the rounded clips in `clips`.
    pub(crate) fn frame_group(
        &self,
        gpu: &Gpu,
        atlas: &wgpu::TextureView,
        clips: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.frame.bg"),
            layout: &self.frame,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: clips.as_entire_binding(),
                },
            ],
        })
    }

    /// The layout of the group a backdrop's composite reads its result through.
    pub(crate) const fn backdrop_layout(&self) -> &wgpu::BindGroupLayout {
        &self.backdrops
    }

    /// The group a backdrop's composite reads `result` through, the texture its result lies in,
    /// with this sampler, the slot table `slots` and the spectrum a lens spreads, as
    /// `effect/backdrop.wgsl` declares them.
    pub(crate) fn backdrop_group(
        &self,
        gpu: &Gpu,
        result: &wgpu::TextureView,
        slots: &wgpu::Buffer,
        spectrum: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.backdrop.composite"),
            layout: &self.backdrops,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(result),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: slots.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: spectrum.as_entire_binding(),
                },
            ],
        })
    }

    /// The layout of one sampled texture's group.
    pub(crate) const fn texture_layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The backend's one sampler, which a group of a layout of its own binds too: the backdrop
    /// blur's.
    pub(crate) const fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// The layout of an effect's image group of `slots` images, from 1.
    pub(crate) fn images_layout(&self, slots: u8) -> &wgpu::BindGroupLayout {
        assert!(
            (1..=MAX_IMAGE_SLOTS).contains(&slots),
            "an image group holds 1 to {MAX_IMAGE_SLOTS} images, not {slots}"
        );
        &self.images[usize::from(slots) - 1]
    }

    /// The group binding `views`, an effect draw's images in slot order, through its layout and
    /// this sampler.
    pub(crate) fn images_group(&self, gpu: &Gpu, views: &[&wgpu::TextureView]) -> wgpu::BindGroup {
        let layout = self.images_layout(u8::try_from(views.len()).expect("at most 4 images"));
        let entries: Vec<wgpu::BindGroupEntry<'_>> = [wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
        }]
        .into_iter()
        .chain(
            (1..)
                .zip(views)
                .map(|(binding, view)| wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::TextureView(view),
                }),
        )
        .collect();
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.effect.images.bg"),
            layout,
            entries: &entries,
        })
    }

    /// The group binding `view` through the texture layout and this sampler.
    pub(crate) fn texture_group(
        &self,
        gpu: &Gpu,
        view: &wgpu::TextureView,
        label: &str,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

/// One fragment-visible read-only storage buffer entry, as the slot tables and the clip pool
/// declare them.
pub(crate) const fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// One fragment-visible 2D float texture entry, the only texture shape any shader declares,
/// read through a sampler or loaded texel by texel as `filtering` says, so every layout
/// spells it alike.
pub(crate) const fn texture_entry(
    binding: u32,
    filtering: Filtering,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float {
                filterable: matches!(filtering, Filtering::Filtered),
            },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// A fragment-visible filtering sampler entry, beside a [`texture_entry`].
pub(crate) const fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
