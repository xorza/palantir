//! One plane's frozen input to the layout walk.

use crate::primitives::text::interned_text::InternedText;
use crate::scene::tree::Tree;

/// What one plane's walk reads and never writes: its tree, and the text arena
/// its runs resolve against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlaneInput<'a> {
    pub(crate) tree: &'a Tree,
    pub(crate) interned_text: &'a InternedText<'a>,
}
