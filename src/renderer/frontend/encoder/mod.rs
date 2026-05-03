use super::cmd_buffer::RenderCmdBuffer;
use crate::cascade::CascadeResult;
use crate::layout::LayoutResult;
use crate::primitives::{Align, HAlign, Rect, Size, TranslateScale, VAlign};
use crate::shape::Shape;
use crate::tree::{NodeId, Tree};

/// Walk the tree pre-order and emit logical-px paint commands. No GPU work,
/// no scale/snap math — that lives in the backend's process step. Pure
/// function over `(&Tree, &LayoutResult, &Cascades)`, so the same call works
/// in unit tests with no device. Reads invisibility cascade from `Cascades`
/// so encoder and hit-index can't drift.
///
/// `damage_filter` enables Step 5 of the damage-rendering plan: when
/// `Some(rect)`, leaf paint commands (`DrawRect`/`DrawText`) are skipped
/// for nodes whose arranged rect doesn't intersect the filter. Clip and
/// transform push/pop pairs are *always* emitted so descendant scissor
/// state and group boundaries (composer text↔quad split) stay correct.
/// `None` paints everything — used for the first frame, full-repaint
/// fallback, and existing tests.
pub fn encode(
    tree: &Tree,
    layout: &LayoutResult,
    cascades: &CascadeResult,
    damage_filter: Option<Rect>,
    out: &mut RenderCmdBuffer,
) {
    out.clear();
    if let Some(root) = tree.root() {
        encode_node(tree, layout, cascades, damage_filter, root, out);
    }
}

fn encode_node(
    tree: &Tree,
    layout: &LayoutResult,
    cascades: &CascadeResult,
    damage_filter: Option<Rect>,
    id: NodeId,
    out: &mut RenderCmdBuffer,
) {
    // Hidden / Collapsed: paint nothing for this node or its subtree.
    // The cascade table already composed self + ancestors; recursing skips
    // the whole subtree because we early-return at the top of every node.
    if cascades.is_invisible(id) {
        return;
    }

    let rect = layout.rect(id);

    // Order: clip is in parent-of-panel space (pre-transform); transform
    // applies inside the clip and only to children. The panel's own
    // background paints under the clip but BEFORE the transform — matching
    // WPF's `RenderTransform` convention.
    let clip = tree.paint(id).attrs.is_clip();
    if clip {
        out.push_clip(rect);
    }

    // Damage filter: skip leaf shape emission when this node's
    // *screen* rect (layout rect projected through ancestor
    // transforms via `cascades`) doesn't intersect the dirty region.
    // Damage rects in `damage_filter` are also screen-space, so the
    // comparison is consistent under arbitrary transform stacks.
    // Push/PopClip and Push/PopTransform are still emitted (above and
    // below) so scissor groups and child transforms stay coherent.
    // `None` filter ⇒ paint everything.
    let paints =
        damage_filter.is_none_or(|d| cascades.rows()[id.index()].screen_rect.intersects(d));

    if paints {
        for shape in tree.shapes_of(id) {
            match shape {
                Shape::RoundedRect {
                    radius,
                    fill,
                    stroke,
                } => {
                    out.draw_rect(rect, *radius, *fill, *stroke);
                }
                Shape::Text { color, align, .. } => {
                    // Shaping happened in measure; the resulting buffer key is
                    // on `LayoutResult.text_shapes`. Missing entry means no
                    // shaper was installed (mono fallback) or the run was empty
                    // — drop in either case.
                    let Some(shaped) = layout.text_shape(id) else {
                        continue;
                    };
                    if shaped.key.is_invalid() {
                        tracing::trace!(?shape, "encoder: dropping text with invalid key");
                        continue;
                    }
                    // `layout.rect(id)` is padding-inclusive (the rendered
                    // rect that DrawRect paints). Text aligns within the
                    // padding-deflated content area so e.g. `Button.padding(8)`
                    // insets the label visually.
                    let inner = rect.deflated_by(tree.layout(id).padding);
                    out.draw_text(
                        align_text_in(inner, shaped.measured, *align),
                        *color,
                        shaped.key,
                    );
                }
                // No backend support for these yet — drop with a trace so they're
                // not silently invisible.
                Shape::Line { .. } => {
                    tracing::trace!(?shape, "encoder: dropping unsupported shape");
                }
            }
        }
    }

    // Skip Push/PopTransform when the transform is identity — composing
    // identity is a no-op, so emitting the pair just wastes two cmd
    // slots and a `transform_stack` push/pop in the composer.
    let transform = tree
        .read_extras(id)
        .transform
        .filter(|t| *t != TranslateScale::IDENTITY);
    if let Some(t) = transform {
        out.push_transform(t);
    }

    for child in tree.children(id) {
        encode_node(tree, layout, cascades, damage_filter, child, out);
    }

    if transform.is_some() {
        out.pop_transform();
    }
    if clip {
        out.pop_clip();
    }
}

/// Position a text run's bounding box inside a leaf's arranged rect per
/// `align`. Returns a rect with `min` shifted by the alignment offset
/// and `size` shrunk to the measured text bbox — composer takes
/// `min` as the glyph origin and `size` as the clip bounds. Glyphs
/// don't stretch, so `Auto`/`Stretch` collapse to start (top-left)
/// — matches `place_axis`'s behavior for non-stretchable content.
fn align_text_in(leaf: Rect, measured: Size, align: Align) -> Rect {
    let dx = match align.halign() {
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
        HAlign::Center => (leaf.size.w - measured.w) * 0.5,
        HAlign::Right => leaf.size.w - measured.w,
    };
    let dy = match align.valign() {
        VAlign::Auto | VAlign::Top | VAlign::Stretch => 0.0,
        VAlign::Center => (leaf.size.h - measured.h) * 0.5,
        VAlign::Bottom => leaf.size.h - measured.h,
    };
    Rect::new(
        leaf.min.x + dx.max(0.0),
        leaf.min.y + dy.max(0.0),
        measured.w,
        measured.h,
    )
}

pub(crate) mod cache;

#[cfg(test)]
mod tests;
