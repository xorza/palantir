//! Shared fixtures for the text suite; each submodule owns one axis.
//!
//! Split by what a failure points at: [`key`] cache identity and metric
//! validation, [`wrap`] shaping and the wrap policies, [`truncate`] the
//! clip/ellipsis cut, [`geometry`] caret, hit-test and selection,
//! [`retention`] the shaped-buffer cache's windows, [`reuse`] the
//! per-window rows and the supersede signal they carry.

use crate::common::hash::hash_str;
use crate::layout::types::align::{Align, HAlign};
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::record_store::RecordStore;
use crate::text::cosmic::truncate::ClusterGlyph;
use crate::text::cosmic::{self, CosmicMeasure};
use crate::text::key::{TextShapeKey, WrapBound};
use crate::text::mono;
use crate::text::probe::internals as probe;
use crate::text::request::TextShapeRequest;
use crate::text::request::internals::TestShape;
use crate::text::root::internals::TestMeasure;
use crate::text::run::TextRun;
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
/// Override with the `TestShape` builders, so the one thing a case is
/// about reads on one line: `shape(16.0).width(32.0).halign(HAlign::Right)`.
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
    shape(font_size_px).leading(font_size_px * LINE_HEIGHT_MULT)
}

/// Height of one line at `shape`'s leading, as a measured extent reports
/// it — `Size` ceils, so a fractional leading rounds up.
fn one_line_h(shape: TestShape) -> f32 {
    shape.line_height_px.ceil()
}

fn slot(widget_id: WidgetId) -> TextRunSlot {
    slot_at(widget_id, 0)
}

fn slot_at(widget_id: WidgetId, ordinal: u16) -> TextRunSlot {
    TextRunSlot { widget_id, ordinal }
}

/// Measure through the mono fallback. Mints no shaped buffer, so every
/// run it measures carries the invalid sentinel.
fn mono_shape(text: &str, shape: TestShape, fit: LineFit) -> TestMeasure {
    let root = mono::measure(shape.request(text, fit), WrapFloor::Scan);
    TestMeasure::new(root, TextShapeKey::INVALID)
}

/// A truncating measure and the unbounded probe it cuts from. Truncation
/// reads the cached unbounded shape, so the probe has to be measured
/// first — returning both keeps a caller that needs the probe's key from
/// re-deriving the shape by hand.
struct Truncated {
    fitted: TestMeasure,
    unbounded: TestMeasure,
}

fn truncate(cosmic: &mut CosmicMeasure, text: &str, shape: TestShape, fit: LineFit) -> Truncated {
    let unbounded = cosmic.measure(text, shape.unbounded());
    let fitted = cosmic.measure_with_fit(text, shape, fit, unbounded.key);
    Truncated { fitted, unbounded }
}

/// [`truncate`] when only the truncated result is wanted.
fn measure_truncated(
    cosmic: &mut CosmicMeasure,
    text: &str,
    shape: TestShape,
    fit: LineFit,
) -> TestMeasure {
    truncate(cosmic, text, shape, fit).fitted
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
