//! Shared fixtures for the text suite; each submodule owns one axis: [`key`], [`wrap`], [`truncate`], [`geometry`],
//! [`retention`], [`reuse`].

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdSet};
use crate::primitives::layout::align::{Align, HAlign};
use crate::scene::record_store::RecordStore;
use crate::text::cosmic::CosmicMeasure;
use crate::text::cosmic::cluster_glyph::ClusterGlyph;
use crate::text::cosmic::shaped_buffer_cache;
use crate::text::font_family::FontFamily;
use crate::text::font_scope::FontScope;
use crate::text::font_scope::internals::{ARABIC, HEBREW};
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::{LineAlign, TextShapeKey, WrapBound};
use crate::text::mono;
use crate::text::probe::internals as probe;
use crate::text::request::TextShapeRequest;
use crate::text::request::internals::TestShape;
use crate::text::root::TextRoot;
use crate::text::root::internals::TestMeasure;
use crate::text::run::TextRun;
use crate::text::shaped_ref::ShapedTextRef;
use crate::text::shaper::TextShaper;
use crate::text::system::{TextRunSlot, TextSystem};
use crate::text::wrap::{LineFit, TextWrap, WrapFloor};
use crate::text::{RENDERED_RUN_KEEP_FRAMES, RENDERED_RUN_KEEP_SPREAD_MASK};
use crate::widgets::theme::text_style::LINE_HEIGHT_MULT;

mod fonts;
mod geometry;
mod key;
mod retention;
mod reuse;
mod truncate;
mod wrap;

/// Measurement parameters with the defaults most cases want: bundled Inter Regular, unbounded, `HAlign::Auto`,
/// leading equal to the font size. Override via the `TestShape` builders: `shape(16.0).width(32.0)`.
fn shape(font_size: f32) -> TestShape {
    TestShape::new(GlyphFont {
        size: font_size,
        line_height: font_size,
        family: FontFamily::SANS,
        weight: FontWeight::REGULAR,
        slant: FontSlant::Normal,
    })
}

/// [`shape`] at production leading ([`LINE_HEIGHT_MULT`]), what the cosmic geometry cases pin.
fn ui_shape(font_size: f32) -> TestShape {
    shape(font_size).leading(font_size * LINE_HEIGHT_MULT)
}

/// Height of one line at `shape`'s leading (`Size` ceils fractional leading).
fn one_line_h(shape: TestShape) -> f32 {
    shape.font.line_height.ceil()
}

fn slot(widget_id: WidgetId) -> TextRunSlot {
    slot_at(widget_id, 0)
}

fn slot_at(widget_id: WidgetId, ordinal: u16) -> TextRunSlot {
    TextRunSlot { widget_id, ordinal }
}

/// The mono metric's extent for `text`: unbounded shape where `shape` commits no width, else its bounded resolve
/// under `fit`. The wrap floor and line count come from [`mono_root`].
fn mono_extent(text: &str, shape: TestShape, fit: LineFit) -> Size {
    let request = shape.request(text, fit);
    match request.max_width() {
        None => mono::root(request, WrapFloor::Skip).extent.size,
        Some(_) => mono::resolve(request).size,
    }
}

fn mono_root(text: &str, shape: TestShape) -> TextRoot {
    mono::root(shape.unbounded_request(text), WrapFloor::Scan)
}

/// A truncating measure and the unbounded probe it cuts from (which must be measured first).
#[derive(Debug)]
struct Truncated {
    fitted: TestMeasure,
    unbounded: TestMeasure,
}

fn truncate(cosmic: &mut CosmicMeasure, text: &str, shape: TestShape, fit: LineFit) -> Truncated {
    let unbounded = cosmic.measure(text, shape.unbounded());
    let fitted = cosmic.measure_with_fit(text, shape, fit, unbounded.buffer_key());
    Truncated { fitted, unbounded }
}

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

/// Glyph geometry in the block-local space the renderer and probe see.
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
