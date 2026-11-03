//! One frame of [`ROWS`] widgets resolving their animated look: the per-widget tick every
//! animating theme slot pays while it retargets, under each kind of motion; the tick of a look at
//! rest; and the call a static theme makes, with no motion.

use crate::animation::AnimationMap;
use crate::animation::animation_map_typed::{AnimationClock, AnimationMapTyped};
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::bench::backends::Backend;
use crate::bench::case_group::CaseGroup;
use crate::bench::kind::{Kind, Scale};
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::look::animated_look::AnimatedLook;
use crate::theme::text_style::TextStyle;
use criterion::{Criterion, Throughput};
use std::hint::black_box;
use std::time::Duration;

const ROWS: usize = 4096;
const SLOT: AnimationSlot = AnimationSlot::new("bench");

#[derive(Clone, Copy, Debug)]
enum Motion {
    Duration,
    Spring,
}

impl Motion {
    const fn label(self) -> &'static str {
        match self {
            Self::Duration => "duration",
            Self::Spring => "spring",
        }
    }

    const fn spec(self) -> AnimationSpec {
        match self {
            Self::Duration => AnimationSpec::duration(Duration::from_millis(200), Easing::OutCubic),
            Self::Spring => AnimationSpec::SPRING,
        }
    }
}

fn look(bg: RgbaF32, text: RgbaF32) -> AnimatedLook {
    AnimatedLook {
        chrome: Chrome::fill(bg),
        text: TextStyle::stock(ColorScheme::Dark).with_color(text),
    }
}

fn ids() -> Vec<WidgetId> {
    (0..ROWS)
        .map(|idx| WidgetId::from_hash(idx as u64))
        .collect()
}

fn first() -> AnimatedLook {
    look(RgbaF32::srgb(0.1, 0.2, 0.3), RgbaF32::WHITE)
}

fn bench_motion(group: &mut CaseGroup<'_>, motion: Motion) {
    group.bench_function(Backend::Cpu.case(motion.label()), |b| {
        let ids = ids();
        let first = first();
        let second = look(RgbaF32::srgb(0.8, 0.4, 0.2), RgbaF32::BLACK);
        let spec = motion.spec();
        let mut map = AnimationMapTyped::default();
        let mut clock = AnimationClock {
            dt: 1.0 / 60.0,
            render_frame_id: 1,
        };
        for &id in &ids {
            map.tick(AnimationRequest::new(id, SLOT, first.clone()), spec, clock);
        }
        let mut use_second = true;
        b.iter(|| {
            clock.render_frame_id += 1;
            let target = if use_second { &second } else { &first };
            use_second = !use_second;
            for &id in &ids {
                black_box(map.tick(AnimationRequest::new(id, SLOT, target.clone()), spec, clock));
            }
        });
    });
}

/// Every row at rest on the target it is asked for: the steady path a settled look takes.
fn bench_steady(group: &mut CaseGroup<'_>) {
    group.bench_function(Backend::Cpu.case("steady"), |b| {
        let ids = ids();
        let target = first();
        let spec = AnimationSpec::SPRING;
        let mut map = AnimationMapTyped::default();
        let mut clock = AnimationClock {
            dt: 1.0 / 60.0,
            render_frame_id: 1,
        };
        for &id in &ids {
            map.tick(AnimationRequest::new(id, SLOT, target.clone()), spec, clock);
        }
        b.iter(|| {
            clock.render_frame_id += 1;
            for &id in &ids {
                black_box(map.tick(AnimationRequest::new(id, SLOT, target.clone()), spec, clock));
            }
        });
    });
}

/// An instant spec, as a static theme passes: the target back with no row.
fn bench_instant(group: &mut CaseGroup<'_>) {
    group.bench_function(Backend::Cpu.case("instant"), |b| {
        let ids = ids();
        let target = first();
        let mut map = AnimationMap::default();
        let mut clock = AnimationClock {
            dt: 1.0 / 60.0,
            render_frame_id: 1,
        };
        b.iter(|| {
            clock.render_frame_id += 1;
            for &id in &ids {
                black_box(map.animate(
                    AnimationRequest::new(id, SLOT, target.clone()),
                    AnimationSpec::SNAP,
                    clock,
                ));
            }
        });
    });
}

/// `atomic/animation/cpu/<case>`: [`ROWS`] widgets resolving their look in one frame, retargeting
/// under each motion, at rest, and with an instant spec. Per widget.
pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "animation", Scale::Sample);
    group.throughput(Throughput::Elements(ROWS as u64));
    for motion in [Motion::Duration, Motion::Spring] {
        bench_motion(&mut group, motion);
    }
    bench_steady(&mut group);
    bench_instant(&mut group);
    group.finish();
}
