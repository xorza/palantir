//! An image the showcase makes in code, kept with the function that makes it, so a device that
//! holds none of its texels gets them again from that function rather than from a CPU copy.

use palantir::{Image, ImageHandle, Ui};

/// A registered image and the function that makes its texels.
#[derive(Clone, Debug)]
pub(crate) struct MadeImage {
    handle: ImageHandle,
    make: fn() -> Image,
}

impl MadeImage {
    pub(crate) fn new(ui: &Ui, make: fn() -> Image) -> Self {
        let handle = ui
            .load_image(&make())
            .expect("a showcase image fits every supported GPU");
        Self { handle, make }
    }

    /// The handle to paint with, its texels written again first when the device attached now
    /// does not hold them.
    pub(crate) fn handle(&self) -> &ImageHandle {
        if !self.handle.is_resident() {
            self.handle
                .update(&(self.make)())
                .expect("a showcase image fits every supported GPU");
        }
        &self.handle
    }
}

#[cfg(test)]
mod tests {
    use palantir::glam::UVec2;
    use palantir::internals::headless_test_gpu;
    use palantir::{Harness, HarnessDepth, Image, ImagePaint, Shape, Ui};

    use crate::made_image::MadeImage;

    /// A red and a blue texel, side by side.
    fn two_texels() -> Image {
        Image::from_srgba8(
            UVec2::new(2, 1),
            [[255, 0, 0, 255], [0, 0, 255, 255]].concat(),
        )
        .unwrap()
    }

    /// A new device holds none of the image, and the next [`MadeImage::handle`] writes its texels
    /// again from the function that made them: the frame paints as it did on the old device.
    #[test]
    fn a_new_device_gets_the_texels_again_from_the_function() {
        let gpu = headless_test_gpu();
        let mut h = Harness::builder(UVec2::new(8, 4))
            .depth(HarnessDepth::gpu(gpu.handles()))
            .build();
        let made = MadeImage::new(h.ui(), two_texels);
        let mut record = |ui: &mut Ui| {
            let _ = ui.add_shape(Shape::rect(), ImagePaint::new(made.handle().clone()));
        };
        h.frame(&mut record);
        let before = h.read_back();
        let image = made.handle().clone();
        assert!(image.is_resident());

        h.recover(gpu.handles());
        assert!(!image.is_resident(), "a new device holds none of it");
        h.frame(&mut record);
        assert!(image.is_resident(), "the frame wrote it again");
        assert!(h.read_back() == before, "it paints as on the old device");
    }
}
