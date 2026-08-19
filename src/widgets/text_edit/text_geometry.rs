//! The editor's text layout plus everything only the shape probe answers.

use crate::layout::types::align::{self, Align};
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::text::probe::Caret;
use crate::ui::Ui;
use crate::widgets::text_edit::text_layout::TextLayout;
use glam::Vec2;
use std::ops::Range;

/// What one probe of the content run yields. A named struct because the
/// closure has to hand all three back at once — the probe's borrow ends
/// with it, so nothing can be re-read afterwards.
#[derive(Clone, Copy, Debug)]
struct Probed {
    measured: Size,
    caret_pos: Caret,
    text_hash: u64,
}

#[derive(Debug)]
pub(super) struct GeometryInput<'a> {
    pub(super) layout: TextLayout,
    pub(super) text: &'a str,
    pub(super) placeholder: &'a str,
    pub(super) caret: usize,
    pub(super) selection: Option<Range<usize>>,
}

/// The layout plus everything only the shape probe could answer. Paint
/// reads this; nothing here exists before [`resolve_geometry`] runs,
/// which is why it is a separate type rather than zeroed fields on
/// [`TextLayout`].
#[derive(Clone, Copy, Debug)]
pub(super) struct TextGeometry {
    pub(super) layout: TextLayout,
    /// Where the shaped block sits inside the inner rect, as the *record*
    /// pass can work it out — from last pass's rect, since arrange has not
    /// run.
    ///
    /// **Read by the hit-test and by nothing else.** Painting stopped needing
    /// it when the block became a child the engine places: what a click has to
    /// undo is where the text was when the user aimed at it, which is last
    /// frame's, so a value one frame behind is the right one here and the wrong
    /// one there. Stored back into `ViewState` at the end of the pass, which is
    /// what makes it next frame's [`TextLayout::prev_block_offset`].
    pub(super) block_offset: Vec2,
    /// What the run measured, and what the placeholder measured. Both axes:
    /// the width alone drives horizontal scroll and the hug reservation, and
    /// the block node the shapes hang on wants the height too — see [`record`].
    pub(super) content_size: Size,
    pub(super) display_size: Size,
    pub(super) caret_pos: Caret,
    pub(super) text_hash: u64,
}

impl TextGeometry {
    /// Measure the run and fill `selection_rects` with the wash for
    /// `input.selection` — an out-parameter so the caller's retained buffer
    /// is refilled in place instead of a fresh one being handed back each
    /// frame.
    pub(super) fn resolve(
        ui: &mut Ui,
        input: GeometryInput<'_>,
        selection_rects: &mut Vec<Rect>,
    ) -> Self {
        let layout = input.layout;
        // The block is load-bearing: the content probe holds the shaper's
        // exclusive borrow, so the placeholder measurement below cannot be
        // taken until this one has dropped. Overlapping them is E0499, not a
        // runtime surprise.
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
        let widget_align = if layout.ctx.multiline {
            Align::v(layout.text_align.valign())
        } else {
            layout.text_align
        };
        let align_size = Size::new(
            if layout.ctx.multiline {
                layout.inner_size.w
            } else {
                (layout.inner_size.w - layout.caret_room).max(0.0)
            },
            layout.inner_size.h,
        );
        let containing = Rect {
            min: Vec2::ZERO,
            size: align_size,
        };
        let aligned = |size: Size| {
            align::align_in_rect(
                containing,
                Size::new(size.w, size.h.max(layout.ctx.font.line_height_px)),
                widget_align,
            )
            .min
        };
        TextGeometry {
            layout,
            block_offset: aligned(measured),
            content_size: measured,
            display_size: placeholder_measured,
            caret_pos,
            text_hash,
        }
    }
}
