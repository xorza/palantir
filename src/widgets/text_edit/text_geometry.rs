//! The editor's text layout plus everything only the shape probe answers.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::text::probe::Caret;
use crate::ui::Ui;
use crate::widgets::text_edit::text_layout::TextLayout;
use glam::Vec2;
use std::num::NonZeroU64;
use std::ops::Range;

#[derive(Clone, Copy, Debug)]
struct Probed {
    measured: Size,
    caret_pos: Caret,
    text_hash: Option<NonZeroU64>,
}

#[derive(Debug)]
pub(super) struct GeometryInput<'a> {
    pub(super) layout: TextLayout,
    pub(super) text: &'a str,
    pub(super) placeholder: &'a str,
    pub(super) caret: usize,
    pub(super) selection: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TextGeometry {
    pub(super) layout: TextLayout,
    /// Block position inside the inner rect, from last pass's rect (arrange has not run). Only the
    /// hit-test reads it; stored back into `ViewState` as next frame's [`TextLayout::prev_block_offset`].
    pub(super) block_offset: Vec2,
    /// What the run measured, placeholder or not.
    pub(super) content_size: Size,
    /// What is on show: [`Self::content_size`], or the placeholder's size for an empty run with one.
    pub(super) display_size: Size,
    pub(super) caret_pos: Caret,
    /// Hash of the probed bytes, `None` where the face named no size (see `EditState::observe_text_hash`).
    pub(super) text_hash: Option<NonZeroU64>,
}

impl TextGeometry {
    pub(super) fn resolve(
        ui: &mut Ui,
        input: GeometryInput<'_>,
        selection_rects: &mut Vec<Rect>,
    ) -> Self {
        let layout = input.layout;
        // Scoped: the content probe's exclusive shaper borrow must end before the placeholder probe.
        let Probed {
            measured,
            caret_pos,
            text_hash,
        } = {
            let probe = ui.probe_text(layout.ctx.run(input.text));
            selection_rects.clear();
            if let Some(selection) = input.selection {
                probe.selection_rects(selection, |rect| selection_rects.push(rect));
            }
            Probed {
                measured: probe.size(),
                caret_pos: probe.caret_at(input.caret),
                text_hash: probe.text_hash(),
            }
        };
        let placeholder_measured = if input.text.is_empty() && !input.placeholder.is_empty() {
            ui.probe_text(layout.ctx.run(input.placeholder)).size()
        } else {
            measured
        };
        // Same align and block box as the block node, so next frame's hit-test offset matches the placement.
        let block = layout.block_align().place_in(
            Rect {
                min: Vec2::ZERO,
                size: layout.inner_size(),
            },
            layout.block_size(placeholder_measured),
        );
        TextGeometry {
            layout,
            block_offset: block.min,
            content_size: measured,
            display_size: placeholder_measured,
            caret_pos,
            text_hash,
        }
    }
}
