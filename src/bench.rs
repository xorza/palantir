//! Function-only facade over the colocated benchmark drivers.
//!
//! Every implementation lives in a `bench.rs` beside the code it
//! measures; this module is the single `internals`-gated surface the
//! thin `benches/*.rs` targets link against, so no production module
//! has to be `pub` for a benchmark's sake.

pub use crate::animation::bench::bench as animation;
pub use crate::host::bench::{alloc_free, alloc_free_gpu, alloc_resize};
pub use crate::input::bench::bench as input;
pub use crate::layout::cache::bench::bench as layout_caches;
pub use crate::renderer::backend::bench::bench as record_pass;
pub use crate::renderer::backend::curve_pipeline::bench::bench as curve_pipeline;
pub use crate::renderer::backend::image_pipeline::bench::bench as image_pipeline;
pub use crate::renderer::backend::schedule::bench::bench as schedule;
pub use crate::renderer::backend::text::bench::bench as text_atlas;
pub use crate::renderer::frontend::bench::bench as gradient;
pub use crate::renderer::frontend::composer::bench::bench as composer;
pub use crate::renderer::frontend::composer::text_grid::bench::bench as text_grid;
pub use crate::renderer::gradient_atlas::bench::bench as gradient_atlas;
pub use crate::scene::cascade::bench::bench as cascade;
pub use crate::scene::damage::bench::bench as damage;
pub use crate::scene::tree::paint_anims::bench::bench as paint_anims;
pub use crate::text::bench::bench as text_shape;
pub use crate::ui::bench::{bench as frame, config as frame_config};
pub use crate::ui::bench_fixture::FrameFixture;
pub use crate::widgets::text_edit::bench::bench as text_edit;
