//! Internals-gated benchmark facade and cross-subsystem workloads.
//!
//! Subdirectories mirror the production hierarchy so every Criterion driver
//! has one source-level home without making production modules own benchmark
//! children.

mod frame;
mod input;
mod layout;
mod renderer;
mod text;
mod ui;

pub use crate::bench::frame::bench as frame;
pub use crate::bench::frame::config as frame_config;
pub use crate::bench::frame::fixture::FrameFixture;
pub use crate::bench::frame::text_ui;
pub use crate::bench::input::bench as input;
pub use crate::bench::layout::cache::bench as layout_caches;
pub use crate::bench::renderer::backend::text::bench as text_atlas;
pub use crate::bench::renderer::frontend::composer::bench as composer;
pub use crate::bench::text::bench as text_shape;
pub use crate::bench::ui::cascade::bench as cascade;
pub use crate::bench::ui::damage::bench as damage;
