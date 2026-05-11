// Re-import `palantir` as a self-alias so proc-macros that emit
// `::palantir::Animatable` paths (from `palantir-anim-derive`) resolve
// when the derive is used *inside* the crate (e.g. on `Stroke`,
// `Background`). Outside the crate this path resolves naturally.
extern crate self as palantir;

pub(crate) mod animation;
pub(crate) mod common;
pub(crate) mod forest;
pub(crate) mod input;
pub(crate) mod layout;
pub(crate) mod primitives;
pub(crate) mod renderer;
pub(crate) mod shape;
#[cfg(any(test, feature = "internals"))]
pub mod support;
pub(crate) mod text;
pub(crate) mod ui;
pub(crate) mod widgets;

pub use animation::animatable::Animatable;
pub use animation::easing::Easing;
pub use animation::{AnimSlot, AnimSpec};
// Same-name re-export: the derive lives in the macro namespace,
// the trait in the type namespace — `use palantir::Animatable;` pulls
// both, and `#[derive(Animatable)]` works alongside `T: Animatable`.
pub use forest::element::{Configure, Element, LayoutMode};
pub use forest::tree::Layer;
pub use forest::visibility::Visibility;
pub use forest::widget_id::WidgetId;
pub use input::keyboard::{Key, KeyPress, Modifiers, TextChunk};
pub use input::sense::Sense;
pub use input::{FocusPolicy, InputEvent, PointerButton, ResponseState};
pub use layout::types::align::{Align, HAlign, VAlign};
pub use layout::types::clip_mode::ClipMode;
pub use layout::types::display::Display;
pub use layout::types::grid_cell::GridCell;
pub use layout::types::justify::Justify;
pub use layout::types::sizing::{Sizes, Sizing};
pub use layout::types::track::Track;
pub use palantir_anim_derive::Animatable;
pub use primitives::color::Color;
pub use primitives::corners::Corners;
pub use primitives::mesh::{Mesh, MeshVertex};
pub use primitives::rect::Rect;
pub use primitives::size::Size;
pub use primitives::spacing::Spacing;
pub use primitives::stroke::Stroke;
pub use primitives::transform::TranslateScale;
pub use renderer::backend::WgpuBackend;
pub use renderer::frontend::FrameOutput;
pub use shape::{LineCap, LineJoin, PolylineColors, Shape, TextWrap};
pub use text::TextShaper;
pub use text::cosmic::CosmicMeasure;
pub use ui::Ui;
pub use ui::debug_overlay::DebugOverlayConfig;
pub use widgets::Response;
pub use widgets::button::Button;
pub use widgets::frame::Frame;
pub use widgets::grid::Grid;
pub use widgets::panel::Panel;
pub use widgets::popup::{ClickOutside, Popup};
pub use widgets::scroll::Scroll;
pub use widgets::text::Text;
pub use widgets::text_edit::TextEdit;
pub use widgets::theme::{
    AnimatedLook, Background, ButtonTheme, ScrollbarTheme, TextEditTheme, TextStyle, Theme,
    WidgetLook,
};
