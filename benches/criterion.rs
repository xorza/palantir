//! Every criterion driver in the crate, in one target.
//!
//! The runner lives in the library (`palantir::bench`): the drivers are
//! `pub(crate)` and reach crate privates, so a separate crate cannot
//! name them — and keeping the selection logic there makes it
//! unit-testable, which a `harness = false` target never is.

fn main() {
    palantir::bench::run();
}
