    pub fn cpu_frame<T: 'static>(
        host: &mut Host<T>,
        display: Display,
        state: &mut T,
        record: impl FnMut(&mut Ui<T>),
    ) -> FrameReport {
        host.cpu_frame(display, state, record)
    }

    /// GPU half of `Host::frame` against a caller-supplied texture.
    pub fn render_to_texture<T: 'static>(
        host: &mut Host<T>,
        target: &wgpu::Texture,
        report: &FrameReport,
    ) {
        host.render_to_texture(target, report);
    }

    make these impl Host
