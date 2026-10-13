//! The frame state one cascade run reads.

use crate::display::Display;
use crate::layout::Layout;
use crate::scene::forest::Forest;

/// What a cascade is built from: [`CascadeKey::new`](crate::cascade::cascade_key::CascadeKey::new)
/// hashes it, and [`CascadeEngine::run`](crate::cascade::engine::CascadeEngine::run) walks it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CascadeSource<'a> {
    pub(crate) forest: &'a Forest,
    pub(crate) layout: &'a Layout,
    pub(crate) display: Display,
    /// `TextShaper::font_epoch`: a later font load can move ink extent without moving any rect or authoring.
    pub(crate) font_epoch: u32,
}
