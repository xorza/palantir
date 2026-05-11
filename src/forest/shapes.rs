use crate::shape::{ShapePayloads, ShapeRecord};

/// Per-frame shape store for one [`crate::forest::tree::Tree`].
///
/// - `records` is the flat shape buffer; each node owns a contiguous
///   sub-range via `NodeRecord.shape_span`. The gaps between a node's
///   children's spans hold that node's direct shapes in record order,
///   which is what [`crate::forest::tree::TreeItems`] interleaves.
/// - `payloads` holds variable-length side-tables that record variants
///   (`Mesh` / `Polyline`) reference via inner `Span`s.
///
/// Cleared together per frame, capacity retained — same lifecycle as
/// the rest of the tree.
#[derive(Default)]
pub(crate) struct Shapes {
    pub(crate) records: Vec<ShapeRecord>,
    pub(crate) payloads: ShapePayloads,
}

impl Shapes {
    pub(crate) fn clear(&mut self) {
        self.records.clear();
        self.payloads.clear();
    }
}
