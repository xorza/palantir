#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    palantir::bench::alloc_free();
}
