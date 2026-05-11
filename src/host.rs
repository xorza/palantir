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
//! (`Damage`, debug overlay, frame-state Arc) is stashed in
//! [`Host`] itself; the user-facing [`FrameInfo`] is plain owned data.

use std::time::Instant;

use crate::Display;
use crate::debug_overlay::DebugOverlayConfig;
use crate::primitives::color::Color;
use crate::renderer::backend::WgpuBackend;
use crate::renderer::frontend::{FrameState, Frontend};
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::ui::damage::Damage;

/// Owns the full palantir pipeline: [`Ui`] (record/layout/cascade/damage)
/// plus the CPU [`Frontend`](crate::renderer::frontend::Frontend) and
/// GPU [`WgpuBackend`](crate::renderer::backend::WgpuBackend). The
/// renderer halves are private; reach the recorder via the public
/// [`Host::ui`] field.
pub struct Host {
    pub ui: Ui,
    /// Per-frame debug visualizations. Default = all-off. Read by
    /// `render` after `run_frame`; flip flags between frames.
    pub debug_overlay: DebugOverlayConfig,
    pub(crate) frontend: Frontend,
    pub(crate) backend: WgpuBackend,
    /// Set by `run_frame`, consumed by `render`. `None` if `render`
    /// wasn't called after the last `run_frame` (e.g. host bailed on
    /// a `Skip` frame); the next `run_frame` overwrites it.
    pub(crate) pending: Option<PendingSubmit>,
    /// Monotonic clock anchor — `start.elapsed()` feeds `Ui::frame`
    /// each call so the host doesn't have to thread a clock through.
    pub(crate) start: Instant,
}

pub(crate) struct PendingSubmit {
    pub(crate) damage: Damage,
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
            debug_overlay: DebugOverlayConfig::default(),
            frontend: Frontend::default(),
            backend: WgpuBackend::new(device, queue, format, shaper),
            pending: None,
            start: Instant::now(),
        }
    }

    /// Drive one CPU frame: `Ui::run_frame` → encode → compose.
    /// Returns the host-facing [`FrameInfo`]; internal state needed
    /// by [`Self::render`] is stashed.
    pub fn run_frame(&mut self, display: Display, record: impl FnMut(&mut Ui)) -> FrameInfo {
        let Some(frame) = self.ui.frame(display, self.start.elapsed(), record) else {
            // Skip: keep the prior composer buffer untouched; the
            // backend's Skip path short-circuits to a backbuffer →
            // surface copy without reading it.
            self.pending = Some(PendingSubmit {
                damage: Damage::Skip,
                frame_state: self.ui.frame_state.clone(),
            });
            return FrameInfo {
                can_skip_rendering: true,
                repaint_requested: self.ui.repaint_requested,
            };
        };
        let info = FrameInfo {
            can_skip_rendering: false,
            repaint_requested: frame.repaint_requested(),
        };
        self.frontend.build(&frame);
        self.pending = Some(PendingSubmit {
            damage: frame.damage,
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
            self.debug_overlay,
            &p.frame_state,
        );
    }
}
