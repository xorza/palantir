use crate::common::span::Span;
use crate::gpu::pipeline::quad_pipeline::{ExtraSpans, QuadExtras, QuadPipeline};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::render_buffer::quad_draw_row::QuadInstance;

/// The extras follow the frame's 3 quads in order: the clear at 3, the outline at 4; no dim
/// takes no instance. A frame with none lays nothing and its clear reads
/// `None`, so a pre-clear it forgot cannot draw the last frame's.
#[test]
fn extras_follow_the_quads_in_order() {
    let quad = |x: f32| QuadInstance::of_rect(Rect::new(x, 0.0, 1.0, 1.0));
    let mut tail = Vec::new();
    let spans = ExtraSpans::lay(
        3,
        QuadExtras {
            clear: Some(quad(3.0)),
            dim: None,
            outlines: &[quad(4.0)],
        },
        &mut tail,
    );
    assert_eq!(
        spans,
        ExtraSpans {
            clear: Some(3),
            dim: None,
            outlines: Span::new(4, 1),
        }
    );
    assert_eq!(
        tail.iter().map(|q| q.header.rect.min.x).collect::<Vec<_>>(),
        [3.0, 4.0]
    );
    let spans = ExtraSpans::lay(3, QuadExtras::default(), &mut tail);
    assert!(tail.is_empty());
    assert_eq!(spans.clear, None);
    assert_eq!(spans.outlines.len, 0);
}

/// A shadow builds a pipeline for each set of open mask slots, the set it is asked for alone,
/// and every set compiles: after the sets up to one, those and no later ones are built. So do
/// the wide entry and the general one, which reads the tables its shadow's other corners hold:
/// built over slot 0, neither is built over slot 1 until asked. A fill reads no page, so its
/// one pipeline serves every set.
#[test]
fn a_table_shadow_builds_one_pipeline_a_set_of_mask_slots() {
    use crate::gpu::atlas::atlas_textures::AtlasTextures;
    use crate::gpu::atlas::page_slots::PageSlots;
    use crate::gpu::pipeline::atlas_binding::{AtlasGroup, AtlasSlots};
    use crate::gpu::pipeline::clip_variant::Clipping;
    use crate::gpu::pipeline::frame_variant::FrameVariant;
    use crate::gpu::pipeline::quad_pipeline::QuadVariants;
    use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
    use crate::gpu::pipeline::quad_pipeline::quad_draw::ShadowEntry;
    use crate::gpu::resource::texture_binding::TextureBinding;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::texel::TargetFormat;
    use crate::renderer::render_buffer::fill_form::FillForm;

    let gpu = headless_test_gpu();
    let atlas = AtlasGroup::new(&gpu, &AtlasTextures::new(&gpu));
    let layout = atlas.pipeline_layout(&gpu, &TextureBinding::new(&gpu));
    let program = QuadPipeline::new(&gpu, &layout);
    let mut variants = QuadVariants::NONE;
    let tables = QuadDraw::Shadow(ShadowEntry::Tables);
    let fill = QuadDraw::Fill(FillForm::BOX_SOLID);
    let target = |mask| {
        FrameVariant {
            clipping: Clipping::Off,
            slots: AtlasSlots {
                mask,
                color: PageSlots::FIRST,
            },
        }
        .target(TargetFormat::RGBA8_SRGB.get())
    };
    for slots in PageSlots::all() {
        program.ensure_draw(&mut variants, &gpu, target(slots), tables);
        for other in PageSlots::all() {
            assert_eq!(
                variants.is_built(tables, Clipping::Off, other),
                other.idx() <= slots.idx(),
                "built through {slots:?}, asking {other:?}"
            );
        }
    }
    let second = PageSlots::new(0b10);
    for entry in [ShadowEntry::TablesWide, ShadowEntry::General] {
        let shadow = QuadDraw::Shadow(entry);
        program.ensure_draw(&mut variants, &gpu, target(PageSlots::FIRST), shadow);
        assert!(variants.is_built(shadow, Clipping::Off, PageSlots::FIRST));
        assert!(
            !variants.is_built(shadow, Clipping::Off, second),
            "{entry:?} over slot 0 reads no table on slot 1"
        );
        program.ensure_draw(&mut variants, &gpu, target(second), shadow);
        assert!(variants.is_built(shadow, Clipping::Off, second));
    }
    program.ensure_draw(&mut variants, &gpu, target(PageSlots::FIRST), fill);
    for slots in PageSlots::all() {
        assert!(variants.is_built(fill, Clipping::Off, slots));
    }
}

/// Each fill form's pipeline is labeled by its own shape and brush, so a capture names the
/// form it shows.
#[test]
fn each_fill_form_labels_its_shape_and_brush() {
    use crate::gpu::pipeline::quad_pipeline::draw_label;
    use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
    use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
    use crate::renderer::render_buffer::shape_form::ShapeForm;

    for form in FillForm::ALL {
        let shape = match form.shape() {
            ShapeForm::Box => "box",
            ShapeForm::Triangle => "triangle",
            ShapeForm::Rect => unreachable!("no fill draws a rect"),
        };
        let brush = match form.brush() {
            FillBrush::Solid => "solid",
            FillBrush::Gradient => "gradient",
        };
        assert_eq!(
            draw_label(QuadDraw::Fill(form)),
            format!("palantir.quad.pipeline.{shape}_{brush}"),
        );
    }
}

/// A fill is transparent, and its template's interior collapses, at an alpha of zero of
/// either sign whatever its color; the smallest f16 above zero, `2⁻²⁴ ≈ 5.96e-8`, and an opaque fill
/// paint. On the GPU, over the packing the instance carries.
#[test]
fn a_transparent_fill_is_its_alpha_at_zero() {
    const PROBE: &str = "
@group(0) @binding(14) var<storage, read> probe_in: array<vec2<u32>>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<u32>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
probe_out[id.x] = u32(fill_is_transparent(probe_in[id.x]));
}
";
    let pack = |color: RgbaF32| bytemuck::cast::<_, [u32; 2]>(color.to_f16());
    let smallest = RgbaF32::new(0.0, 0.0, 0.0, 5.960_464_5e-8);
    let cases = [
        (RgbaF32::TRANSPARENT, 1),
        (RgbaF32::new(1.0, 0.5, 0.25, 0.0), 1),
        (RgbaF32::new(0.0, 0.0, 0.0, -0.0), 1),
        (smallest, 0),
        (RgbaF32::new(0.2, 0.4, 0.6, 1.0), 0),
    ];
    let inputs: Vec<[u32; 2]> = cases.iter().map(|&(color, _)| pack(color)).collect();
    let got: Vec<u32> = QuadPipeline::SHADER.probe(PROBE, &inputs);
    let want: Vec<u32> = cases.iter().map(|&(_, want)| want).collect();
    assert_eq!(got, want);
}
