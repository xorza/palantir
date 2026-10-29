//! The panes the cascade lists: each one's plan and footprint, which every later stage reads.

use crate::cascade::pane_row::PaneRow;
use crate::internals::ui_harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::{BackdropFilter, Lens};
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::UVec2;

/// The one pane a frame lists, and its footprint in physical px.
#[derive(Debug)]
struct Listed {
    pane: PaneRow,
    physical: URect,
}

/// The one pane a frame of `filter` at `rect` lists, on a surface of 200² physical px at `scale`.
fn listed(scale: f32, rect: Rect, filter: BackdropFilter) -> Listed {
    let mut h = UiHarness::new(UVec2::splat(200)).scale(scale);
    h.frame(|ui: &mut Ui| {
        Panel::canvas()
            .id_salt("panes")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::rect(rect).backdrop_filter(filter));
            });
    });
    let scene = h.ui.frame_scene();
    let panes: Vec<PaneRow> = scene.panes().copied().collect();
    assert_eq!(panes.len(), 1);
    Listed {
        pane: panes[0],
        physical: panes[0].physical_footprint(scene.stamp.display),
    }
}

/// A pane's plan is its physical σ's, and its footprint its rect grown by the plan's reach and a
/// pixel. At scale 2, σ 5 is 10 physical px: two levels, and a pane at (20, 20)..(60, 40) logical
/// reads (40, 40)..(120, 80) physical grown by the reach and the pixel.
#[test]
fn a_pane_lists_its_plan_and_footprint() {
    let Listed { pane, physical } = listed(
        2.0,
        Rect::new(20.0, 20.0, 40.0, 20.0),
        BackdropFilter::blur(5.0),
    );
    let plan = BlurPlan::new(10.0);
    assert_eq!(plan.levels(), 2);
    assert_eq!(pane.plan, plan);
    let reach = plan.reach() + 1;
    assert_eq!(
        physical,
        URect::from_min_max(
            UVec2::splat(40 - reach.min(40)),
            UVec2::new(120 + reach, 80 + reach).min(UVec2::splat(200)),
        )
    );
}

/// A pane's plan takes its σ in f32. σ 1639/256 at scale 1.25 is 8.0029 physical px, past the
/// 8 px edge of the second level: two levels. Rounded to f16 again it would be 8.0 and one level,
/// whose reach differs.
#[test]
fn a_panes_plan_takes_its_sigma_unrounded() {
    let sigma = 1639.0 / 256.0;
    let physical = sigma * 1.25;
    assert!(physical > 8.0);
    assert_eq!(BlurPlan::new(physical).levels(), 2);
    assert_eq!(BlurPlan::new(8.0).levels(), 1);
    let Listed { pane, .. } = listed(
        1.25,
        Rect::new(20.0, 20.0, 20.0, 20.0),
        BackdropFilter::blur(sigma),
    );
    assert_eq!(pane.plan, BlurPlan::new(physical));
}

/// A pane past every plan and every lens coerces instead of overflowing: σ 65504 logical, the
/// widest a lane holds, at scale 2 plans as `BlurPlan::MAX_SIGMA`; a lens of thickness and index
/// 65504 reaches `u32::MAX`, so the footprint is the whole surface.
#[test]
fn a_pane_past_every_plan_coerces_to_the_widest() {
    let Listed { pane, physical } = listed(
        2.0,
        Rect::new(20.0, 20.0, 40.0, 20.0),
        BackdropFilter::blur(65504.0).with_lens(Lens {
            thickness: 65504.0,
            refractive_index: 65504.0,
            ..Lens::default()
        }),
    );
    assert_eq!(pane.plan, BlurPlan::new(BlurPlan::MAX_SIGMA));
    assert_eq!(
        physical,
        URect::from_min_max(UVec2::ZERO, UVec2::splat(200))
    );
}
