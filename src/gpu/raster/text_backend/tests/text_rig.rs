//! A text backend on the shared test device, and the frames driving it.

use crate::gpu::pipeline::raster_pipeline::RasterPipeline;
use crate::gpu::raster::text_backend::{TextBackend, TextBatchInput};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::test_gpu::{TestGpu, headless_test_gpu};
use crate::primitives::geometry::urect::URect;
use crate::primitives::layout::align::Align;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use crate::shape::record_store::RecordStore;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::run::TextRun;
use crate::text::shaped_ref::ShapedTextRef;
use crate::text::shaper::TextShaper;
use crate::text::wrap::TextWrap;
use glam::{UVec2, Vec2};

pub(super) const PHYSICAL: UVec2 = UVec2::new(640, 480);

const FONT_PX: f32 = 14.0;

/// One backend over its own program, plus the shaper, record store and device its rows need.
#[derive(Debug)]
pub(super) struct TextRig {
    test_gpu: TestGpu,
    pub(super) shaper: TextShaper,
    pub(super) store: RecordStore,
    pub(super) backend: TextBackend,
}

impl TextRig {
    pub(super) fn new() -> Self {
        let test_gpu = headless_test_gpu();
        let shaper = TextShaper::new();
        let backend = TextBackend::new(
            &test_gpu,
            &RasterPipeline::new(&test_gpu, &TextureBinding::new(&test_gpu)),
            shaper.clone(),
        );
        Self {
            test_gpu,
            shaper,
            store: RecordStore::default(),
            backend,
        }
    }

    /// A row drawing `text` at 14 px from `origin`, bounded by [`PHYSICAL`] at scale 1 in near-white. Shaped through the run first so the stamped key matches the shaped buffer's.
    pub(super) fn row(&mut self, text: &str, line_height: f32, origin: Vec2) -> TextDrawRow {
        let interned = self.store.text.intern(text);
        let recorded = self.store.text.record(interned);
        let run = TextRun {
            text,
            font: GlyphFont {
                font_size: FONT_PX,
                line_height,
                family: FontFamily::SANS,
                weight: FontWeight::REGULAR,
                slant: FontSlant::Normal,
            },
            wrap: TextWrap::SingleLine,
            align: Align::default(),
            max_width: None,
        };
        let key = run
            .unbounded_key(&self.shaper.family_keys())
            .expect("the fixture face names a usable size");
        self.shaper.layout(&run);
        TextDrawRow {
            text: ShapedTextRef::new(key, &recorded),
            origin,
            bounds: URect::new(0, 0, PHYSICAL.x, PHYSICAL.y),
            color: RgbaF16::new(0.94, 0.94, 0.94, 1.0),
            scale: 1.0,
            spin: None,
            clip: ClipChain::NONE,
        }
    }

    /// Prepare `batches` at `scale` in one deferred upload; submit and wait.
    pub(super) fn frame(&mut self, scale: f32, batches: &[&[TextDrawRow]]) {
        let interned_text = self.store.text.interned();
        let backend = &mut self.backend;
        self.test_gpu.frame(|ctx| {
            for (idx, runs) in batches.iter().enumerate() {
                backend.prepare_batch(TextBatchInput {
                    idx,
                    runs,
                    scale,
                    interned_text,
                });
            }
            backend.pass.flush(ctx);
        });
    }
}
