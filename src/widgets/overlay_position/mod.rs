use crate::forest::layer::Layer;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::ui::Ui;
use glam::Vec2;

#[derive(Clone, Copy, Debug)]
struct AxisPosition {
    preferred: f32,
    fallback_trailing_edge: Option<f32>,
}

impl AxisPosition {
    fn fixed(preferred: f32) -> Self {
        Self {
            preferred,
            fallback_trailing_edge: None,
        }
    }

    fn reversible(preferred: f32, fallback_trailing_edge: f32) -> Self {
        Self {
            preferred,
            fallback_trailing_edge: Some(fallback_trailing_edge),
        }
    }

    fn resolve(self, extent: f32, bounds_min: f32, bounds_max: f32) -> f32 {
        let fallback = self.fallback_trailing_edge.map(|edge| edge - extent);
        let position = match fallback {
            Some(fallback) if self.preferred + extent > bounds_max && fallback >= bounds_min => {
                fallback
            }
            _ => self.preferred,
        };
        position.clamp(bounds_min, (bounds_max - extent).max(bounds_min))
    }
}

/// Edge-aware positioning policy for a measured side-layer body.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OverlayPosition {
    horizontal: AxisPosition,
    vertical: AxisPosition,
}

impl OverlayPosition {
    /// Prefer `anchor` as the top-left and flip either overflowing axis
    /// so the body's trailing edge meets that axis of the anchor.
    pub(crate) fn around(anchor: Vec2) -> Self {
        Self {
            horizontal: AxisPosition::reversible(anchor.x, anchor.x),
            vertical: AxisPosition::reversible(anchor.y, anchor.y),
        }
    }

    /// Prefer the trigger's bottom-left, flip above when necessary, and
    /// clamp horizontally without changing the trigger-side alignment.
    pub(crate) fn below(trigger: Rect, gap: f32) -> Self {
        Self {
            horizontal: AxisPosition::fixed(trigger.min.x),
            vertical: AxisPosition::reversible(trigger.max().y + gap, trigger.min.y - gap),
        }
    }

    pub(crate) fn resolve(
        self,
        measured_size: Option<Size>,
        bounds: Rect,
    ) -> ResolvedOverlayPosition {
        let needs_measure = measured_size.is_none();
        let anchor = measured_size.map_or_else(
            || Vec2::new(self.horizontal.preferred, self.vertical.preferred),
            |size| {
                let bounds_max = bounds.max();
                Vec2::new(
                    self.horizontal.resolve(size.w, bounds.min.x, bounds_max.x),
                    self.vertical.resolve(size.h, bounds.min.y, bounds_max.y),
                )
            },
        );
        ResolvedOverlayPosition {
            anchor,
            measure_cap: bounds.size,
            needs_measure,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedOverlayPosition {
    anchor: Vec2,
    measure_cap: Size,
    needs_measure: bool,
}

impl ResolvedOverlayPosition {
    /// Record against the full positioning bounds, then settle an
    /// unmeasured body once its natural size is available.
    pub(crate) fn show(self, ui: &mut Ui, layer: Layer, body: impl FnOnce(&mut Ui)) {
        ui.layer(layer, self.anchor, Some(self.measure_cap), body);
        if self.needs_measure {
            ui.request_relayout();
        }
    }
}

#[cfg(test)]
mod tests;
