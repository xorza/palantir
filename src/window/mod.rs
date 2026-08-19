//! Backend-agnostic window vocabulary shared by the recorder
//! ([`Ui`](crate::Ui)) and the windowing host
//! ([`WinitHost`](crate::WinitHost)). Both depend *into* this module and
//! neither back out, so the recorder never reaches up into the winit
//! backend — [`WindowRequests`](window_requests::WindowRequests),
//! [`WindowFrameState`](window_frame_state::WindowFrameState), and
//! [`WindowConfig`](window_config::WindowConfig) deliberately carry no
//! winit/wgpu types.

pub(crate) mod cursor_icon;
pub(crate) mod vsync;
pub(crate) mod window_commands;
pub(crate) mod window_config;
pub(crate) mod window_directory;
pub(crate) mod window_frame_state;
pub(crate) mod window_geometry;
pub(crate) mod window_output;
pub(crate) mod window_requests;
pub(crate) mod window_token;
