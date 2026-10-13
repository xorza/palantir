//! One frame of [`ROWS`] widgets retargeting their animated look: the per-widget
//! tick every animating theme slot pays, under each kind of motion.

use crate::animation::anim_map_typed::{AnimClock, AnimMapTyped};
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::bench::fixture::{Backend, Kind, Scale};
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::widget_look::animated_look::AnimatedLook;
use crate::widgets::theme::text_style::TextStyle;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, Throughput};
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

fn look(background: RgbaF32, text: RgbaF32) -> AnimatedLook {
    AnimatedLook {
        background: Background::fill(background),
        text: TextStyle::default().with_color(text),
    }
}

fn bench_motion(group: &mut BenchmarkGroup<'_, WallTime>, motion: Motion) {
    group.bench_function(Backend::Cpu.case(motion.label()), |b| {
        let ids: Vec<_> = (0..ROWS)
            .map(|index| WidgetId::from_hash(index as u64))
            .collect();
        let first = look(RgbaF32::srgb(0.1, 0.2, 0.3), RgbaF32::WHITE);
        let second = look(RgbaF32::srgb(0.8, 0.4, 0.2), RgbaF32::BLACK);
        let spec = motion.spec();
        let mut map = AnimMapTyped::default();
        let mut clock = AnimClock {
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

/// `atomic/animation/cpu/<motion>`: [`ROWS`] widgets retargeting their look in one frame. Per
/// widget.
pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "animation", Scale::Sample);
    group.throughput(Throughput::Elements(ROWS as u64));
    for motion in [Motion::Duration, Motion::Spring] {
        bench_motion(&mut group, motion);
    }
    group.finish();
}
