//! Internals-gated benchmark facade and cross-subsystem workloads.
//!
//! Subdirectories mirror the production hierarchy so every benchmark driver
//! has one source-level home without making production modules own benchmark
//! children.

mod allocation;
mod animation;
mod frame;
mod input;
mod layout;
mod renderer;
mod text;
mod ui;

pub use crate::bench::allocation::free::bench as alloc_free;
pub use crate::bench::allocation::free_gpu::bench as alloc_free_gpu;
pub use crate::bench::allocation::resize::bench as alloc_resize;
pub use crate::bench::animation::bench as animation;
pub use crate::bench::frame::bench as frame;
pub use crate::bench::frame::config as frame_config;
pub use crate::bench::frame::fixture::FrameFixture;
pub use crate::bench::frame::text_ui;
pub use crate::bench::input::bench as input;
pub use crate::bench::layout::cache::bench as layout_caches;
pub use crate::bench::renderer::backend::curve::bench as curve_pipeline;
pub use crate::bench::renderer::backend::text::bench as text_atlas;
pub use crate::bench::renderer::frontend::composer::bench as composer;
pub use crate::bench::text::bench as text_shape;
pub use crate::bench::ui::cascade::bench as cascade;
pub use crate::bench::ui::damage::bench as damage;
