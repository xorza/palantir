//! The backend's construction-time switches.

/// What a host opts into when it builds the backend. Separate from
/// [`SharedResources`](crate::renderer::backend::shared_resources::SharedResources)
/// because these are choices, not handles.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BackendConfig {
    pub(crate) collect_gpu_stats: bool,
}
