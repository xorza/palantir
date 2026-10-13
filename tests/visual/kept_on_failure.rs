#![expect(
    clippy::print_stderr,
    reason = "a written frame is reported to the terminal so the run says where it went"
)]

use std::fs;
use std::path::Path;
use std::thread;

use palantir::golden::image::RgbaImage;

use crate::goldens;

/// Writes `image` to `output/<name>/actual.png` if the test panics while this is alive: the artifact for a pixel assertion with no expected image.
#[derive(Debug)]
pub(crate) struct KeptOnFailure<'a> {
    name: &'a str,
    image: &'a RgbaImage,
}

impl<'a> KeptOnFailure<'a> {
    pub(crate) const fn new(name: &'a str, image: &'a RgbaImage) -> Self {
        Self { name, image }
    }
}

impl Drop for KeptOnFailure<'_> {
    fn drop(&mut self) {
        if !thread::panicking() {
            return;
        }
        let output = Path::new(goldens::ROOT).join("output").join(self.name);
        // A failed write must not turn the panic into an abort.
        if fs::create_dir_all(&output).is_ok() && self.image.save(output.join("actual.png")).is_ok()
        {
            eprintln!("`{}`: frame written to {}", self.name, output.display());
        }
    }
}
