//! The allocation bench.
//!
//! Its own target because `dhat::Alloc` has to be *the* global
//! allocator and would tax every timing in `criterion` 10-30x. One
//! bench of a few steps, not several benches — see
//! `palantir::host::bench` for what each step pins.

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    palantir::bench::alloc::run();
}
