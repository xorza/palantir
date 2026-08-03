//! Shared fixtures for the text suite; each submodule owns one axis.
//!
//! Split by what a failure points at: [`key`] cache identity and metric
//! validation, [`wrap`] shaping and the wrap policies, [`truncate`] the
//! clip/ellipsis cut, [`geometry`] caret, hit-test and selection,
//! [`retention`] the shaped-buffer cache's windows, [`reuse`] the
//! per-window rows and the supersede signal they carry.

use crate::common::hash::hash_str;
use crate::layout::types::align::HAlign;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::record_store::RecordStore;
use crate::text::cosmic::{self, ClusterGlyph, CosmicMeasure};
use crate::text::key::TextShapeKey;
use crate::text::layout_probe;
use crate::text::mono;
use crate::text::request::TextShapeRequest;
use crate::text::request::internals::TestShape;
use crate::text::root::internals::TestMeasure;
use crate::text::shaped_ref::ShapedTextRef;
use crate::text::shaper::TextShaper;
use crate::text::system::{TextRunSlot, TextSystem};
use crate::text::wrap::{LineFit, TextWrap, WrapFloor};
use crate::text::{FontFamily, FontWeight};
use crate::widgets::theme::text_style::LINE_HEIGHT_MULT;
use rustc_hash::FxHashSet;

mod geometry;
mod key;
mod retention;
mod reuse;
mod truncate;
mod wrap;

/// Measurement parameters with the defaults nearly every case wants:
/// bundled Inter Regular, unbounded, `HAlign::Auto`, and leading equal to
/// the font size — which keeps the mono fallback's line height numerically
/// equal to `font_size`, the placeholder layout the mono cases pin.
///
/// Override with struct-update syntax: `TestShape { max_width_px:
/// Some(32.0), ..shape(16.0) }`.
fn shape(font_size_px: f32) -> TestShape {
    TestShape {
        font_size_px,
        line_height_px: font_size_px,
        max_width_px: None,
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
        halign: HAlign::Auto,
    }
}

/// [`shape`] at production leading ([`LINE_HEIGHT_MULT`]) — what the real
/// UI shapes at, and what the cosmic geometry cases pin.
fn ui_shape(font_size_px: f32) -> TestShape {
    TestShape {
        line_height_px: font_size_px * LINE_HEIGHT_MULT,
        ..shape(font_size_px)
    }
}

fn slot(widget_id: WidgetId) -> TextRunSlot {
    slot_at(widget_id, 0)
}

fn slot_at(widget_id: WidgetId, ordinal: u16) -> TextRunSlot {
    TextRunSlot { widget_id, ordinal }
}

fn mono_shape(
    text: &str,
    font_size_px: f32,
    line_height_px: f32,
    max_width_px: Option<f32>,
    fit: LineFit,
) -> TestMeasure {
    let request = TextShapeRequest::unbounded(
        text,
        font_size_px,
        line_height_px,
        FontFamily::Sans,
        FontWeight::Regular,
    );
    let request = match max_width_px {
        Some(width) => request.bounded(width, HAlign::Auto, fit),
        None => request,
    };
    // Mono mints no shaped buffer, so every run it measures is invalid.
    let root = mono::internals::measure(request, WrapFloor::Scan);
    TestMeasure {
        size: root.size,
        key: TextShapeKey::INVALID,
        intrinsic_min: root.intrinsic_min,
        single_line: root.single_line,
    }
}

fn measure_truncated(
    cosmic: &mut CosmicMeasure,
    text: &str,
    params: TestShape,
    fit: LineFit,
) -> TestMeasure {
    let unbounded = cosmic.measure(
        text,
        TestShape {
            max_width_px: None,
            halign: HAlign::Auto,
            ..params
        },
    );
    cosmic.measure_with_fit(text, params, fit, unbounded.key)
}

#[derive(Clone, Debug, PartialEq)]
struct GlyphPosition {
    x: f32,
    width: f32,
    line_top: f32,
    line_height: f32,
    start: usize,
    end: usize,
}

/// Glyph geometry in the same block-local space the renderer and probe
/// see — `left` off the buffer's own x, exactly as `extract_glyphs` folds
/// it into the run origin.
fn glyph_positions(cosmic: &CosmicMeasure, key: TextShapeKey) -> Vec<GlyphPosition> {
    let shaped = cosmic.shaped_run(key).expect("shaped buffer must exist");
    let left = shaped.left;
    shaped
        .buffer
        .layout_runs()
        .flat_map(move |run| {
            run.glyphs.iter().map(move |glyph| GlyphPosition {
                x: glyph.x - left,
                width: glyph.w,
                line_top: run.line_top,
                line_height: run.line_height,
                start: glyph.start,
                end: glyph.end,
            })
        })
        .collect()
}
