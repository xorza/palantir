#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    aperture::bench::alloc_resize();
}
