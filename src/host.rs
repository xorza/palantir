//! `Host` — the top-level palantir handle owning the recorder
//! ([`Ui`]), the CPU paint stage ([`Frontend`]), and the GPU backend
//! ([`WgpuBackend`]). One type to hold; one [`Host::run_frame`] +
//! [`Host::render`] pair per frame.
//!
//! The two-stage split (`run_frame` → CPU work; `render` → GPU
//! submit) lets the host bail out between the two on `Skip` frames —
//! no `surface.get_current_texture()`, no submit, no present — and
//! also on host-side errors (surface acquire failure, occluded
//! window). State that needs to flow between the two stages
//! (`DamagePaint`, debug overlay, frame-state Arc) is stashed in
//! [`Host`] itself; the user-facing [`FrameInfo`] is plain owned data.

use crate::primitives::color::Color;
use crate::renderer::backend::WgpuBackend;
use crate::renderer::frontend::{FrameState, Frontend};
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::ui::damage::DamagePaint;
use crate::ui::debug_overlay::DebugOverlayConfig;

/// Owns the full palantir pipeline: [`Ui`] (record/layout/cascade/damage)
/// plus the CPU [`Frontend`](crate::renderer::frontend::Frontend) and
/// GPU [`WgpuBackend`](crate::renderer::backend::WgpuBackend). The
/// renderer halves are private; reach the recorder via the public
/// [`Host::ui`] field.
pub struct Host {
    pub ui: Ui,
    pub(crate) frontend: Frontend,
    pub(crate) backend: WgpuBackend,
    /// Set by `run_frame`, consumed by `render`. `None` if `render`
    /// wasn't called after the last `run_frame` (e.g. host bailed on
    /// a `Skip` frame); the next `run_frame` overwrites it.
    pub(crate) pending: Option<PendingSubmit>,
}

pub(crate) struct PendingSubmit {
    pub(crate) damage: DamagePaint,
    pub(crate) debug_overlay: Option<DebugOverlayConfig>,
    pub(crate) frame_state: FrameState,
}

/// What [`Host::run_frame`] tells the host about the frame it just
/// recorded. Owned, no borrows — the caller can inspect both fields,
/// branch on them, and (when not skipping) call [`Host::render`].
pub struct FrameInfo {
    /// `true` when this frame's damage diff produced no work — the
    /// backbuffer already holds the right pixels. Hosts can skip
    /// `surface.get_current_texture()` + render + present entirely.
    pub can_skip_rendering: bool,
    /// `true` when an animation tick during this frame hasn't
    /// settled. Hosts honor by re-requesting a redraw so the next
    /// frame runs even when input is idle.
    pub repaint_requested: bool,
}

impl Host {
    /// Construct with a bundled-fonts shaper shared between the `Ui`
    /// (measurement) and the backend (rasterization) so they hit one
    /// buffer cache.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::with_text(device, queue, format, TextShaper::with_bundled_fonts())
    }

    /// Construct with a caller-supplied shaper. Tests that want to
    /// amortize font loading across many `Host` instances pass a
    /// `clone()` of a shared `TextShaper`. The handle is installed on
    /// both the `Ui` (measurement) and the backend (rasterization).
    pub fn with_text(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        shaper: TextShaper,
    ) -> Self {
        Self {
            ui: Ui::with_text(shaper.clone()),
            frontend: Frontend::default(),
            backend: WgpuBackend::new(device, queue, format, shaper),
            pending: None,
        }
    }

    /// Drive one CPU frame: `Ui::run_frame` → encode → compose.
    /// Returns the host-facing [`FrameInfo`]; internal state needed
    /// by [`Self::render`] is stashed.
    pub fn run_frame(
        &mut self,
        display: crate::layout::types::display::Display,
        now: std::time::Duration,
        record: impl FnMut(&mut Ui),
    ) -> FrameInfo {
        let frame = self.ui.run_frame(display, now, record);
        let info = FrameInfo {
            can_skip_rendering: frame.can_skip_rendering(),
            repaint_requested: frame.repaint_requested(),
        };
        self.frontend.build(
            frame.forest,
            frame.layout,
            frame.cascades,
            frame.damage_filter(),
            &frame.display,
        );
        self.pending = Some(PendingSubmit {
            damage: frame.damage,
            debug_overlay: frame.debug_overlay,
            frame_state: frame.frame_state.clone(),
        });
        info
    }

    /// GPU submit half. Call after [`Self::run_frame`] when the host
    /// wants to paint (i.e. `FrameInfo::can_skip_rendering` was
    /// false). No-op if called without a preceding `run_frame` or
    /// after a frame the host elected to skip.
    pub fn render(&mut self, surface_tex: &wgpu::Texture, clear: Color) {
        let Some(p) = self.pending.take() else {
            return;
        };
        self.backend.submit(
            surface_tex,
            clear,
            &self.frontend.composer.buffer,
            &mut self.frontend.gradient_atlas,
            p.damage,
            p.debug_overlay,
            &p.frame_state,
        );
    }
}
