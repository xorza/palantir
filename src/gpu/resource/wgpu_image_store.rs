//! GPU textures and bind groups for registered images, created, rewritten and
//! freed as the registry asks.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target;
use crate::image_registry::image_store::ImageStore;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::Image;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::array;
use std::cell::{Ref, RefCell, RefMut};

/// The [`ImageStore`] a host's registry writes through and its backend draws
/// from. They share one `Rc`, so the device side sits behind a `RefCell`; a draw takes
/// one [`Self::read`] per pass.
///
/// Built before any device, as the registry every recorder clones holds it from the start; the
/// host [attaches](Self::attach) each device it draws with, before any image registers.
#[derive(Debug)]
pub(crate) struct WgpuImageStore {
    /// `None` until the first attach.
    attached: RefCell<Option<AttachedStore>>,
    /// Staging for [`premultiply_into`], reused across updates. Opaque images skip
    /// it ([`Self::write`]).
    staged: RefCell<Vec<SrgbaU8>>,
    /// Built with the store, at startup: the build takes milliseconds, which shouldn't
    /// land on the frame that registers a soft-edged image.
    premultiply: Box<[[u8; 256]; 256]>,
}

/// The store's side of one device: the textures it holds on it.
#[derive(Debug)]
struct AttachedStore {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The device's `max_texture_dimension_2d`, which an image registered on an earlier device
    /// can exceed.
    max_texture_dim: u32,
    /// The group-0 layout and sampler every pipeline of the backend composes over. Built here
    /// because the host attaches this store before it builds the backend, which takes its copy
    /// from [`WgpuImageStore::binding`].
    binding: TextureBinding,
    textures: FxHashMap<TextureId, ImageTexture>,
}

#[derive(Debug)]
pub(crate) struct ImageTexture {
    texture: wgpu::Texture,
    pub(crate) bind_group: wgpu::BindGroup,
}

const ATTACHED: &str = "the host attaches a device before it draws or registers an image";

/// A store with no device yet.
impl Default for WgpuImageStore {
    fn default() -> Self {
        Self {
            attached: RefCell::default(),
            staged: RefCell::default(),
            premultiply: premultiplied_bytes(),
        }
    }
}

impl WgpuImageStore {
    /// Hold textures on `gpu` from now on. The textures of an earlier device drop: every image
    /// paints nothing until its next write.
    pub(crate) fn attach(&self, gpu: &Gpu) {
        *self.attached.borrow_mut() = Some(AttachedStore {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            max_texture_dim: gpu.max_texture_dim().get(),
            binding: TextureBinding::new(&gpu.device),
            textures: FxHashMap::default(),
        });
    }

    /// The binding of the device attached now.
    pub(crate) fn binding(&self) -> TextureBinding {
        self.attached().binding.clone()
    }

    /// One borrow per render traversal, avoiding a `RefCell` probe per run.
    pub(crate) fn read(&self) -> Ref<'_, FxHashMap<TextureId, ImageTexture>> {
        Ref::map(self.attached(), |attached| &attached.textures)
    }

    fn attached(&self) -> Ref<'_, AttachedStore> {
        Ref::map(self.attached.borrow(), |attached| {
            attached.as_ref().expect(ATTACHED)
        })
    }

    fn attached_mut(&self) -> RefMut<'_, AttachedStore> {
        RefMut::map(self.attached.borrow_mut(), |attached| {
            attached.as_mut().expect(ATTACHED)
        })
    }
}

/// An empty texture at `size` and its bind group; the write that follows fills it.
fn create(
    device: &wgpu::Device,
    binding: &TextureBinding,
    id: TextureId,
    size: UVec2,
) -> ImageTexture {
    let raw_id = id.0;
    let texture_label = format!("palantir.image.tex.{raw_id:016x}");
    let bind_group_label = format!("palantir.image.tex.bg.{raw_id:016x}");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&texture_label),
        size: render_target::extent(size),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = binding.bind_group(device, &view, &bind_group_label);
    ImageTexture {
        texture,
        bind_group,
    }
}

impl ImageStore for WgpuImageStore {
    fn write(&self, id: TextureId, image: &Image) {
        let mut attached = self.attached_mut();
        // Registration checks the device it ran on; a later device can be smaller.
        if image.size.max_element() > attached.max_texture_dim {
            tracing::warn!(
                size = ?image.size,
                max_dimension = attached.max_texture_dim,
                "an image exceeds this device's texture limit and paints nothing"
            );
            return;
        }
        let AttachedStore {
            device,
            queue,
            binding,
            textures,
            ..
        } = &mut *attached;
        let entry = textures
            .entry(id)
            .or_insert_with(|| create(device, binding, id, image.size));
        debug_assert_eq!(
            render_target::texture_size(&entry.texture),
            image.size,
            "a write matches the texture it lands in",
        );
        // A pitch off `COPY_BYTES_PER_ROW_ALIGNMENT` is re-packed inside wgpu; padding the rows here
        // would not save that, as wgpu copies the whole into staging either way.
        let write = |data: &[u8]| {
            queue.write_texture(
                entry.texture.as_image_copy(),
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.size.x * 4),
                    rows_per_image: Some(image.size.y),
                },
                render_target::extent(image.size),
            );
        };
        // An image with no transparency is already premultiplied (scale by one), so
        // it is written as is rather than copied into a second buffer.
        let texels = image.texels();
        if texels.iter().all(|texel| texel.a == u8::MAX) {
            write(&image.pixels);
            return;
        }
        let mut staged = self.staged.borrow_mut();
        premultiply_into(&self.premultiply, texels, &mut staged);
        write(bytemuck::cast_slice(&staged));
    }

    fn free(&self, id: TextureId) {
        self.attached_mut().textures.remove(&id);
    }
}

/// `texels` with every color scaled by its own alpha, still sRGB-encoded:
/// what the texture holds and an [`Image`] does not.
///
/// **The filter runs before the shader**, so texels must already carry their
/// coverage: straight color blended halfway to transparent black leaves a
/// dark (or colored) fringe once the shader multiplies by alpha. The image
/// shader's header states the contract.
///
/// The scale is in linear light, which the sRGB texture decodes to and the
/// filter blends.
///
/// The caller tests the alpha, since the error needs differing neighbor
/// alphas, a property of the image. The raster atlases keep straight alpha
/// because their shader reads one texel per pixel and the icon path
/// premultiplies its own taps.
///
/// Paid once per upload; an image refilled every frame pays every frame.
fn premultiply_into(table: &[[u8; 256]; 256], texels: &[SrgbaU8], out: &mut Vec<SrgbaU8>) {
    out.clear();
    out.reserve_exact(texels.len());
    out.extend(texels.iter().map(|texel| {
        let row = &table[texel.a as usize];
        SrgbaU8 {
            r: row[texel.r as usize],
            g: row[texel.g as usize],
            b: row[texel.b as usize],
            a: texel.a,
        }
    }));
}

/// `encode(decode(value) * alpha)` for every byte pair, indexed `[alpha][value]`.
///
/// The arithmetic (an sRGB decode and an iterative encode) is what costs, so
/// it runs 65536 times once instead of per channel per texel. 64 KiB, one per
/// store.
fn premultiplied_bytes() -> Box<[[u8; 256]; 256]> {
    let decoded: [f32; 256] = array::from_fn(|value| {
        RgbaF32::from(SrgbaU8 {
            r: value as u8,
            g: 0,
            b: 0,
            a: u8::MAX,
        })
        .r
    });
    let mut table: Box<[[u8; 256]; 256]> =
        vec![[0u8; 256]; 256].into_boxed_slice().try_into().unwrap();
    for (alpha, row) in table.iter_mut().enumerate() {
        let coverage = alpha as f32 / 255.0;
        for (value, out) in row.iter_mut().enumerate() {
            *out = RgbaF32 {
                r: decoded[value] * coverage,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }
            .to_srgba8()
            .r;
        }
    }
    // Full coverage must reach the GPU as the bytes it was handed, but the
    // round trip drifts by an LSB, so the identity is stated.
    table[usize::from(u8::MAX)] = array::from_fn(|value| value as u8);
    table
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::resource::wgpu_image_store::WgpuImageStore;

    impl WgpuImageStore {
        /// Registered images resident on the GPU; surface-format-change tests assert
        /// it survives a pipeline rebuild.
        pub(crate) fn resident(&self) -> usize {
            self.read().len()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::wgpu_image_store::{
        WgpuImageStore, premultiplied_bytes, premultiply_into,
    };
    use crate::gpu::test_gpu;
    use crate::image_registry::ImageRegistry;
    use crate::image_registry::image_handle::ImageHandle;
    use crate::primitives::identity::texture_id::TextureId;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::srgba_u8::SrgbaU8;
    use crate::primitives::paint::image::Image;
    use glam::UVec2;
    use std::rc::Rc;

    /// Must match the arithmetic over every byte pair, except full coverage,
    /// which is the identity.
    #[test]
    fn the_premultiply_table_answers_for_the_arithmetic() {
        let texels: Vec<SrgbaU8> = (0..=u8::MAX)
            .flat_map(|a| {
                (0..=u8::MAX).map(move |v| SrgbaU8 {
                    r: v,
                    g: v,
                    b: v,
                    a,
                })
            })
            .collect();
        let mut out = Vec::new();
        premultiply_into(&premultiplied_bytes(), &texels, &mut out);

        for (texel, got) in texels.iter().zip(&out) {
            let straight = RgbaF32::from(*texel);
            let want = match texel.a {
                u8::MAX => *texel,
                _ => RgbaF32 {
                    r: straight.r * straight.a,
                    g: straight.g * straight.a,
                    b: straight.b * straight.a,
                    a: straight.a,
                }
                .to_srgba8(),
            };
            assert_eq!(*got, want, "premultiplied {texel:?}");
        }
    }

    #[test]
    fn a_gpu_texture_lives_exactly_as_long_as_its_handle() {
        let gpu = test_gpu::headless_test_gpu();
        let store = Rc::new(WgpuImageStore::default());
        store.attach(&gpu);
        let weak = Rc::downgrade(&store);
        let registry = ImageRegistry::new(Rc::clone(&store));
        assert!(!store.read().contains_key(&TextureId(1)));

        let mut image = Image::blank(UVec2::splat(2));
        let handle = ImageHandle::new(TextureId(1), &image, registry.clone());
        assert_eq!(store.resident(), 1);
        let registered = store.read()[&TextureId(1)].bind_group.clone();
        assert!(!store.read().contains_key(&TextureId(2)));
        image.texels_mut().fill(SrgbaU8::hex(0x4cd3ff));
        handle.update(&image);
        assert_eq!(handle.generation(), 1);
        assert_eq!(
            store.read()[&TextureId(1)].bind_group,
            registered,
            "a write keeps the texture and its binding",
        );
        let clone = handle.clone();
        drop(handle);
        assert_eq!(store.resident(), 1, "a clone keeps the texture");
        drop(clone);
        assert_eq!(store.resident(), 0);
        assert!(!store.read().contains_key(&TextureId(1)));

        let survivor = ImageHandle::new(TextureId(2), &image, registry);
        store.attach(&test_gpu::headless_test_gpu());
        assert_eq!(
            store.resident(),
            0,
            "an attach drops the textures of the old device"
        );
        survivor.update(&image);
        assert_eq!(store.resident(), 1, "a write fills the image again");
        drop(store);
        survivor.update(&image);
        assert_eq!(survivor.generation(), 2);
        assert_eq!(weak.upgrade().unwrap().resident(), 1);
        drop(survivor);
        assert!(
            weak.upgrade().is_none(),
            "the last image handle releases a store whose other owners are gone",
        );
    }
}
