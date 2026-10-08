//! One frame of [`ROWS`] widgets retargeting their animated look: the per-widget
//! tick every animating theme slot pays, under each kind of motion.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::bench::Run;
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
    let ids: Vec<_> = (0..ROWS)
        .map(|index| WidgetId::from_hash(index as u64))
        .collect();
    let first = look(RgbaF32::srgb(0.1, 0.2, 0.3), RgbaF32::WHITE);
    let second = look(RgbaF32::srgb(0.8, 0.4, 0.2), RgbaF32::BLACK);
    let spec = motion.spec();
    let mut map = AnimMapTyped::default();
    for &id in &ids {
        map.tick(id, SLOT, first.clone(), spec, 1.0 / 60.0, 1);
    }

    let mut render_frame_id = 1u64;
    let mut use_second = true;
    group.bench_function(motion.label(), |b| {
        b.iter(|| {
            render_frame_id += 1;
            let target = if use_second { &second } else { &first };
            use_second = !use_second;
            for &id in &ids {
                black_box(map.tick(id, SLOT, target.clone(), spec, 1.0 / 60.0, render_frame_id));
            }
        });
    });
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.group(c);
    group.throughput(Throughput::Elements(ROWS as u64));
    for motion in [Motion::Duration, Motion::Spring] {
        bench_motion(&mut group, motion);
    }
    group.finish();
}
