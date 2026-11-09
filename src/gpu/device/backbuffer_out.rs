//! How a frame painted in the backbuffer reaches a target that takes a copy, which decides what a
//! partial frame's last step costs.

/// How [`Backbuffer::present_onto`](crate::gpu::surface::backbuffer::Backbuffer::present_onto)
/// puts the backbuffer on a target that takes a copy as well as a draw. A target that takes no
/// copy, a GLES swapchain image, always takes the draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackbufferOut {
    /// A texture copy: the immediate-mode GPUs, on which the draw costs more. Partial frames
    /// through the draw against the copy: a mobile AMD iGPU under Windows +8 to +10 %, a high-end NVIDIA GPU +4 to +5 %, the
    /// a handheld's RADV +0.3 to +2.6 %.
    Copy,
    /// A full-screen draw in a pass that clears the target: Apple's GPUs, whose pass writes the
    /// target from its tiles where the copy reads it through another engine. On the Apple laptop
    /// the draw takes 2 to 6 % off the partial frames that capture a backdrop
    /// (`backdrop_frame/gpu/blur/sliding` −6.2 %, `glass/under` −4.5 %) and leaves the rest within
    /// 1 %.
    Draw,
}

impl BackbufferOut {
    /// How the backbuffer reaches a target on the adapter `info` describes: the draw on an Apple
    /// GPU, which Metal reports by memoryless storage, the copy elsewhere. Refused: the draw on
    /// every device whose transient attachments save memory, the other tilers among them, which
    /// nobody measured.
    pub(crate) fn of(info: &wgpu::AdapterInfo) -> Self {
        if info.backend == wgpu::Backend::Metal && info.transient_saves_memory == Some(true) {
            Self::Draw
        } else {
            Self::Copy
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::backbuffer_out::BackbufferOut;

    /// Metal with memoryless storage, an Apple GPU, draws; Metal without it, an Intel or AMD GPU
    /// in a Mac, copies, as does every other backend, a Vulkan device whose transient attachments
    /// save memory included.
    #[test]
    fn an_apple_gpu_draws_the_backbuffer_out() {
        let info = |backend, transient| wgpu::AdapterInfo {
            transient_saves_memory: transient,
            ..wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, backend)
        };
        for (backend, transient, want) in [
            (wgpu::Backend::Metal, Some(true), BackbufferOut::Draw),
            (wgpu::Backend::Metal, Some(false), BackbufferOut::Copy),
            (wgpu::Backend::Metal, None, BackbufferOut::Copy),
            (wgpu::Backend::Vulkan, Some(true), BackbufferOut::Copy),
            (wgpu::Backend::Vulkan, None, BackbufferOut::Copy),
            (wgpu::Backend::Gl, Some(true), BackbufferOut::Copy),
        ] {
            assert_eq!(
                BackbufferOut::of(&info(backend, transient)),
                want,
                "{backend:?} {transient:?}"
            );
        }
    }
}
