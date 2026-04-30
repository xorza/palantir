use crate::primitives::{Color, Corners, Rect, Stroke};
use crate::shape::{Shape, ShapeRect};
use crate::tree::{NodeId, Tree};
use glam::Vec2;

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
    DrawRect {
        rect: Rect,
        radius: Corners,
        fill: Color,
        stroke: Option<Stroke>,
    },
    // Future: DrawText { … }, DrawLine { … }, DrawPath { … }.
}

/// Walk the tree pre-order and emit logical-px paint commands. No GPU work,
/// no scale/snap math — that lives in the backend's process step. Pure
/// function over `&Tree`, so the same call works in unit tests with no
/// device.
pub fn encode(tree: &Tree, out: &mut Vec<RenderCmd>) {
    out.clear();
    if let Some(root) = tree.root() {
        encode_node(tree, root, out);
    }
}

fn encode_node(tree: &Tree, id: NodeId, out: &mut Vec<RenderCmd>) {
    let node = tree.node(id);
    if node.element.clip {
        out.push(RenderCmd::PushClip(node.rect));
    }

    let owner = node.rect;
    for shape in tree.shapes_of(id) {
        match shape {
            Shape::RoundedRect {
                bounds,
                radius,
                fill,
                stroke,
            } => {
                let rect = match bounds {
                    ShapeRect::Full => owner,
                    ShapeRect::Offset(r) => Rect {
                        min: owner.min + Vec2::new(r.min.x, r.min.y),
                        size: r.size,
                    },
                };
                out.push(RenderCmd::DrawRect {
                    rect,
                    radius: *radius,
                    fill: *fill,
                    stroke: *stroke,
                });
            }
            // No backend support for these yet — drop with a trace so they're
            // not silently invisible.
            Shape::Line { .. } | Shape::Text { .. } => {
                tracing::trace!(?shape, "encoder: dropping unsupported shape");
            }
        }
    }

    let mut c = node.first_child;
    while let Some(child) = c {
        encode_node(tree, child, out);
        c = tree.node(child).next_sibling;
    }

    if node.element.clip {
        out.push(RenderCmd::PopClip);
    }
}

#[cfg(test)]
mod tests;
