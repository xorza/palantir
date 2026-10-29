# Open issues

- **A lost swapchain is reconfigured, not recreated.** `WindowSurface::acquire`
  (`src/gpu/surface/window_surface.rs`) folds `CurrentSurfaceTexture::Lost` into
  `Acquired::Outdated`, and the winit host's next frame calls `configure` on the lost surface,
  which wgpu fails with `ConfigureSurfaceError::InvalidSurface`; no error handler is installed,
  so the uncaptured error panics. wgpu documents `Lost` as needing `Instance::create_surface`.
