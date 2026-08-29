//! The theme half of a container's chrome.

use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::background::Background;

/// What a container paints and how it clips when the caller named
/// neither.
///
/// The pair a theme's panel slot supplies, named once so the three
/// containers that take it —
/// [`Panel`](crate::Panel), [`Grid`](crate::Grid) and
/// [`Popup`](crate::Popup) — do not each write down which two theme
/// fields the fallback is. Read it with
/// [`Theme::container_chrome`](crate::Theme::container_chrome) and hand
/// it to
/// [`Node::resolve_container_chrome`](crate::scene::node::Node::resolve_container_chrome).
///
/// Borrowed, because `Widget::record` takes an `Option<&Background>` at
/// the end of it: owning the answer meant cloning the theme's 124 bytes
/// for every container that named no chrome of its own, which is most of
/// them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContainerChrome<'a> {
    pub(crate) background: Option<&'a Background>,
    pub(crate) clip: ClipMode,
}
