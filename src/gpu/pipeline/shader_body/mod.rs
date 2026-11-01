//! Assembly of a pipeline's WGSL: the shared prelude, then the body a pipeline family owns, with
//! Rust-owned constants substituted.

use crate::primitives::paint::antialias::AA_HALF_WIDTH;

/// Concatenated ahead of every shader body, so shared vocabulary has one definition.
const PRELUDE: &str = include_str!("../prelude.wgsl");

/// The gradient group's bindings and its LUT read, for the shaders that bind it.
pub(crate) const GRADIENT: &str = include_str!("../gradient.wgsl");

/// A shader body a pipeline family owns: its WGSL, its label, and the numbers it takes from the
/// Rust types that own them. Every shader is built through [`Self::module`], so the prelude
/// precedes all and an unsubstituted marker panics rather than compiling with a comment where a
/// number belongs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShaderBody {
    pub(crate) label: &'static str,
    pub(crate) wgsl: &'static str,
    /// Shared WGSL the body reads, put between the prelude and the body.
    pub(crate) includes: &'static [&'static str],
    /// The numbers the body declares as markers.
    pub(crate) constants: fn() -> Vec<ShaderConstant>,
}

impl ShaderBody {
    /// The complete source: [`PRELUDE`] in front, every constant substituted.
    fn specialize(self) -> String {
        let body: String = self.includes.iter().copied().chain([self.wgsl]).collect();
        specialize_source(&body, &(self.constants)())
    }

    /// The shader module for this body.
    pub(crate) fn module(self, device: &wgpu::Device) -> wgpu::ShaderModule {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(self.label),
            source: wgpu::ShaderSource::Wgsl(self.specialize().into()),
        })
    }
}

/// A number substituted for its marker, `/*{MARKER}*/`, in a shader body.
#[derive(Debug)]
pub(crate) struct ShaderConstant {
    marker: &'static str,
    value: String,
}

impl ShaderConstant {
    pub(crate) fn uint(marker: &'static str, value: u32) -> Self {
        Self {
            marker,
            value: format!("{value}u"),
        }
    }

    pub(crate) fn float(marker: &'static str, value: f32) -> Self {
        assert!(value.is_finite());
        Self {
            marker,
            value: format!("{value:?}"),
        }
    }
}

/// The constants [`PRELUDE`] declares, substituted into every shader.
fn prelude_constants() -> [ShaderConstant; 1] {
    [ShaderConstant::float("AA_HALF_WIDTH", AA_HALF_WIDTH)]
}

fn specialize_source(body: &str, constants: &[ShaderConstant]) -> String {
    let mut specialized = format!("{PRELUDE}{body}");
    for constant in prelude_constants().iter().chain(constants) {
        let marker = format!("/*{{{}}}*/", constant.marker);
        assert_eq!(
            specialized.matches(&marker).count(),
            1,
            "WGSL marker {marker} must occur exactly once",
        );
        specialized = specialized.replace(&marker, &constant.value);
    }
    assert!(
        !specialized.contains("/*{"),
        "WGSL template contains an unsubstituted constant marker",
    );
    specialized
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::pipeline::shader_body::ShaderBody;
    use crate::gpu::test_gpu::headless_test_gpu;
    use bytemuck::Pod;
    use naga::front::wgsl;
    use naga::proc::Layouter;
    use std::iter;
    use std::sync::mpsc;
    use wgpu::util::DeviceExt;

    impl ShaderBody {
        /// Run `probe`, WGSL appended to this body, on the test GPU: its compute entry point
        /// `probe` runs once per input, at `global_invocation_id.x`, and reads `array<I>` at
        /// `@group(0) @binding(14)` and writes one `O` per input to `array<O>` at
        /// `@group(0) @binding(15)`, so the body's functions run on the GPU as they draw.
        pub(crate) fn probe<I: Pod, O: Pod>(self, probe: &str, inputs: &[I]) -> Vec<O> {
            assert!(!inputs.is_empty(), "a probe runs on at least one input");
            let gpu = headless_test_gpu();
            let device = &gpu.device;
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(self.label),
                source: wgpu::ShaderSource::Wgsl((self.specialize() + probe).into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("test.probe"),
                layout: None,
                module: &module,
                entry_point: Some("probe"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            });
            let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("test.probe.in"),
                contents: bytemuck::cast_slice(inputs),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let size = inputs.len() * size_of::<O>();
            let output = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("test.probe.out"),
                size: size as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("test.probe.read"),
                size: size as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("test.probe.bg"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 14,
                        resource: input.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 15,
                        resource: output.as_entire_binding(),
                    },
                ],
            });
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(u32::try_from(inputs.len()).unwrap(), 1, 1);
            }
            encoder.copy_buffer_to_buffer(&output, 0, &read, 0, None);
            gpu.queue.submit(iter::once(encoder.finish()));
            let slice = read.slice(..);
            let (tx, rx) = mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |mapped| {
                tx.send(mapped).ok();
            });
            gpu.wait();
            rx.recv().unwrap().unwrap();
            bytemuck::cast_slice(&slice.get_mapped_range().unwrap()).to_vec()
        }

        /// The specialized source, parsed as wgpu parses it.
        pub(crate) fn parse(self) -> naga::Module {
            wgsl::parse_str(&self.specialize())
                .unwrap_or_else(|err| panic!("{} parses: {err}", self.label))
        }

        /// Assert that struct `name` of this shader, laid out as naga lays it out for wgpu, is
        /// `size` bytes with `members` in order, each a WGSL member's name and the offset of the
        /// Rust field that mirrors it.
        #[track_caller]
        pub(crate) fn assert_struct_layout(
            self,
            name: &str,
            size: usize,
            members: &[(&str, usize)],
        ) {
            let label = self.label;
            let module = self.parse();
            let mut layouter = Layouter::default();
            layouter
                .update(module.to_ctx())
                .unwrap_or_else(|err| panic!("{label} lays out: {err}"));
            let (handle, ty) = module
                .types
                .iter()
                .find(|(_, ty)| ty.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("{label} declares struct {name}"));
            let naga::TypeInner::Struct {
                members: declared, ..
            } = &ty.inner
            else {
                panic!("{name} is a struct");
            };
            let declared: Vec<(&str, usize)> = declared
                .iter()
                .map(|m| {
                    (
                        m.name.as_deref().expect("a named member"),
                        m.offset as usize,
                    )
                })
                .collect();
            assert_eq!(
                declared, members,
                "{name}'s members against its Rust mirror"
            );
            assert_eq!(
                layouter[handle].size as usize, size,
                "{name}'s size against its Rust mirror"
            );
        }
    }
}

#[cfg(test)]
mod tests;
