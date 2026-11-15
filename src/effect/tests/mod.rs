//! The effects' tests and the fixtures they share: [`loading`] holds what a load checks and what a
//! name resolves to, [`painting`] what an effect paints on a device.

use crate::shape::effect_paint::EffectPaint;
use std::error;
use std::ptr;
use std::rc::Rc;

use glam::{UVec2, Vec2};
use half::f16;
use std::time::Duration;

use crate::animation::motion_role::MotionRole;
use crate::effect::contract::{self, ContractSpec, ParamsLayout};
use crate::effect::effect_module::EffectModule;
use crate::effect::effect_registry::{EffectRegistry, MaterialFit, ReducedName};
use crate::effect::effect_source::{EffectSource, ReducedEffect};
use crate::effect::error::{EffectError, EntryMismatch, ParamMismatch};
use crate::effect::stock;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::gpu::test_gpu::isolated_headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::HarnessDepth;
use crate::image_registry::image_handle::ImageHandle;
use crate::internals::paint_capture::PaintCall;
use crate::internals::panic_probe::assert_panics_with;
use crate::internals::srgb_reference;
use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, KindId};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::{EffectParam, EffectParams, ParamType};
use crate::primitives::effect::press_params::PressParams;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::Image;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::backdrop_reach::BackdropReach;
use crate::primitives::paint::material::frosted_params::FrostedParams;
use crate::primitives::paint::material::glass_params::GlassParams;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::material::material_effect::MaterialEffect;
use crate::primitives::preference::Preference;
use crate::primitives::reduction::Reduction;
use crate::renderer::frontend::payload::draw_closed_payload::closed_effect::{
    EffectCaptureDraw, EffectRead,
};
use crate::shape::Shape;
use crate::shape::paint::capture_params::CaptureReach;
use crate::shape::paint_animation::{PaintAnimation, PaintRepeat};
use crate::theme::Theme;
use crate::theme::error::ThemeEffectError;
use crate::theme::look::press_effect::PressEffect;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::ui::ui_preferences::UiPreferences;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

/// The contract an effect of `kind` over `params` with `image_slots` images is checked against.
const fn spec(kind: KindId, params: ParamsLayout, image_slots: u8) -> ContractSpec {
    ContractSpec {
        kind,
        params,
        image_slots,
    }
}

/// The id of the app's load `n`, from 0, past the stock effects every registry holds first.
fn app(n: u32) -> EffectId {
    let count = stock::effects().press.len() + 2;
    EffectId(u32::try_from(count).unwrap() + n)
}

/// A ring's parameters: a vec4 and four f32s, 32 B in Rust and in WGSL.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
#[repr(C)]
struct Ring {
    tint: [f32; 4],
    radius: f32,
    width: f32,
    edge: f32,
    r#speed: f32,
}

const RING: EffectName<Ring> = EffectName::named("ring").expect("the name fits");
const THIN_RING: EffectName<Ring> = EffectName::named("thin ring").expect("the name fits");

const RING_PARAMS: &str = "
struct Params { tint: vec4<f32>, radius: f32, width: f32, edge: f32, speed: f32 }
";

/// A ring's WGSL with `params` as its parameter struct and `entry` as its function.
fn ring_wgsl(params: &str, entry: &str) -> String {
    format!("{params}\n{entry}")
}

const RING_ENTRY: &str = "
fn effect(input: EffectInput, params: Params) -> EffectOutput {
    return EffectOutput(params.tint, vec4<f32>(0.0));
}
";

fn ring(wgsl: impl Into<String>) -> EffectSource<Ring> {
    EffectSource::new(wgsl.into())
}

fn load(wgsl: &str) -> Result<(), EffectError> {
    EffectRegistry::default().load(RING, &ring(wgsl.to_owned()))
}

/// A blend picker's parameters: a premultiplied linear color, and which blend `effect` returns.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
#[repr(C)]
struct Blend {
    color: [f32; 4],
    mode: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

const BLEND: EffectName<Blend> = EffectName::named("blend").expect("the name fits");

const BLEND_WGSL: &str = "
struct Params { color: vec4<f32>, mode: u32, pad0: u32, pad1: u32, pad2: u32 }

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    switch params.mode {
        case 0u: { return effect_over(params.color); }
        case 1u: { return effect_add(params.color); }
        case 2u: { return effect_multiply(params.color); }
        case 3u: { return effect_screen(params.color); }
        // A probe of the input: its size over 80 and its first corner, opaque.
        default: { return effect_over(vec4<f32>(input.size / 80.0, input.corners.x, 1.0)); }
    }
}
";

/// A shift's parameters: how far right a backdrop effect reads, in logical px.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
#[repr(C)]
struct Shift {
    dx: f32,
}

const SHIFT: EffectName<Shift, BackdropEffect> = EffectName::named("shift").expect("the name fits");

/// [`SHIFT`]'s source, which reads 8 px past its shape, as its `dx` reaches.
fn reaching_shift() -> EffectSource<Shift, BackdropEffect> {
    EffectSource::new(SHIFT_WGSL).reach(|_| BackdropReach::Px(8.0))
}

/// What is painted `dx` px to the right, opaque.
const SHIFT_WGSL: &str = "
struct Params { dx: f32 }

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    return effect_over(effect_backdrop(input.pos + vec2<f32>(params.dx, 0.0)));
}
";

mod loading;
mod painting;
