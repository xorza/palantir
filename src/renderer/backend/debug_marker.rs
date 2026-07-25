//! GPU debug groups (RenderDoc / Xcode capture labels), compiled out
//! unless the `gpu-debug-markers` feature is on.
//!
//! Every emitted draw step used to record a pair unconditionally. In
//! wgpu 30 a pair is not free on the CPU even when no capture tool is
//! attached: `render_pass_push_debug_group` memcpys the label bytes into
//! the pass's `string_data`, pushes two `ArcRenderCommand`s into the
//! command vec, and adds two iterations to the command-replay match at
//! pass end. Only the HAL call is conditional on the `debug_utils`
//! extension. Step count is exactly what grows with UI complexity, so a
//! frame with a few hundred groups paid several hundred recorded
//! commands for tooling nobody had attached.
//!
//! The markers themselves are worth keeping — a capture of this renderer
//! is worth far more than the CPU it costs, and the labels are well
//! chosen — so this gates them rather than deleting them. Turn the
//! feature on whenever you intend to capture; `showcase` enables it.

#[cfg(feature = "gpu-debug-markers")]
#[inline]
pub(super) fn push(pass: &mut wgpu::RenderPass<'_>, label: &str) {
    pass.push_debug_group(label);
}

#[cfg(not(feature = "gpu-debug-markers"))]
#[inline]
pub(super) fn push(_pass: &mut wgpu::RenderPass<'_>, _label: &str) {}

#[cfg(feature = "gpu-debug-markers")]
#[inline]
pub(super) fn pop(pass: &mut wgpu::RenderPass<'_>) {
    pass.pop_debug_group();
}

#[cfg(not(feature = "gpu-debug-markers"))]
#[inline]
pub(super) fn pop(_pass: &mut wgpu::RenderPass<'_>) {}

#[cfg(feature = "gpu-debug-markers")]
#[inline]
pub(super) fn push_encoder(encoder: &mut wgpu::CommandEncoder, label: &str) {
    encoder.push_debug_group(label);
}

#[cfg(not(feature = "gpu-debug-markers"))]
#[inline]
pub(super) fn push_encoder(_encoder: &mut wgpu::CommandEncoder, _label: &str) {}

#[cfg(feature = "gpu-debug-markers")]
#[inline]
pub(super) fn pop_encoder(encoder: &mut wgpu::CommandEncoder) {
    encoder.pop_debug_group();
}

#[cfg(not(feature = "gpu-debug-markers"))]
#[inline]
pub(super) fn pop_encoder(_encoder: &mut wgpu::CommandEncoder) {}
