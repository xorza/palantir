//! The backdrops the cascade lists: each one's plan and footprint, which every later stage reads.

use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::display::system_scale::SystemScale;
use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::shadow::Shadow;
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// The one backdrop a frame lists, and its footprint in physical px.
#[derive(Debug)]
struct Listed {
    backdrop: PlaneBackdrop,
    physical: URect,
}

/// The one backdrop a frame of `filter` at `rect` lists, on a surface of 200² physical px at
/// `scale`.
fn listed(scale: f32, rect: Rect, filter: &Material) -> Listed {
    listed_under(scale, None, rect, filter)
}

/// [`listed`] with the backdrop inside a canvas that clips to `clip` logical px square when some.
fn listed_under(scale: f32, clip: Option<f32>, rect: Rect, filter: &Material) -> Listed {
    listed_drawn(scale, clip, |ui| {
        ui.add_shape(Shape::rect().at(rect), filter.clone());
    })
}

/// The one backdrop `draw` adds to a canvas that clips to `clip` logical px square when some, on a
/// surface of 200² physical px at `scale`.
fn listed_drawn(scale: f32, clip: Option<f32>, draw: impl Fn(&mut Ui)) -> Listed {
    let mut h = Harness::builder(UVec2::splat(200))
        .mono()
        .system_scale(SystemScale::new(scale).unwrap())
        .build();
    h.frame(|ui: &mut Ui| {
        let canvas = Panel::canvas().id_salt("backdrops");
        let canvas = match clip {
            Some(side) => canvas
                .size((Sizing::fixed(side), Sizing::fixed(side)))
                .clip_rect(),
            None => canvas.size((Sizing::FILL, Sizing::FILL)),
        };
        canvas.show(ui, |ui| draw(ui));
    });
    let scene = h.frames().ui().frame_scene();
    let backdrops: Vec<PlaneBackdrop> = scene.backdrops().copied().collect();
    assert_eq!(backdrops.len(), 1);
    Listed {
        backdrop: backdrops[0],
        physical: backdrops[0].physical_footprint(scene.stamp.display),
    }
}

/// A backdrop's plan is its physical σ's, and its footprint its rect grown by the plan's reach and
/// a pixel. At scale 2, σ 5 is 10 physical px: two levels, and a backdrop at (20, 20)..(60, 40)
/// logical reads (40, 40)..(120, 80) physical grown by the reach and the pixel.
#[test]
fn a_backdrop_lists_its_plan_and_footprint() {
    let Listed { backdrop, physical } =
        listed(2.0, Rect::new(20.0, 20.0, 40.0, 20.0), &Material::blur(5.0));
    let plan = BlurPlan::new(10.0);
    assert_eq!(plan.levels(), 2);
    assert_eq!(backdrop.plan, plan);
    let reach = plan.reach() + 1;
    assert_eq!(
        physical,
        URect::from_min_max(
            UVec2::splat(40 - reach.min(40)),
            UVec2::new(120 + reach, 80 + reach).min(UVec2::splat(200)),
        )
    );
}

/// A backdrop's plan takes its σ in f32. σ 1639/256 at scale 1.25 is 8.0029 physical px, past the
/// 8 px edge of the second level: two levels. Rounded to f16 again it would be 8.0 and one level,
/// whose reach differs.
#[test]
fn a_backdrops_plan_takes_its_sigma_unrounded() {
    let sigma = 1639.0 / 256.0;
    let physical = sigma * 1.25;
    assert!(physical > 8.0);
    assert_eq!(BlurPlan::new(physical).levels(), 2);
    assert_eq!(BlurPlan::new(8.0).levels(), 1);
    let Listed { backdrop, .. } = listed(
        1.25,
        Rect::new(20.0, 20.0, 20.0, 20.0),
        &Material::blur(sigma),
    );
    assert_eq!(backdrop.plan, BlurPlan::new(physical));
}

/// A backdrop past every plan and every lens coerces instead of overflowing: σ 65504 logical, the
/// widest a lane holds, at scale 2 plans as `BlurPlan::MAX_SIGMA`; a lens of thickness and index
/// 65504 reaches `u32::MAX`, so the footprint is the whole surface.
#[test]
fn a_backdrop_past_every_plan_coerces_to_the_widest() {
    let Listed { backdrop, physical } = listed(
        2.0,
        Rect::new(20.0, 20.0, 40.0, 20.0),
        &Material::glass(Lens {
            thickness: 65504.0,
            refractive_index: 65504.0,
            ..Lens::WINDOW_GLASS
        })
        .with_backdrop_blur(65504.0),
    );
    assert_eq!(backdrop.plan, BlurPlan::new(BlurPlan::MAX_SIGMA));
    assert_eq!(
        physical,
        URect::from_min_max(UVec2::ZERO, UVec2::splat(200))
    );
}

/// A glass backdrop's footprint grows by what its lens reads past it. The lens below leans `8 ·
/// √(1.5² − 1) = 8.94` px, a reach of 10. A backdrop 80 px high holds its 10 px bezel, the lean and
/// a pixel, and reads a pixel past itself; one 12 px high lets a sample out by `6 + 8.94 − 12 + 1`,
/// so 5; one a clip cuts keeps the reach, as its screen rect is not its own. Sharp, so the plan
/// reads nothing past a pixel; at scale 1, so logical px are physical.
#[test]
fn a_glass_backdrops_footprint_holds_what_its_lens_reads() {
    let lens = Lens {
        bezel: 10.0,
        thickness: 8.0,
        refractive_index: 1.5,
        dispersion: 0.0,
        highlight: 0.0,
    };
    let glass = Material::glass(lens).with_backdrop_blur(0.0);
    let reach = BlurPlan::new(0.0).reach() + 1;
    let grown = |rect: URect, by: u32| {
        URect::from_min_max(rect.min - UVec2::splat(by), rect.max() + UVec2::splat(by))
    };
    for (what, clip, rect, margin) in [
        ("wide", None, Rect::new(40.0, 40.0, 100.0, 80.0), 1),
        ("thin", None, Rect::new(40.0, 40.0, 100.0, 12.0), 5),
        (
            "clipped",
            Some(120.0),
            Rect::new(40.0, 40.0, 100.0, 60.0),
            10,
        ),
    ] {
        let Listed { physical, .. } = listed_under(1.0, clip, rect, &glass);
        let rect = URect::covering(match clip {
            Some(side) => Rect::new(40.0, 40.0, side - 40.0, 60.0),
            None => rect,
        });
        assert_eq!(physical, grown(rect, reach + margin), "{what}");
    }
}

/// A shadow is no part of a backdrop, and a triangle's lens keeps its reach. The lens below leans
/// `20 · √(1.5² − 1) = 22.36` px past a bezel of 4: a reach of 24. A box 10 px high lets a sample
/// out by `4 + 22.36 − 10 + 1`, so 18, and a pixel for the bilinear read, 19. Under a drop shadow
/// of σ 1, whose halo is 4, its paint rect is (36, 36)..(144, 54), grown by those 19 too: the
/// paint rect's own side, 18, gave a margin of 11, and a footprint 4 px short of what the lens
/// reads past the box. A triangle at (40, 40), (100, 40), (40, 100) keeps the reach, 24, around its
/// paint rect, its 60 px box grown by its edge's half-pixel ramp, (39, 39)..(101, 101) in whole
/// pixels: a lens's chord near a corner is shorter than the box's side, which gave a margin of 1.
/// At scale 1, so logical px are physical, and sharp, so the plan reads a pixel.
#[test]
fn a_backdrops_lens_margin_is_its_own_shapes() {
    let lens = Lens {
        bezel: 4.0,
        thickness: 20.0,
        refractive_index: 1.5,
        dispersion: 0.0,
        highlight: 0.0,
    };
    let glass = Material::glass(lens).with_backdrop_blur(0.0);
    let reach = BlurPlan::new(0.0).reach() + 1;
    let grown = |rect: URect, by: u32| {
        URect::from_min_max(rect.min - UVec2::splat(by), rect.max() + UVec2::splat(by))
    };

    let shadowed = listed_drawn(1.0, None, |ui| {
        ui.add_shape(
            Shape::rect().at(Rect::new(40.0, 40.0, 100.0, 10.0)),
            Paint::from(glass.clone()).with_shadow(Shadow::drop(RgbaF32::BLACK, Vec2::ZERO, 1.0)),
        );
    });
    assert_eq!(shadowed.backdrop.rect, Rect::new(36.0, 36.0, 108.0, 18.0));
    assert_eq!(
        shadowed.physical,
        grown(
            URect::from_min_max(UVec2::splat(36), UVec2::new(144, 54)),
            reach + 19
        )
    );

    let triangle = listed_drawn(1.0, None, |ui| {
        ui.add_shape(
            Shape::triangle(Vec2::ZERO, Vec2::new(60.0, 0.0), Vec2::new(0.0, 60.0))
                .at(Rect::new(40.0, 40.0, 60.0, 60.0)),
            glass.clone(),
        );
    });
    assert_eq!(
        triangle.physical,
        grown(
            URect::from_min_max(UVec2::splat(39), UVec2::splat(101)),
            reach + 24
        )
    );
}
