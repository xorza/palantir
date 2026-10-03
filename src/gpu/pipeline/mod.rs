//! The render pipelines, one directory per pipeline beside its `shader.wgsl`,
//! and the recipe, prelude and shader assembly they share.

pub(crate) mod blit_pipeline;
pub(crate) mod curve_pipeline;
pub(crate) mod format_pipelines;
pub(crate) mod image_pipeline;
pub(crate) mod mesh_pipeline;
pub(crate) mod pipeline_recipe;
pub(crate) mod quad_pipeline;
pub(crate) mod shader_body;
pub(crate) mod stencil_variant;

/// Size of the per-pipeline immediate (push-constant) region every
/// palantir shader reads, through the one `var<immediate> imm:
/// Immediates` in `prelude.wgsl`. Locked at the maximum used by any
/// pipeline so a `set_immediates` for one shader stays valid across
/// pipeline switches:
///
/// - offset 0 (8 bytes): [`ViewportPush`](crate::gpu::surface::viewport::ViewportPush) — viewport size, written
///   once per pass by `WgpuBackend`.
/// - offset 8 (8 bytes): `text::Params` — atlas dimensions, written per
///   raster batch by `RasterPass::render_batch`, because the two raster
///   tenants hold differently sized atlases.
///
/// Pipelines that don't read the tail (quad/mesh/image/curve) still
/// declare `immediate_size = IMMEDIATES_BYTES` so the immediate-state
/// layout matches and bytes written by other pipelines stay valid
/// after a pipeline switch.
///
/// `DeviceRequirements` asks every device for exactly this many bytes and
/// rejects one that grants fewer, so growing the layout raises the request
/// by the same step.
pub(crate) const IMMEDIATES_BYTES: u32 = 16;
