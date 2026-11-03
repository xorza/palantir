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
