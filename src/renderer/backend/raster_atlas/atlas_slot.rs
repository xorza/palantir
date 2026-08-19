use crate::renderer::backend::raster_atlas::content_type::ContentType;
use etagere::AllocId;

/// One entry of [`RasterAtlas`](super::RasterAtlas)'s dense slab: where a
/// raster sits on its side, what bearing it draws with, and the two stamps
/// that decide when it may be taken away.
///
/// Kept narrow and `Copy` because it is the hot read: the encoded-run
/// cache's hit path loads one per glyph and copies it whole, which is why
/// the key that maps to it lives in a parallel column instead of a field
/// here.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasSlot {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) left: i16,
    pub(crate) top: i16,
    pub(crate) content: ContentType,
    /// The packer rectangle this raster owns, or `None` for a
    /// non-drawing entry — a whitespace glyph, or one the rasterizer
    /// produced no pixels for. The clock can only reclaim the first kind;
    /// the second expires on a deadline instead.
    pub(crate) alloc: Option<AllocId>,
    /// Advanced whenever the slab index is handed to another raster, so
    /// an encoded run still holding the index reads it as stale rather
    /// than drawing whatever took its place.
    pub(crate) generation: u32,
    /// Frame this entry was last drawn or looked up on, in its atlas's
    /// own clock. The clock hand skips anything stamped with the current
    /// frame, and a non-drawing entry's deadline is measured from here.
    pub(crate) last_use: u64,
}
