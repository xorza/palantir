//! GPU debug groups (RenderDoc / Xcode capture labels), compiled out unless the `gpu-debug-markers` feature is on.
//!
//! A push/pop pair per draw step costs CPU even with no capture tool attached: wgpu 30's `render_pass_push_debug_group` copies the label into the pass's `string_data` and pushes two `ArcRenderCommand`s, and only the HAL call is conditional. Step count grows with UI complexity, so a few hundred groups cost several hundred commands. The labels are worth keeping, so they are gated; enable the feature when capturing (`showcase` does).

/// One `cfg!` rather than a `#[cfg]` pair per call: the constant folds away, so a marker-free build emits neither the wgpu call nor a second body.
const ENABLED: bool = cfg!(feature = "gpu-debug-markers");

#[inline]
pub(crate) fn push(pass: &mut wgpu::RenderPass<'_>, label: &str) {
    if ENABLED {
        pass.push_debug_group(label);
    }
}

#[inline]
pub(crate) fn pop(pass: &mut wgpu::RenderPass<'_>) {
    if ENABLED {
        pass.pop_debug_group();
    }
}

#[inline]
pub(crate) fn push_encoder(encoder: &mut wgpu::CommandEncoder, label: &str) {
    if ENABLED {
        encoder.push_debug_group(label);
    }
}

#[inline]
pub(crate) fn pop_encoder(encoder: &mut wgpu::CommandEncoder) {
    if ENABLED {
        encoder.pop_debug_group();
    }
}
