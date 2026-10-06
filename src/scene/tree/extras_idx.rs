//! Sparse side-table indices for optional per-node data.

use crate::common::index16::Index16;

/// One node's slots in the three optional side tables. `None` means the
/// node carries none of that kind, which is the common case — the tables
/// are sparse so a plain leaf pays six bytes here instead of a full row
/// in each.
///
/// # Ceiling
///
/// [`Index16`] caps each table at
/// [`Index16::LAST`]` + 1` rows **per layer, per frame**, and a row past
/// that panics in release rather than wrapping. What fills each:
///
/// - `bounds` — a node with a non-default `BoundsExtras`: any grid child
///   off cell `(0, 0)`, any canvas child at a nonzero position, any
///   explicit `min_size` / `max_size`.
/// - `panel` — a node with non-default panel columns.
/// - `chrome` — a node with a paintable [`Background`], or one clipping
///   to rounded corners. A themed button or card takes one, so this is
///   the table a large scene fills first.
///
/// A tree wide enough to reach that is one recording ~65 k chrome-bearing
/// nodes in a single frame, which no virtualized list does; a 66 k-cell
/// grid rendered whole does. The ceiling is deliberate: widening the
/// three to 32-bit indices doubles this struct, and it is a per-node SoA
/// column read by every pass. The cost of keeping it is that such a scene
/// panics with the table's name instead of rendering.
///
/// The node arena's own ceiling is checked differently, and the asymmetry
/// is deliberate too: [`SubtreeEnd`](crate::scene::tree::subtree_end)
/// packs a flag into bit 31 and debug-asserts the 2^31 bound, which at
/// 56 bytes per record is 120 GB of arena — unreachable, where this one is
/// merely large.
///
/// [`Background`]: crate::Background
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct ExtrasIdx {
    pub(crate) bounds: Option<Index16>,
    pub(crate) panel: Option<Index16>,
    pub(crate) chrome: Option<Index16>,
}

// SAFETY: `repr(C)` over three `Option<Index16>`, each two bytes of a
// `NonZeroU16` with `None` at zero, whose sizes sum to the struct's, so
// every byte is initialized and none is padding.
unsafe impl bytemuck::NoUninit for ExtrasIdx {}

const _: () = assert!(size_of::<ExtrasIdx>() == 3 * size_of::<u16>());
