//! A text backend on the shared test device, and the frames it is driven
//! through.

use crate::gpu::gpu_ctx::GpuCtx;
use crate::gpu::raster_program::RasterProgram;
use crate::gpu::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
use crate::gpu::text::TextBackend;
use crate::layout::types::align::Align;
use crate::primitives::color::rgba_f16::RgbaF16;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::scene::record_store::RecordStore;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::run::TextRun;
use crate::text::shaped_ref::ShapedTextRef;
use crate::text::shaper::TextShaper;
use crate::text::wrap::TextWrap;
use glam::{UVec2, Vec2};
use wgpu::util::StagingBelt;

/// The surface every row is bounded by.
pub(super) const PHYSICAL: UVec2 = UVec2::new(640, 480);

/// The 14 px font every row is shaped at.
const FONT_PX: f32 = 14.0;

/// One backend over a program of its own, the shaper and record store
/// its rows come from, and the lease that keeps the device alive. Which
/// program object a case built against is not something any of them
/// asserts on.
#[derive(Debug)]
pub(super) struct TextRig {
    lease: HeadlessTestGpuLease,
    pub(super) shaper: TextShaper,
    pub(super) store: RecordStore,
    pub(super) backend: TextBackend,
}

impl TextRig {
    pub(super) fn new() -> Self {
        let lease = headless_test_gpu();
        let shaper = TextShaper::new();
        let backend = TextBackend::new(
            &lease.device,
            &RasterProgram::new(&lease.device),
            shaper.clone(),
        );
        Self {
            lease,
            shaper,
            store: RecordStore::default(),
            backend,
        }
    }

    /// A row drawing `text` at 14 px on `line_height_px` lines, from
    /// `origin`, bounded by [`PHYSICAL`] at scale 1 in near-white.
    ///
    /// Shaped through the run first, so the key stamped into the row is
    /// the one the shaped buffer landed under: no width and a
    /// non-binding policy, the unbounded root and nothing else.
    pub(super) fn row(&mut self, text: &str, line_height_px: f32, origin: Vec2) -> TextDrawRow {
        let interned = self.store.intern(text);
        let recorded = self.store.record_text(interned);
        let run = TextRun {
            text,
            font: GlyphFont {
                size_px: FONT_PX,
                line_height_px,
                family: FontFamily::SANS,
                weight: FontWeight::REGULAR,
                slant: FontSlant::Normal,
            },
            wrap: TextWrap::SingleLine,
            align: Align::default(),
            max_width_px: None,
        };
        let key = run
            .unbounded_key()
            .expect("the fixture face names a usable size");
        self.shaper.layout(&run);
        TextDrawRow {
            text: ShapedTextRef::new(key, &recorded),
            origin,
            bounds: URect::new(0, 0, PHYSICAL.x, PHYSICAL.y),
            color: RgbaF16::new(0.94, 0.94, 0.94, 1.0),
            scale: 1.0,
        }
    }

    /// Prepare `batches` at `scale`, batch `i` under index `i`, in one
    /// deferred upload; submit it and wait for the device.
    pub(super) fn frame(&mut self, scale: f32, batches: &[&[TextDrawRow]]) {
        let device = &self.lease.device;
        let mut belt = StagingBelt::new(device.clone(), 1 << 16);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut ctx = GpuCtx::new(device, &self.lease.queue, &mut belt, &mut encoder);
            let interned_text = self.store.interned_text();
            for (index, rows) in batches.iter().enumerate() {
                self.backend
                    .prepare_batch(&mut ctx, scale, index, rows, &interned_text);
            }
            self.backend.pass.flush(&mut ctx);
        }
        belt.finish_and_recall_on_submit(&encoder);
        self.lease.queue.submit([encoder.finish()]);
        self.lease.wait();
    }
}
