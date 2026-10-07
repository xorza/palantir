//! Every criterion driver in the crate, in one target.
//!
//! The runner lives in the library (`palantir::bench`): the drivers are `pub(crate)` and reach crate privates, and keeping selection there makes it unit-testable, which a `harness = false` target never is.

use palantir::bench;
fn main() {
    bench::run();
}
