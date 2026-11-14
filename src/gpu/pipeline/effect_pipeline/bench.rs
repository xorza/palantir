//! The benches of an effect that paints around a box, over a made scene: 28 cards a 1440p window
//! holds, each with a glow its color effect paints around it, as the showcase's ring does. Its
//! arms:
//!
//! - `none`: the cards alone, the floor;
//! - `rect`: each card's glow an effect over the card grown by [`REACH`], so its fragment stage
//!   runs under the card too, which is drawn over it;
//! - `halo`: the same glow as the card's halo (`EffectPaint::outside`), which runs it outside the
//!   card alone and reads the card's own frame;
//! - `shadow`: each card's drop shadow reaching as far, the quad pipeline's grid around a hole,
//!   for reference.
//!
//! The cases:
//!
//! - `atomic/effect_pass/gpu/<arm>`: the device time of the main pass, a whole repaint a frame.
//! - `atomic/effect_pass_batch/gpu/<kind>/<arm>`: the glows' share of it, the effect batches or the
//!   shadow batches, from in-pass timestamps; only where the adapter writes them, and on a tiler
//!   each timestamp splits the pass, so these do not sum to `effect_pass`.

use crate::bench::backends::Backends;
use crate::bench::fixture;
use crate::bench::kind::{Kind, Scale};
use crate::bench::scene::{BENCH_DPR, BENCH_SURFACE};
use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::effect::effect_source::EffectSource;
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::HarnessDepth;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::shadow::Shadow;
use crate::shape::Shape;
use crate::shape::effect_paint::EffectPaint;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use criterion::Criterion;
use glam::Vec2;

/// Cards a row and rows of cards.
const COLS: usize = 7;
const ROWS: usize = 4;
const CARDS: usize = COLS * ROWS;
/// A card's logical size and radius.
const CARD: Vec2 = Vec2::new(120.0, 90.0);
const RADIUS: f32 = 16.0;
/// How far a glow paints past its card, logical px, and the gap between two glows.
const REACH: f32 = 24.0;
const GAP: f32 = 8.0;
/// Frames each case runs before it checks its paint and starts timing.
const SETTLE_FRAMES: u32 = 4;

/// The glow's parameters, as the showcase's ring takes them: its two colors, how far its card
/// sits inside the effect's rect, the card's radius and the glow's falloff.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
#[repr(C)]
struct Glow {
    color_a: [f32; 4],
    color_b: [f32; 4],
    inset: f32,
    radius: f32,
    falloff: f32,
    pad0: f32,
}

const GLOW: EffectName<Glow> = EffectName::named("bench glow").expect("the name fits");

/// The showcase's ring (`palantir-showcase/src/pages/effects/ring.wgsl`) at rest: a glow that fades
/// from the card's edge to nothing at the rect's, its color turning around the card.
const GLOW_WGSL: &str = "
struct Params {
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    inset: f32,
    radius: f32,
    falloff: f32,
    pad0: f32,
}

fn rounded_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let p = input.pos - input.size * 0.5;
    let d = rounded_box(p, input.size * 0.5 - vec2<f32>(params.inset), params.radius);
    let outside = max(d, 0.0);
    let band = saturate(1.5 - abs(d - 1.0) * input.scale);
    let fade = exp(-outside / params.falloff) * saturate(1.0 - outside / params.inset);
    let glow = max(fade * select(0.0, 0.8, d > 0.0), band);
    let k = 0.5 + 0.5 * sin(atan2(p.y, p.x) + input.time);
    let color = mix(params.color_a, params.color_b, k);
    let a = color.a * glow;
    return effect_over(vec4<f32>(color.rgb * a, a));
}
";

/// The glow as a halo's: the same light measured from the card's own frame, which its input
/// holds, its fade by the halo's reach, so it paints what [`GLOW`] does outside the card.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
#[repr(C)]
struct HaloGlow {
    color_a: [f32; 4],
    color_b: [f32; 4],
    reach: f32,
    falloff: f32,
    pad0: f32,
    pad1: f32,
}

const HALO_GLOW: EffectName<HaloGlow> =
    EffectName::named("bench halo glow").expect("the name fits");

const HALO_GLOW_WGSL: &str = "
struct Params {
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    reach: f32,
    falloff: f32,
    pad0: f32,
    pad1: f32,
}

fn rounded_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let p = input.pos - input.size * 0.5;
    let d = rounded_box(p, input.size * 0.5, input.corners.x);
    let outside = max(d, 0.0);
    let band = saturate(1.5 - abs(d - 1.0) * input.scale);
    let fade = exp(-outside / params.falloff) * saturate(1.0 - outside / params.reach);
    let glow = max(fade * select(0.0, 0.8, d > 0.0), band);
    let k = 0.5 + 0.5 * sin(atan2(p.y, p.x) + input.time);
    let color = mix(params.color_a, params.color_b, k);
    let a = color.a * glow;
    return effect_over(vec4<f32>(color.rgb * a, a));
}
";

/// How a card's glow is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlowArm {
    None,
    Rect,
    Halo,
    Shadow,
}

impl GlowArm {
    const ALL: [Self; 4] = [Self::None, Self::Rect, Self::Halo, Self::Shadow];

    const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Rect => "rect",
            Self::Halo => "halo",
            Self::Shadow => "shadow",
        }
    }

    /// The effect draws a frame of the arm makes.
    const fn effects(self) -> usize {
        match self {
            Self::Rect | Self::Halo => CARDS,
            Self::None | Self::Shadow => 0,
        }
    }
}

/// The card at `i`, in the scene's logical px.
fn card(i: usize) -> Rect {
    let pitch = CARD + Vec2::splat(2.0 * REACH + GAP);
    let at = Vec2::splat(REACH + GAP)
        + Vec2::new((i % COLS) as f32 * pitch.x, (i / COLS) as f32 * pitch.y);
    Rect::new(at.x, at.y, CARD.x, CARD.y)
}

/// The scene's frame under `arm`: every card, with its glow drawn as the arm says.
fn record(ui: &mut Ui, arm: GlowArm) {
    let fill = RgbaF32::srgb(0.2, 0.22, 0.26);
    let (color_a, color_b, falloff) = ([0.2, 0.5, 1.0, 1.0], [0.95, 0.35, 0.55, 1.0], 8.0);
    let glow = Glow {
        color_a,
        color_b,
        inset: REACH,
        radius: RADIUS,
        falloff,
        pad0: 0.0,
    };
    let halo = HaloGlow {
        color_a,
        color_b,
        reach: REACH,
        falloff,
        pad0: 0.0,
        pad1: 0.0,
    };
    let shadow = Shadow::drop(
        RgbaF32::new(0.0, 0.0, 0.0, 0.6),
        Vec2::ZERO,
        REACH / ShadowGeometry::REACH_SIGMAS,
    );
    Panel::canvas()
        .id_salt("effect-bench")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for i in 0..CARDS {
                let card = card(i);
                let shape = Shape::rect().at(card).corners(Corners::all(RADIUS));
                match arm {
                    GlowArm::None => {
                        ui.add_shape(shape, fill);
                    }
                    GlowArm::Rect => {
                        ui.add_shape(
                            Shape::rect().at(card.inflated(REACH)),
                            EffectPaint::new(GLOW, glow),
                        );
                        ui.add_shape(shape, fill);
                    }
                    GlowArm::Halo => {
                        ui.add_shape(shape, EffectPaint::new(HALO_GLOW, halo).outside(REACH));
                        ui.add_shape(shape, fill);
                    }
                    GlowArm::Shadow => {
                        ui.add_shape(shape, Paint::from(fill).with_shadow(shadow));
                    }
                }
            }
        });
}

/// A window of the bench's surface and scale on `gpu`, the glow loaded, whose panels paint no
/// theme chrome and whose every frame paints whole: its clear alternates, as the clip benches' does.
#[derive(Debug)]
struct WholeFrames {
    window: Harness,
    odd: bool,
}

impl WholeFrames {
    fn new(gpu: &BenchGpu) -> Self {
        let mut window = Harness::builder(BENCH_SURFACE)
            .system_scale(BENCH_DPR)
            .swapchain_present()
            .collect_gpu_stats(true)
            .depth(HarnessDepth::gpu(gpu.handles()))
            .build();
        window.ui_mut().theme_mut().panel_chrome = None;
        window
            .ui()
            .load_effect(GLOW, EffectSource::new(GLOW_WGSL))
            .expect("the bench's glow loads");
        window
            .ui()
            .load_effect(HALO_GLOW, EffectSource::new(HALO_GLOW_WGSL))
            .expect("the bench's halo glow loads");
        Self { window, odd: false }
    }

    fn frame(&mut self, gpu: &BenchGpu, arm: GlowArm) {
        self.odd = !self.odd;
        self.window.ui_mut().theme_mut().window_bg = if self.odd {
            RgbaF32::BLACK
        } else {
            RgbaF32::srgb(0.0, 0.0, 0.01)
        };
        let paint = self.window.frame(|ui| record(ui, arm)).paint();
        assert_eq!(paint, FramePaint::Full, "effect scene {}", arm.label());
        let effects = self.window.render_buffer().effects.len();
        assert_eq!(
            effects,
            arm.effects(),
            "effect scene {}: its effect draws",
            arm.label()
        );
        gpu.wait();
        gpu.poll();
    }
}

pub(crate) fn atomic(c: &mut Criterion, backends: &Backends) {
    effect_pass(c, backends);
    effect_pass_batch(c, backends);
}

/// `atomic/effect_pass/gpu/<arm>`: see the module doc.
fn effect_pass(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "effect_pass", Scale::Frame);
    for arm in GlowArm::ALL {
        group.bench_function(backend.case(arm.label()), |b| {
            let gpu = backends.device(Timing::PassOnly);
            let mut frames = WholeFrames::new(gpu);
            let mut time = || {
                frames.frame(gpu, arm);
                frames.window.ui().gpu_pass_stats().last_pass()
            };
            for _ in 0..SETTLE_FRAMES {
                time();
            }
            fixture::settle_timestamps(&mut time);
            b.iter_custom(|iters| fixture::device_time(iters, &mut time));
        });
    }
    group.finish();
}

/// `atomic/effect_pass_batch/gpu/<kind>/<arm>`: see the module doc.
fn effect_pass_batch(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.batch_timed_gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "effect_pass_batch", Scale::Frame);
    for (kind, arm) in [
        (BatchKind::Effect, GlowArm::Rect),
        (BatchKind::Effect, GlowArm::Halo),
        (BatchKind::Shadows, GlowArm::Shadow),
    ] {
        let case = format!("{}/{}", kind.label(), arm.label());
        group.bench_function(backend.case(&case), |b| {
            let gpu = backends.device(Timing::Instrumented);
            let mut frames = WholeFrames::new(gpu);
            let mut time = || {
                frames.frame(gpu, arm);
                frames.window.ui().gpu_pass_stats().last_kind(kind)
            };
            for _ in 0..SETTLE_FRAMES {
                time();
            }
            fixture::settle_timestamps(&mut time);
            b.iter_custom(|iters| fixture::device_time(iters, &mut time));
        });
    }
    group.finish();
}
