use crate::cascade::Cascades;
use crate::layout::LayoutResult;
use crate::primitives::{Color, Corners, Rect, Stroke, TranslateScale};
use crate::shape::Shape;
use crate::text::TextCacheKey;
use crate::tree::{NodeId, Tree};

/// One typed paint instruction in logical (DIP) coordinates. Produced by
/// `encode` from the tree, consumed by the backend which scales/snaps to
/// physical pixels and groups by scissor.
///
/// Decoupling encode from backend means: (a) the encoder is pure data and
/// tree-shaped knowledge; (b) any backend (wgpu, future software/Vello,
/// test harness) consumes the same stream; (c) future shape kinds (Text,
/// Line, Path) just add variants without touching pipeline code.
#[derive(Clone, Debug)]
pub enum RenderCmd {
    /// Push a logical-px clip rect; the backend intersects it with the parent
    /// at process time. Pairs with `PopClip`.
    PushClip(Rect),
    PopClip,
    /// Push a transform applied to subsequent draws and clips, composed onto
    /// any ancestor transform. Pairs with `PopTransform`.
    PushTransform(TranslateScale),
    PopTransform,
    DrawRect {
        rect: Rect,
        radius: Corners,
        fill: Color,
        stroke: Option<Stroke>,
    },
    /// Place a shaped text run at `rect` (logical px). The shaped buffer is
    /// resolved at submit time via [`crate::text::TextCacheKey`] against the
    /// `TextMeasure` that did the shaping. Runs whose key is invalid are
    /// dropped by the backend.
    DrawText {
        rect: Rect,
        color: Color,
        key: TextCacheKey,
    },
    // Future: DrawLine { … }, DrawPath { … }.
}

/// Walk the tree pre-order and emit logical-px paint commands. No GPU work,
/// no scale/snap math — that lives in the backend's process step. Pure
/// function over `(&Tree, &LayoutResult, &Cascades)`, so the same call works
/// in unit tests with no device. Reads invisibility cascade from `Cascades`
/// so encoder and hit-index can't drift.
pub fn encode(
    tree: &Tree,
    layout: &LayoutResult,
    cascades: &Cascades,
    disabled_dim: f32,
    out: &mut Vec<RenderCmd>,
) {
    out.clear();
    if let Some(root) = tree.root() {
        encode_node(tree, layout, cascades, disabled_dim, root, out);
    }
}

fn encode_node(
    tree: &Tree,
    layout: &LayoutResult,
    cascades: &Cascades,
    disabled_dim: f32,
    id: NodeId,
    out: &mut Vec<RenderCmd>,
) {
    // Hidden / Collapsed: paint nothing for this node or its subtree.
    // The cascade table already composed self + ancestors; recursing skips
    // the whole subtree because we early-return at the top of every node.
    if cascades.is_invisible(id) {
        return;
    }

    let rect = layout.rect(id);
    let rgb_mul = if cascades.is_disabled(id) {
        disabled_dim
    } else {
        1.0
    };

    // Order: clip is in parent-of-panel space (pre-transform); transform
    // applies inside the clip and only to children. The panel's own
    // background paints under the clip but BEFORE the transform — matching
    // WPF's `RenderTransform` convention.
    let clip = tree.paint(id).attrs.is_clip();
    if clip {
        out.push(RenderCmd::PushClip(rect));
    }

    for shape in tree.shapes_of(id) {
        match shape {
            Shape::RoundedRect {
                radius,
                fill,
                stroke,
            } => {
                out.push(RenderCmd::DrawRect {
                    rect,
                    radius: *radius,
                    fill: fill.dim_rgb(rgb_mul),
                    stroke: stroke.map(|s| Stroke {
                        width: s.width,
                        color: s.color.dim_rgb(rgb_mul),
                    }),
                });
            }
            Shape::Text { color, .. } => {
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
                out.push(RenderCmd::DrawText {
                    rect,
                    color: color.dim_rgb(rgb_mul),
                    key: shaped.key,
                });
            }
            // No backend support for these yet — drop with a trace so they're
            // not silently invisible.
            Shape::Line { .. } => {
                tracing::trace!(?shape, "encoder: dropping unsupported shape");
            }
        }
    }

    let transform = tree.read_extras(id).transform;
    let has_transform = transform.is_some();
    if let Some(t) = transform {
        out.push(RenderCmd::PushTransform(t));
    }

    let mut c = tree.child_cursor(id);
    while let Some(child) = c.next(tree) {
        encode_node(tree, layout, cascades, disabled_dim, child, out);
    }

    if has_transform {
        out.push(RenderCmd::PopTransform);
    }
    if clip {
        out.push(RenderCmd::PopClip);
    }
}

#[cfg(test)]
mod tests;
