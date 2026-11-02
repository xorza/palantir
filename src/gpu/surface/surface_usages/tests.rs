use crate::gpu::surface::surface_usages::SurfaceUsages;

#[test]
fn maps_to_wgpu_both_ways_and_displays_wgpu_names() {
    assert_eq!(
        SurfaceUsages::ALL.to_wgpu(),
        wgpu::TextureUsages::RENDER_ATTACHMENT,
        "every declared flag maps"
    );
    assert_eq!(SurfaceUsages::NONE.to_wgpu(), wgpu::TextureUsages::empty());
    assert_eq!(
        SurfaceUsages::from_wgpu_truncate(
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST
        ),
        SurfaceUsages::RENDER_ATTACHMENT,
        "a usage Palantir does not require drops"
    );
    assert_eq!(
        SurfaceUsages::from_wgpu_truncate(wgpu::TextureUsages::COPY_DST),
        SurfaceUsages::NONE
    );

    assert_eq!(
        SurfaceUsages::RENDER_ATTACHMENT.to_string(),
        "RENDER_ATTACHMENT"
    );
    assert_eq!(SurfaceUsages::NONE.to_string(), "");
}
