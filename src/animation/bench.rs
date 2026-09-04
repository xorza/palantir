use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::anim_slot::AnimSlot;
use crate::animation::anim_spec::AnimSpec;
use crate::animation::easing::Easing;
use crate::bench::Run;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::animated_look::AnimatedLook;
use criterion::{Criterion, Throughput};
use std::hint::black_box;
use std::time::Duration;

const ROWS: usize = 4096;
const SLOT: AnimSlot = AnimSlot::new("bench");

#[derive(Clone, Copy, Debug)]
enum Motion {
    Duration,
    Spring,
}

impl Motion {
    fn spec(self) -> AnimSpec {
        match self {
            Self::Duration => AnimSpec::duration(0.2, Easing::OutCubic),
            Self::Spring => AnimSpec::SPRING,
        }
    }
}

fn look(background: RgbaF32, text: RgbaF32) -> AnimatedLook {
    AnimatedLook {
        background: Background::fill(background),
        text: TextStyle::default().with_color(text),
    }
}

fn bench_motion(c: &mut Criterion, run: Run<'_>, sub: &str, motion: Motion) {
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
    let mut group = run.subgroup(c, sub);
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));
    group.throughput(Throughput::Elements(ROWS as u64));
    group.bench_function("animated_look", |b| {
        b.iter(|| {
            render_frame_id += 1;
            let target = if use_second { &second } else { &first };
            use_second = !use_second;
            for &id in &ids {
                black_box(map.tick(id, SLOT, target.clone(), spec, 1.0 / 60.0, render_frame_id));
            }
        });
    });
    group.finish();
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    bench_motion(c, run, "duration", Motion::Duration);
    bench_motion(c, run, "spring", Motion::Spring);
}
