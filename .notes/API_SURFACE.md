# Public API surface

Every item the crate exports, rendered as its declaration from rustdoc JSON
(`python3 scripts/api_surface.py`), sorted by path. The surface is the union of a
build with no features and one build per public feature; a tag names the features an
item needs. `internals` and `bench` are the crate's own test surface and are left out.

Generated on top of `2c608f01` (plus the working tree).

`prelude` re-exports these root items: `Align`, `App`, `Axis`, `Background`, `Block`, `Brush`, `Button`, `Checkbox`, `ComboBox`, `Configure`, `ContextMenu`, `Corners`, `DragValue`, `Expander`, `Grid`, `GridCell`, `HAlign`, `InnerResponse`, `Justify`, `Key`, `KeyPress`, `MenuItem`, `Modal`, `Modifiers`, `OverlayResponse`, `Panel`, `PointerButton`, `Popup`, `ProgressBar`, `RadioButton`, `Rect`, `Response`, `RgbaF32`, `Scroll`, `Sense`, `Separator`, `Shadow`, `Shortcut`, `Size`, `SizeSpec`, `Sizing`, `Slider`, `Spacing`, `Spinner`, `Splitter`, `Stroke`, `Switch`, `TabbedView`, `Text`, `TextEdit`, `TextStyle`, `Theme`, `Tooltip`, `Track`, `UVec2`, `Ui`, `VAlign`, `ValueResponse`, `Vec2`, `WidgetId`, `WindowToken`, `fmt`.

```text
Align
    struct Align
        (_)
        pub const fn new(h: HAlign, v: VAlign) -> Self
        pub const fn h(h: HAlign) -> Self
        pub const fn v(v: VAlign) -> Self
        pub const fn halign(self) -> HAlign
        pub const fn valign(self) -> VAlign
        pub const TOP_LEFT: Self
        pub const TOP: Self
        pub const TOP_RIGHT: Self
        pub const LEFT: Self
        pub const CENTER: Self
        pub const RIGHT: Self
        pub const BOTTOM_LEFT: Self
        pub const BOTTOM: Self
        pub const BOTTOM_RIGHT: Self
        pub const STRETCH: Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

AllowedSplits
    enum AllowedSplits
        All
        Row
        Column
        None
        pub fn allows(self, side: SplitSide) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

Anchor
    struct Anchor
        // and private fields
        pub const fn at_point(point: Vec2) -> Self
        pub const fn above(rect: Rect) -> Self
        pub const fn below(rect: Rect) -> Self
        pub const fn left_of(rect: Rect) -> Self
        pub const fn right_of(rect: Rect) -> Self
        pub const fn with_align(self, align: AnchorAlign) -> Self
        pub const fn with_gap(self, px: f32) -> Self
        impl Clone
        impl Copy
        impl Debug

AnchorAlign
    enum AnchorAlign
        Start
        Center
        End
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

AnimatedLook
    struct AnimatedLook
        pub background: Background
        pub text: TextStyle
        impl Animatable
        impl Clone
        impl Debug
        impl Default
        impl PartialEq

AnimationSpec
    struct AnimationSpec
        // and private fields
        pub const FAST: Self
        pub const MEDIUM: Self
        pub const SNAP: Self
        pub const SPRING: Self
        pub const fn duration(length: Duration, ease: Easing) -> Self
        pub fn spring(stiffness: f32, damping: f32) -> Self
        pub const fn is_instant(self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for AnimationSpec

App
    trait App
        fn update(&mut self, _window: WindowToken, _ui: &Ui) { .. }
        fn record(&mut self, window: WindowToken, ui: &mut Ui)

Axis
    enum Axis
        X
        Y
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

Background
    struct Background
        pub fill: Brush
        pub border: Stroke
        pub corners: Corners
        pub shadow: Shadow
        pub const NONE: Self
        pub const fn is_noop(&self) -> bool
        pub fn fill<I: Into<Brush>>(brush: I) -> Self
        pub fn rounded<I: Into<Brush>>(brush: I, corners: Corners) -> Self
        pub const fn with_border(self, border: Stroke) -> Self
        pub const fn with_shadow(self, shadow: Shadow) -> Self
        impl Animatable
        impl Clone
        impl Debug
        impl Default
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for Background

BarMode
    enum BarMode
        Reserved
        Overlay
        Hidden
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

BatchKind
    enum BatchKind
        Setup = 0
        PreClear = 1
        Mask = 2
        Quads = 3
        Shadows = 4
        Text = 5
        Mesh = 6
        Image = 7
        Curve = 8
        Icon = 9
        pub const COUNT: usize
        pub const ALL: [Self; 10]
        pub const fn label(self) -> &'static str
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

Block
    struct Block
        // and private fields
        pub fn new() -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        pub const fn background(self, background: Background) -> Self
        pub const fn default_background(self, background: Background) -> Self
        impl Configure
        impl Debug
        impl ThemeDefaults  (blanket)

Brush
    enum Brush
        Solid(RgbaF32)
        Linear(LinearGradient)
        Radial(RadialGradient)
        Conic(ConicGradient)
        pub const TRANSPARENT: Self
        pub const fn is_noop(&self) -> bool
        pub const fn as_solid(&self) -> Option<RgbaF32>
        impl Animatable
        impl Clone
        impl Debug
        impl Default
        impl From<Gradient<ConicGeometry>>
        impl From<Gradient<LinearGeometry>>
        impl From<Gradient<RadialGeometry>>
        impl From<GradientBuilder<ConicGeometry>>
        impl From<GradientBuilder<LinearGeometry>>
        impl From<GradientBuilder<RadialGeometry>>
        impl From<RgbaF32>
        impl From<SrgbaU8>
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for Brush

Button
    struct Button<'a>
        // and private fields
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a ButtonTheme>>) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub const fn text_wrap(self, wrap: TextWrap) -> Self
        pub const fn text_align(self, a: Align) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Button<'a>

ButtonPhase
    enum ButtonPhase
        Idle
        Down { count: u8 }
        Held
        Up { click: Option<u8> }
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

ButtonState
    struct ButtonState
        pub phase: ButtonPhase
        pub drag: Drag
        pub const fn held(self) -> bool
        pub const fn clicked(self) -> bool
        pub const fn released(self) -> bool
        pub const fn press_count(self) -> u8
        pub const fn click_count(self) -> u8
        pub const fn double_clicked(self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

ButtonTheme
    struct ButtonTheme
        pub looks: StatefulLook
        pub defaults: SlotDefaults
        pub fn from_palette(p: &Palette) -> Self
        pub fn menu_button(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl ThemeSlot
        impl<'de> Deserialize<'de> for ButtonTheme

Checkbox
    struct Checkbox<'a>
        // and private fields
        pub fn new(value: &'a mut bool) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ToggleTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Checkbox<'a>

ClickOutside
    enum ClickOutside
        Block
        Dismiss
        PassThrough
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

ClipMode
    enum ClipMode
        None = 0
        Rect = 1
        Rounded = 2
        pub const fn is_clip(self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl Ord
        impl PartialEq
        impl PartialOrd
        impl Serialize
        impl<'de> Deserialize<'de> for ClipMode

Clipboard
    struct Clipboard
        // and private fields
        pub fn text(&self) -> Result<String, ClipboardUnavailable>
        pub fn set_text(&self, text: &str) -> Result<(), ClipboardUnavailable>
        impl Clone
        impl Debug

ClipboardUnavailable
    struct ClipboardUnavailable
        impl Clone
        impl Copy
        impl Debug
        impl Display
        impl Eq
        impl Error
        impl PartialEq

Clock
    trait Clock: Debug
        fn now(&self) -> Duration
        fn skip(&mut self, hidden: Duration) { .. }
        fn deadline(&self, at: Duration) -> Option<Instant> { .. }

CloseHandle
    struct CloseHandle
        // and private fields
        pub fn close(&self)
        pub const fn requested(&self) -> bool
        impl Debug
        impl Default

ColorButton
    struct ColorButton<'a>
        // and private fields
        pub fn new(picker: ColorPicker<'a>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for ColorButton<'a>

ColorCoords
    enum ColorCoords
        Okhsv(Okhsv)
        Hsv(Hsv)
        pub fn new(model: ColorModel, color: RgbaF32, fallback_hue: f32) -> Self
        pub const fn model(self) -> ColorModel
        pub fn to_color(self) -> RgbaF32
        pub fn with_model(self, model: ColorModel) -> Self
        pub const fn hue(self) -> f32
        pub const fn saturation(self) -> f32
        pub const fn value(self) -> f32
        pub const fn set_hue(&mut self, h: f32)
        pub const fn set_saturation(&mut self, s: f32)
        pub const fn set_value(&mut self, v: f32)
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

ColorField
    struct ColorField<'a>
        // and private fields
        pub fn new(coords: &'a mut ColorCoords) -> Self
        pub const fn texel_size(self, n: u32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for ColorField<'a>

ColorModel
    enum ColorModel
        Okhsv
        Hsv
        pub const ALL: [Self; 2]
        pub const fn label(self) -> &'static str
        pub fn slice(self, hue: f32) -> HueSlice
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for ColorModel

ColorPicker
    struct ColorPicker<'a>
        // and private fields
        pub fn new(color: &'a mut RgbaF32) -> Self
        pub const fn alpha(self, on: bool) -> Self
        pub const fn model(self, model: ColorModel) -> Self
        pub const fn history(self, on: bool) -> Self
        pub const fn swatches(self, colors: &'a [RgbaF32]) -> Self
        pub const fn texel_size(self, n: u32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub const fn color(&self) -> RgbaF32
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for ColorPicker<'a>

ColorPickerTheme
    struct ColorPickerTheme
        pub field_width: f32
        pub field_height: f32
        pub bar_thickness: f32
        pub chip_size: f32
        pub swatch_size: f32
        pub handle_radius: f32
        pub handle_width: f32
        pub handle_outer: RgbaF32
        pub handle_inner: RgbaF32
        pub checker_light: RgbaF32
        pub checker_dark: RgbaF32
        pub checker_cell: f32
        pub border: RgbaF32
        pub border_width: f32
        pub gap: f32
        pub popup: Background
        pub popup_padding: Spacing
        pub value: DragValueTheme
        pub hex: TextEditTheme
        pub label: TextStyleOverrides
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for ColorPickerTheme

ColorRamp
    struct ColorRamp
        pub stops: GradientStops
        pub interpolation: Interpolation
        pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self
        pub fn two_stop(c0: RgbaF32, c1: RgbaF32) -> Self
        pub const fn with_interpolation(self, interpolation: Interpolation) -> Self
        pub const fn is_noop(&self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for ColorRamp

ColorStrip
    struct ColorStrip<'a>
        // and private fields
        pub fn for_hue(coords: &'a mut ColorCoords) -> Self
        pub fn for_alpha(color: &'a mut RgbaF32) -> Self
        pub const fn texel_size(self, n: u32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for ColorStrip<'a>

ColorSwatch
    struct ColorSwatch<'a>
        // and private fields
        pub fn new(color: RgbaF32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for ColorSwatch<'a>

ComboBox
    struct ComboBox<'a, S, L>
        // and private fields
        impl<'a, S: AsRef<str>> ComboBox<'a, S, fn(&S) -> &str>
            pub fn new(selected: &'a mut usize, options: &'a [S]) -> Self
        impl<'a, S, L: Fn(&S) -> &str> ComboBox<'a, S, L>
            pub fn labeled(selected: &'a mut usize, options: &'a [S], label: L) -> Self
            pub fn style(self, s: impl Into<Option<&'a ComboBoxTheme>>) -> Self
            pub fn button_style(self, s: impl Into<Option<&'a ButtonTheme>>) -> Self
            pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl ThemeDefaults  (blanket)
        impl<'a, S: Debug, L: Debug> Debug for ComboBox<'a, S, L>
        impl<S, L> Configure for ComboBox<'_, S, L>

ComboBoxTheme
    struct ComboBoxTheme
        pub gap: f32
        pub arrow_size: Vec2
        pub arrow_width: f32
        pub const fn from_palette(_p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for ComboBoxTheme

Configure
    trait Configure: Sized
        fn configure(&mut self) -> ConfigureWidget<'_>
        fn id_salt(self, key: impl Hash) -> Self { .. }
        fn id(self, id: WidgetId) -> Self { .. }
        fn auto_id(self) -> Self { .. }
        fn size(self, s: impl Into<SizeSpec>) -> Self { .. }
        fn min_size(self, s: impl Into<Size>) -> Self { .. }
        fn max_size(self, s: impl Into<Size>) -> Self { .. }
        fn padding(self, p: impl Into<Spacing>) -> Self { .. }
        fn margin(self, m: impl Into<Spacing>) -> Self { .. }
        fn transform(self, t: TranslateScale) -> Self { .. }
        fn position(self, p: impl Into<Vec2>) -> Self { .. }
        fn grid_cell(self, cell: impl Into<GridCell>) -> Self { .. }
        fn adopt_placement(self, from: &Widget) -> Self { .. }
        fn gap(self, g: f32) -> Self { .. }
        fn line_gap(self, g: f32) -> Self { .. }
        fn justify(self, j: Justify) -> Self { .. }
        fn align(self, a: Align) -> Self { .. }
        fn child_align(self, a: Align) -> Self { .. }
        fn sense(self, s: Sense) -> Self { .. }
        fn add_sense(self, s: Sense) -> Self { .. }
        fn disabled(self, d: bool) -> Self { .. }
        fn focusable(self, f: bool) -> Self { .. }
        fn tab_stop(self, stop: bool) -> Self { .. }
        fn arrow_focus(self, axis: Axis) -> Self { .. }
        fn tab_index(self, index: i16) -> Self { .. }
        fn input_scope(self, takes: KeyFilter) -> Self { .. }
        fn visibility(self, v: Visibility) -> Self { .. }
        fn hidden(self) -> Self { .. }
        fn collapsed(self) -> Self { .. }
        fn clip(self, mode: ClipMode) -> Self { .. }
        fn clip_rect(self) -> Self { .. }
        fn clip_rounded(self) -> Self { .. }

ConicGeometry
    struct ConicGeometry
        pub center: Vec2
        pub start_angle: f32
        impl Clone
        impl Copy
        impl Debug
        impl GradientGeometry  (blanket)
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for ConicGeometry

ConicGradient
    type ConicGradient = Gradient<ConicGeometry>

ConicGradientBuilder
    type ConicGradientBuilder = GradientBuilder<ConicGeometry>

ContextMenu
    struct ContextMenu<'a>
        // and private fields
        pub fn for_id(for_id: WidgetId) -> Self
        pub fn on(snapshot: &ResponseSnapshot) -> Self
        pub fn style(self, s: impl Into<Option<&'a ContextMenuTheme>>) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &CloseHandle) -> R) -> OverlayResponse<Option<R>>
        pub fn open(ui: &mut Ui, for_id: WidgetId, point: Vec2)
        pub fn close(ui: &mut Ui, for_id: WidgetId)
        pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool
        pub const fn background(self, background: Background) -> Self
        pub const fn default_background(self, background: Background) -> Self
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for ContextMenu<'a>

ContextMenuTheme
    struct ContextMenuTheme
        pub panel: Background
        pub padding: Spacing
        pub min_width: f32
        pub gap: f32
        pub item: MenuItemTheme
        pub separator: SeparatorTheme
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for ContextMenuTheme

Corners
    struct Corners
        (_)
        pub const ZERO: Self
        pub fn as_array(self) -> [f32; 4]
        pub fn all(r: f32) -> Self
        pub fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self
        pub fn top(r: f32) -> Self
        pub fn bottom(r: f32) -> Self
        pub fn left(r: f32) -> Self
        pub fn right(r: f32) -> Self
        pub fn scaled_by(self, factor: f32) -> Self
        pub const fn is_approx_zero(&self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Pod
        impl Serialize
        impl Zeroable
        impl<'de> Deserialize<'de> for Corners
        impl<T: Num, B: Num> From<(T, B)> for Corners
        impl<T: Num> From<T> for Corners
        impl<TL: Num, TR: Num, BR: Num, BL: Num> From<(TL, TR, BR, BL)> for Corners

CursorIcon
    enum CursorIcon
        Default
        Pointer
        Text
        Grab
        Grabbing
        Move
        Crosshair
        EwResize
        NsResize
        NotAllowed
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

DebugOverlayConfig
    struct DebugOverlayConfig
        pub damage_rect: bool
        pub dim_undamaged: bool
        pub frame_stats: bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

DeviceRequirements
    struct DeviceRequirements
        pub features: Features
        pub limits: Limits
        pub const FEATURES: Features
        pub const GPU_TIMING_FEATURES: Features
        pub fn negotiate(adapter: &Adapter, optional: Features) -> Result<Self, UnmetRequirements>
        pub fn met_by(device: &Device) -> Result<(), UnmetRequirements>
        impl Clone
        impl Debug

Display
    struct Display
        pub physical: UVec2
        pub system_scale: f32
        pub user_scale: UserScale
        pub pixel_snap: bool
        pub refresh_millihertz: Option<u32>
        pub const fn from_physical(physical: UVec2, system_scale: f32) -> Self
        pub const fn scale_factor(&self) -> f32
        pub fn logical_size(&self) -> Size
        pub fn system_logical_size(&self) -> Size
        pub fn logical_rect(&self) -> Rect
        pub fn raster_eq(&self, other: &Display) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

DockDrop
    enum DockDrop
        Into { group: TabGroupId, index: usize }
        Split { group: TabGroupId, side: SplitSide }
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for DockDrop

DockError
    enum DockError<T>
        NonCanonical
        NodeOutOfRange { index: u32 }
        SplitNesting
        SplitRatio { ratio: f32 }
        UnreachableSlots
        MissingPinnedTab
        DuplicateGroup { group: TabGroupId }
        EmptyGroup { group: TabGroupId }
        ActiveTabOutOfRange { group: TabGroupId }
        DuplicateTab { tab: T }
        MissingFocusedGroup { group: TabGroupId }
        GroupAllocator { next_group: u64 }
        impl<T: Clone> Clone for DockError<T>
        impl<T: Copy> Copy for DockError<T>
        impl<T: Debug> Debug for DockError<T>
        impl<T: Debug> Display for DockError<T>
        impl<T: Debug> Error for DockError<T>
        impl<T: PartialEq> PartialEq for DockError<T>

DockNode
    enum DockNode<T>
        Split(DockSplit)
        Group(TabGroup<T>)
        impl<'de, T> Deserialize<'de> for DockNode<T> where T: Deserialize<'de>
        impl<T: Clone> Clone for DockNode<T>
        impl<T: Debug> Debug for DockNode<T>
        impl<T: PartialEq> PartialEq for DockNode<T>
        impl<T> Serialize for DockNode<T> where T: Serialize

DockOperation
    enum DockOperation<T>
        ActivateTab { tab: T }
        OpenTab { tab: T }
        CloseTab { tab: T }
        MoveTab { tab: T, to: DockDrop }
        SetRatio { split: DockPath, ratio: f32 }
        FocusPane { group: TabGroupId }
        impl<'de, T> Deserialize<'de> for DockOperation<T> where T: Deserialize<'de>
        impl<T: Clone> Clone for DockOperation<T>
        impl<T: Copy> Copy for DockOperation<T>
        impl<T: Debug> Debug for DockOperation<T>
        impl<T: PartialEq> PartialEq for DockOperation<T>
        impl<T> Serialize for DockOperation<T> where T: Serialize

DockPath
    struct DockPath
        (_)
        pub const ROOT: DockPath
        pub fn first(self) -> DockPath
        pub fn second(self) -> DockPath
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for DockPath

DockSplit
    struct DockSplit
        // and private fields
        pub const fn direction(self) -> SplitDirection
        pub const fn ratio(self) -> f32
        pub const fn first(self) -> NodeIndex
        pub const fn second(self) -> NodeIndex
        impl Clone
        impl Copy
        impl Debug
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for DockSplit

DockState
    struct DockState<T>
        // and private fields
        impl<T: DockTab> DockState<T>
            pub const ROOT: NodeIndex
            pub const RATIO_MIN: f32
            pub const RATIO_MAX: f32
            pub fn new(seed: impl Hash, pinned: T) -> Self
            pub fn with_max_depth(self, depth: u32) -> Self
            pub const fn with_allowed_splits(self, allowed: AllowedSplits) -> Self
            pub const fn pinned(&self) -> T
            pub const fn seed(&self) -> u64
            pub const fn allowed_splits(&self) -> AllowedSplits
            pub const fn focused(&self) -> TabGroupId
            pub fn node(&self, index: NodeIndex) -> &DockNode<T>
            pub fn groups(&self) -> impl Iterator<Item = &TabGroup<T>>
            pub fn all_tabs(&self) -> impl Iterator<Item = T> + '_
            pub fn active_tabs(&self) -> impl Iterator<Item = T> + '_
            pub fn primary(&self) -> &TabGroup<T>
            pub fn find_tab(&self, tab: T) -> Option<TabAddress>
            pub fn group(&self, id: TabGroupId) -> Option<&TabGroup<T>>
            pub fn apply(&mut self, operation: DockOperation<T>)
            pub fn find_or_insert(&mut self, tab: T, group: TabGroupId)
            pub fn retain_tabs(&mut self, keep: impl FnMut(T) -> bool)
            pub fn can_split(&self, group: TabGroupId) -> bool
        impl<'de, T> Deserialize<'de> for DockState<T> where T: DockTab + Deserialize<'de>
        impl<T: Clone> Clone for DockState<T>
        impl<T: Debug> Debug for DockState<T>
        impl<T: PartialEq> PartialEq for DockState<T>
        impl<T> Serialize for DockState<T> where T: Serialize

DockTab
    trait DockTab: Copy + Eq + Hash + Debug + 'static
        impl<T: Copy + Eq + Hash + Debug + 'static> DockTab for T

DockTabMenu
    struct DockTabMenu<'a, T>
        pub tab: T
        pub group: TabGroupId
        pub operations: &'a mut Vec<DockOperation<T>>
        pub close: &'a CloseHandle
        impl<'a, T: Debug> Debug for DockTabMenu<'a, T>

DockTabs
    trait DockTabs
        type Tab: DockTab
        fn title(&mut self, ui: &mut Ui, tab: Self::Tab) -> InternedStr
        fn content(&mut self, ui: &mut Ui, tab: Self::Tab, size: Option<Size>)
        fn closable(&mut self, _tab: Self::Tab) -> bool { .. }
        fn draggable(&mut self, _tab: Self::Tab) -> bool { .. }
        fn badge(&mut self, _tab: Self::Tab) -> TabBadge { .. }
        fn icon(&mut self, _tab: Self::Tab) -> Option<IconHandle> { .. }
        fn tab_menu(&mut self, _ui: &mut Ui, _menu: DockTabMenu<'_, Self::Tab>) { .. }

DockTheme
    struct DockTheme
        pub preview_fill: RgbaF32
        pub preview_stroke: Stroke
        pub preview_radius: f32
        pub caret_width: f32
        pub ghost: WidgetLook
        pub ghost_padding: Spacing
        pub ghost_offset: Vec2
        pub edge_fraction: f32
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for DockTheme

DockView
    struct DockView<'a, T>
        // and private fields
        impl<'a, T: DockTab> DockView<'a, T>
            pub fn new(state: &'a DockState<T>, operations: &'a mut Vec<DockOperation<T>>) -> Self
            pub const fn min_pane(self, px: f32) -> Self
            pub const fn overflow(self, overflow: TabOverflow) -> Self
            pub fn style(self, s: impl Into<Option<&'a DockTheme>>) -> Self
            pub fn show<'u, D: DockTabs<Tab = T>>(self, ui: &'u mut Ui, tabs: &mut D) -> Response<'u>
        impl<T: DockTab> DockView<'_, T>
            pub fn run<D: DockTabs<Tab = T>>(ui: &mut Ui, state: &mut DockState<T>, tabs: &mut D)
            pub fn dock_id(state: &DockState<T>) -> WidgetId
            pub fn pane_id(state: &DockState<T>, group: TabGroupId) -> WidgetId
            pub fn content_id(state: &DockState<T>, group: TabGroupId) -> WidgetId
            pub fn strip_id(state: &DockState<T>, group: TabGroupId) -> WidgetId
            pub fn splitter_id(state: &DockState<T>, path: DockPath) -> WidgetId
            pub fn tab_key(tab: T) -> u64
            pub fn scan(ui: &mut Ui, state: &DockState<T>, operations: &mut Vec<DockOperation<T>>)
            pub fn content_size(ui: &Ui, state: &DockState<T>, group: TabGroupId) -> Option<Size>
        impl ThemeDefaults  (blanket)
        impl<'a, T: Debug> Debug for DockView<'a, T>
        impl<T> Configure for DockView<'_, T>

Drag
    enum Drag
        None
        Started { delta: Vec2 }
        Active { delta: Vec2 }
        Stopped
        pub const fn delta(self) -> Option<Vec2>
        pub const fn is_live(self) -> bool
        pub const fn started(self) -> bool
        pub const fn stopped(self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

DragNum
    enum DragNum<'a>
        I64(&'a mut i64)
        F64(&'a mut f64)
        impl<'a> Debug for DragNum<'a>
        impl<'a> From<&'a mut f64> for DragNum<'a>
        impl<'a> From<&'a mut i64> for DragNum<'a>

DragValue
    struct DragValue<'a>
        // and private fields
        pub fn new(value: impl Into<DragNum<'a>>) -> Self
        pub const fn speed(self, speed: f64) -> Self
        pub const fn range(self, range: RangeInclusive<f64>) -> Self
        pub const fn decimals(self, n: usize) -> Self
        pub fn suffix(self, text: impl Into<TextInput<'a>>) -> Self
        pub const fn editable(self, on: bool) -> Self
        pub fn style(self, s: impl Into<Option<&'a DragValueTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for DragValue<'a>

DragValueTheme
    struct DragValueTheme
        pub chip: ButtonTheme
        pub editor: TextEditTheme
        pub fn from_chip(chip: ButtonTheme, text_edit: &TextEditTheme) -> Self
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for DragValueTheme

DriverError
    struct DriverError
        (_)
        impl Debug
        impl Display
        impl Error

Easing
    enum Easing
        Linear
        OutCubic
        InOutCubic
        OutQuart
        OutBack
        pub const fn apply(self, t: f32) -> f32
        impl Clone
        impl Copy
        impl Debug
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for Easing

Expander
    struct Expander<'a>
        // and private fields
        pub fn new(label: impl Into<TextInput<'a>>) -> Self
        pub const fn start_open(self, open: bool) -> Self
        pub const fn open(self, open: &'a mut bool) -> Self
        pub const fn keep_body(self, keep: bool) -> Self
        pub fn style(self, s: impl Into<Option<&'a ExpanderTheme>>) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> ExpanderResponse<'_, R>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Expander<'a>

ExpanderResponse
    struct ExpanderResponse<'a, R>
        pub response: Response<'a>
        pub inner: Option<R>
        pub changed: bool
        pub openness: f32
        impl<'a, R: Debug> Debug for ExpanderResponse<'a, R>

ExpanderTheme
    struct ExpanderTheme
        pub looks: StatefulLook
        pub arrow_size: Vec2
        pub arrow_radius: f32
        pub arrow_closed_angle: f32
        pub arrow_open_angle: f32
        pub gap: f32
        pub indent: f32
        pub body_padding: Spacing
        pub defaults: SlotDefaults
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl ThemeSlot
        impl<'de> Deserialize<'de> for ExpanderTheme

FixedClock
    struct FixedClock
        // and private fields
        pub const fn new(now: Duration) -> Self
        pub fn advance(&mut self, dt: Duration)
        impl Clock
        impl Debug
        impl Default

FocusPolicy
    enum FocusPolicy
        PreserveOnMiss
        ClearOnMiss
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

FocusRingTheme
    struct FocusRingTheme
        pub color: RgbaF32
        pub width: f32
        pub const fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for FocusRingTheme

FontFamily
    struct FontFamily
        (_)
        pub const SANS: Self
        pub const MONO: Self
        pub fn named(name: &str) -> Option<Self>
        pub fn name(self) -> &'static str
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for FontFamily

FontLoadError
    enum FontLoadError
        Io { path: PathBuf, source: Error }
        NoFaces
        FamilyTableFull
        impl Debug
        impl Display
        impl Error

FontScope
    enum FontScope
        Bundled
        System
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

FontSlant
    enum FontSlant
        Normal = 0
        Italic = 1
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for FontSlant

FontSource
    enum FontSource
        Bytes(Cow<'static, [u8]>)
        File(PathBuf)
        impl Clone
        impl Debug
        impl From<&'static [u8]>
        impl From<&Path>
        impl From<&str>
        impl From<Cow<'static, [u8]>>
        impl From<PathBuf>
        impl From<Vec<u8>>
        impl<const N: usize> From<&'static [u8; N]> for FontSource

FontWeight
    struct FontWeight
        (_)
        pub const THIN: Self
        pub const EXTRA_LIGHT: Self
        pub const LIGHT: Self
        pub const REGULAR: Self
        pub const MEDIUM: Self
        pub const SEMI_BOLD: Self
        pub const BOLD: Self
        pub const EXTRA_BOLD: Self
        pub const BLACK: Self
        pub const fn new(weight: u16) -> Self
        pub const fn get(self) -> u16
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl Ord
        impl PartialEq
        impl PartialOrd
        impl Serialize
        impl<'de> Deserialize<'de> for FontWeight

FramePaint
    enum FramePaint
        Skip
        Full
        Partial
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

FrameReport
    struct FrameReport
        pub repaint_requested: bool
        pub repaint_after: Option<Duration>
        pub ime_area: Option<Rect>
        // and private fields
        pub const fn paint(&self) -> FramePaint
        impl Debug

Gpu
    struct Gpu
        // and private fields
        pub const fn new(device: Device, queue: Queue) -> Self
        impl Clone
        impl Debug

GpuFrameContext
    struct GpuFrameContext<'a>
        pub device: &'a Device
        pub queue: &'a Queue
        pub encoder: &'a mut CommandEncoder
        pub target: &'a TextureView
        pub physical_size: UVec2
        pub physical_full_size: UVec2
        pub physical_offset: UVec2
        pub display_scale: f32
        pub raster_scale: f32
        pub dt: Duration
        impl Debug

GpuInitContext
    struct GpuInitContext<'a>
        pub device: &'a Device
        pub target_format: TextureFormat
        pub text: &'a TextShaper
        impl<'a> Debug for GpuInitContext<'a>

GpuPaint
    trait GpuPaint: 'static
        fn init(&mut self, context: &GpuInitContext<'_>) { .. }
        fn paint(&mut self, context: &mut GpuFrameContext<'_>)

GpuPassStats
    struct GpuPassStats
        // and private fields
        pub fn last_pass(&self) -> Option<Duration>
        pub fn last_kind(&self, kind: BatchKind) -> Option<Duration>
        pub fn last_pipeline_stats(&self) -> Option<PipelineStats>
        pub fn last_main_pass_cpu(&self) -> Option<Duration>
        impl Clone
        impl Debug
        impl Default

GpuRequestError  [shape differs by feature]
    #[non_exhaustive]
    enum GpuRequestError
        NoBackend
        RequestAdapter { source: DriverError }
        Requirements { source: UnmetRequirements }
        RequestDevice { source: DriverError }
        impl Debug
        impl Display
        impl Error

GpuView
    struct GpuView
        // and private fields
        pub fn new<T: GpuPaint + 'static>(paint: &Rc<RefCell<T>>) -> Self
        pub const fn repaint(self, repaint: bool) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        impl Configure
        impl Debug
        impl ThemeDefaults  (blanket)

Gradient
    struct Gradient<G>
        pub geometry: G
        pub ramp: ColorRamp
        pub spread: Spread
        impl Gradient<ConicGeometry>
            pub fn builder(center: Vec2, start_angle: f32) -> ConicGradientBuilder
            pub fn new(center: Vec2, start_angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self
            pub fn two_stop(c0: RgbaF32, c1: RgbaF32) -> Self
        impl Gradient<LinearGeometry>
            pub fn builder(angle: f32) -> LinearGradientBuilder
            pub fn new(angle: f32, stops: impl IntoIterator<Item = Stop>) -> Self
            pub fn two_stop(angle: f32, c0: RgbaF32, c1: RgbaF32) -> Self
        impl Gradient<RadialGeometry>
            pub fn builder(center: Vec2, radius: Vec2) -> RadialGradientBuilder
            pub fn new(center: Vec2, radius: Vec2, stops: impl IntoIterator<Item = Stop>) -> Self
            pub fn two_stop(c0: RgbaF32, c1: RgbaF32) -> Self
        pub const fn with_spread(self, spread: Spread) -> Self
        pub const fn with_interpolation(self, interpolation: Interpolation) -> Self
        pub const fn is_noop(&self) -> bool
        impl From<Gradient<ConicGeometry>> for Brush
        impl From<Gradient<LinearGeometry>> for Brush
        impl From<Gradient<RadialGeometry>> for Brush
        impl<'de, G> Deserialize<'de> for Gradient<G> where G: Deserialize<'de>
        impl<G: Clone> Clone for Gradient<G>
        impl<G: Debug> Debug for Gradient<G>
        impl<G: GradientGeometry> From<GradientBuilder<G>> for Gradient<G>
        impl<G: GradientGeometry> Hash for Gradient<G>
        impl<G: PartialEq> PartialEq for Gradient<G>
        impl<G> Serialize for Gradient<G> where G: Serialize

GradientBuilder
    struct GradientBuilder<G>
        // and private fields
        impl<G: GradientGeometry> GradientBuilder<G>
            pub fn stop(self, offset: f32, color: RgbaF32) -> Self
            pub const fn spread(self, spread: Spread) -> Self
            pub const fn interpolation(self, interpolation: Interpolation) -> Self
            pub fn build(self) -> Gradient<G>
        impl From<GradientBuilder<ConicGeometry>> for Brush
        impl From<GradientBuilder<LinearGeometry>> for Brush
        impl From<GradientBuilder<RadialGeometry>> for Brush
        impl<G: Clone> Clone for GradientBuilder<G>
        impl<G: Debug> Debug for GradientBuilder<G>
        impl<G: GradientGeometry> From<GradientBuilder<G>> for Gradient<G>

GradientGeometry
    trait GradientGeometry: Geometry
        impl<T: Geometry> GradientGeometry for T

GradientStops
    struct GradientStops
        // and private fields
        pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Deref
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for GradientStops

Grid
    struct Grid<Rows = [Track; 0], Cols = [Track; 0]>
        // and private fields
        impl Grid
            pub fn new() -> Self
        impl<Rows, Cols> Grid<Rows, Cols>
            pub fn rows<NewRows: AsRef<[Track]>>(self, rows: NewRows) -> Grid<NewRows, Cols>
            pub fn cols<NewCols: AsRef<[Track]>>(self, cols: NewCols) -> Grid<Rows, NewCols>
            pub const fn background(self, background: Background) -> Self
            pub const fn default_background(self, background: Background) -> Self
            pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R> where Rows: AsRef<[Track]>, Cols: AsRef<[Track]>
        impl ThemeDefaults  (blanket)
        impl<Rows, Cols> Configure for Grid<Rows, Cols>
        impl<Rows: Debug, Cols: Debug> Debug for Grid<Rows, Cols>

GridCell
    struct GridCell
        // and private fields
        pub const fn at(row: u16, col: u16) -> Self
        pub const fn with_span(self, row_span: u16, col_span: u16) -> Self
        pub const fn row(self) -> u16
        pub const fn col(self) -> u16
        pub const fn row_span(self) -> u16
        pub const fn col_span(self) -> u16
        pub const fn along(axis: Axis, main: u16) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl From<(u16, u16)>
        impl Hash
        impl PartialEq
        impl Pod
        impl Zeroable

HAlign
    enum HAlign
        Auto = 0
        Left = 1
        Center = 2
        Right = 3
        Stretch = 4
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

HostDisconnected  [feature: winit]
    struct HostDisconnected
        impl Clone
        impl Copy
        impl Debug
        impl Display
        impl Eq
        impl Error
        impl PartialEq

HostHandle  [feature: winit]
    struct HostHandle<T: 'static>
        // and private fields
        pub fn request_repaint(&self, window: WindowToken)
        pub fn run_on_main(&self, f: impl FnOnce(&mut T) -> bool + Send + 'static) -> Result<(), HostDisconnected>
        pub fn quit(&self)
        impl<T: 'static> Clone for HostHandle<T>
        impl<T: 'static> Debug for HostHandle<T>

Hsv
    struct Hsv
        pub h: f32
        pub s: f32
        pub v: f32
        pub const fn new(h: f32, s: f32, v: f32) -> Self
        pub fn to_color(self) -> RgbaF32
        pub fn from_color(color: RgbaF32, fallback_hue: f32) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

HueSlice
    struct HueSlice
        (_)
        pub fn color(self, s: f32, v: f32) -> RgbaF32
        impl Clone
        impl Copy
        impl Debug

IVec2
    pub use glam::IVec2 as IVec2

IconDefinition
    struct IconDefinition
        pub name: Cow<'static, str>
        pub view_box: Vec2
        pub svg: Span
        pub tintable: bool
        pub filtered: bool
        impl Clone
        impl Debug

IconHandle
    struct IconHandle
        // and private fields
        pub const fn view_box(&self) -> Vec2
        impl Clone
        impl Copy
        impl Debug
        impl PartialEq

IconId
    struct IconId
        (pub u16)
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

IconSet
    struct IconSet
        // and private fields
        pub fn handle(&self, icon: IconId) -> IconHandle
        pub fn by_name(&self, name: &str) -> Option<IconId>
        pub fn shape(&self, icon: IconId) -> IconShape
        impl Clone
        impl Debug

IconTable
    struct IconTable
        // and private fields
        pub const fn baked(icons: &'static [IconDefinition], svg: &'static [u8]) -> Self
        pub fn from_svgs<'a, N: Into<Cow<'static, str>>>(sources: impl IntoIterator<Item = (N, &'a str)>) -> Result<Self, IconTableError>
        impl Debug

IconTableError
    enum IconTableError
        Unreadable { name: Cow<'static, str> }
        TooMany { count: usize }
        DuplicateName { name: Cow<'static, str> }
        impl Clone
        impl Debug
        impl Display
        impl Eq
        impl Error
        impl PartialEq

Image
    struct Image
        // and private fields
        pub fn from_srgba8(size: UVec2, pixels: Vec<u8>) -> Result<Self, ImageDataError>
        pub fn blank(size: UVec2) -> Self
        pub const fn size(&self) -> UVec2
        pub fn texels(&self) -> &[SrgbaU8]
        pub fn texels_mut(&mut self) -> &mut [SrgbaU8]
        pub fn fill_with(&mut self, texel: impl FnMut(u32, u32) -> SrgbaU8)
        pub fn row_mut(&mut self, row: u32) -> &mut [SrgbaU8]
        pub fn repeat_row(&mut self, row: u32)
        impl Clone
        impl Debug
        impl Eq
        impl PartialEq

ImageDataError
    enum ImageDataError
        ZeroSize { size: UVec2 }
        TooLarge { size: UVec2 }
        LengthMismatch { size: UVec2, expected: usize, actual: usize }
        impl Clone
        impl Copy
        impl Debug
        impl Display
        impl Eq
        impl Error
        impl PartialEq

ImageDownsample
    enum ImageDownsample
        Single
        Mean
        Peak
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

ImageFilter
    enum ImageFilter
        Linear
        Nearest
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for ImageFilter

ImageFit
    enum ImageFit
        Fill
        Contain
        Cover
        None
        Tile { offset: Vec2, scale: Vec2 }
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

ImageHandle
    struct ImageHandle
        // and private fields
        pub fn size(&self) -> UVec2
        pub fn update(&self, image: &Image)
        impl Clone
        impl Debug

ImageTooLarge
    struct ImageTooLarge
        pub size: UVec2
        pub max_dimension: u32
        impl Clone
        impl Copy
        impl Debug
        impl Display
        impl Eq
        impl Error
        impl PartialEq

ImePreedit
    struct ImePreedit<'a>
        pub text: &'a str
        pub cursor: Option<Span>
        impl<'a> Clone for ImePreedit<'a>
        impl<'a> Copy for ImePreedit<'a>
        impl<'a> Debug for ImePreedit<'a>
        impl<'a> Eq for ImePreedit<'a>
        impl<'a> PartialEq for ImePreedit<'a>

InnerResponse
    struct InnerResponse<'a, R>
        pub response: Response<'a>
        pub inner: R
        impl<'a, R: Debug> Debug for InnerResponse<'a, R>

InputDelta
    struct InputDelta
        pub repaint_requested: bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

InputEvent
    enum InputEvent<'a>
        PointerMoved(Vec2)
        PointerLeft
        PointerPressed(PointerButton)
        PointerReleased(PointerButton)
        ScrollPixels(Vec2)
        ScrollLines(Vec2)
        Zoom(f32)
        KeyDown { key: Key, repeat: bool, physical: Key, text: KeyText }
        ModifiersChanged(Modifiers)
        ImePreedit(ImePreedit<'a>)
        ImeCommit(&'a str)
        SurfaceFocusLost
        impl<'a> Clone for InputEvent<'a>
        impl<'a> Copy for InputEvent<'a>
        impl<'a> Debug for InputEvent<'a>

InputPolicy
    enum InputPolicy
        Always
        OnDelta
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

InternedStr
    struct InternedStr
        // and private fields
        pub const fn is_empty(&self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl From<InternedStr> for TextInput<'_>

Interpolation
    enum Interpolation
        Oklab
        Linear
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for Interpolation

Justify
    enum Justify
        Start
        Center
        End
        SpaceBetween
        SpaceAround
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

Key
    enum Key
        ArrowLeft
        ArrowRight
        ArrowUp
        ArrowDown
        Backspace
        Delete
        Home
        End
        PageUp
        PageDown
        Enter
        Tab
        Escape
        F1
        F2
        F3
        F4
        F5
        F6
        F7
        F8
        F9
        F10
        F11
        F12
        Char(char)
        Other
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

KeyClass
    enum KeyClass
        Text
        Edit
        Caret
        Page
        Focus
        Cycle
        Escape
        Accel
        pub fn of(press: KeyPress) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

KeyFilter
    struct KeyFilter
        (_)
        pub const TEXT: Self
        pub const EDIT: Self
        pub const CARET: Self
        pub const PAGE: Self
        pub const FOCUS: Self
        pub const CYCLE: Self
        pub const ESCAPE: Self
        pub const ACCEL: Self
        pub const NONE: Self
        pub const ALL: Self
        pub const fn is_empty(self) -> bool
        pub const fn contains(self, other: Self) -> bool
        pub const fn intersects(self, other: Self) -> bool
        pub const fn union(self, other: Self) -> Self
        pub const fn difference(self, other: Self) -> Self
        pub const fn insert(&mut self, other: Self)
        pub const fn remove(&mut self, other: Self)
        pub const fn set(&mut self, other: Self, on: bool)
        pub const TEXT_FIELD: Self
        pub const fn takes(self, class: KeyClass) -> bool
        pub fn takes_press(self, press: KeyPress) -> bool
        impl BitOr
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

KeyPress
    struct KeyPress
        pub key: Key
        pub mods: Modifiers
        pub repeat: bool
        pub physical: Key
        pub text: KeyText
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

KeyText
    struct KeyText
        // and private fields
        pub const CAPACITY: usize
        pub const EMPTY: Self
        pub fn new(text: &str) -> Self
        pub fn from_char(c: char) -> Self
        pub fn as_str(&self) -> &str
        pub fn is_empty(&self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

KeyboardWake
    struct KeyboardWake
        (_)
        pub const KEY: Self
        pub const MODIFIER: Self
        pub const NONE: Self
        pub const ALL: Self
        pub const fn is_empty(self) -> bool
        pub const fn contains(self, other: Self) -> bool
        pub const fn intersects(self, other: Self) -> bool
        pub const fn union(self, other: Self) -> Self
        pub const fn difference(self, other: Self) -> Self
        pub const fn insert(&mut self, other: Self)
        pub const fn remove(&mut self, other: Self)
        pub const fn set(&mut self, other: Self, on: bool)
        impl BitOr
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

Layer
    enum Layer
        Main = 0
        Popup = 1
        Modal = 2
        Menu = 3
        Tooltip = 4
        Debug = 5
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl Ord
        impl PartialEq
        impl PartialOrd

LayerScope
    struct LayerScope<'a>
        // and private fields
        pub const fn fixed_at(self, point: Vec2) -> Self
        pub const fn anchor(self, anchor: Anchor) -> Self
        pub fn max_size(self, size: impl Into<Size>) -> Self
        pub fn show<R>(self, body: impl FnOnce(&mut Ui) -> R) -> R
        impl<'a> Debug for LayerScope<'a>

LinearGeometry
    struct LinearGeometry
        pub angle: f32
        impl Clone
        impl Copy
        impl Debug
        impl GradientGeometry  (blanket)
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for LinearGeometry

LinearGradient
    type LinearGradient = Gradient<LinearGeometry>

LinearGradientBuilder
    type LinearGradientBuilder = GradientBuilder<LinearGeometry>

MenuItem
    struct MenuItem<'a>
        // and private fields
        pub fn new(label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a MenuItemTheme>>) -> Self
        pub const fn shortcut(self, s: Shortcut) -> Self
        pub const fn shortcut_hint(self, shortcut: Shortcut) -> Self
        pub fn show<'ui>(self, ui: &'ui mut Ui, popup: &CloseHandle) -> Response<'ui>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for MenuItem<'a>

MenuItemTheme
    struct MenuItemTheme
        pub looks: StatefulLook
        pub shortcut: RgbaF32
        pub gap: f32
        pub defaults: SlotDefaults
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl ThemeSlot
        impl<'de> Deserialize<'de> for MenuItemTheme

MenuSeparator
    struct MenuSeparator<'a>
        // and private fields
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a SeparatorTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for MenuSeparator<'a>

Modal
    struct Modal<'a>
        // and private fields
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a ModalTheme>>) -> Self
        pub const fn backdrop(self, c: RgbaF32) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &CloseHandle) -> R) -> OverlayResponse<R>
        pub const fn background(self, background: Background) -> Self
        pub const fn default_background(self, background: Background) -> Self
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Modal<'a>

ModalTheme
    struct ModalTheme
        pub panel: Background
        pub backdrop: RgbaF32
        pub padding: Spacing
        pub min_width: f32
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for ModalTheme

Modifiers
    struct Modifiers
        pub ctrl: bool
        pub shift: bool
        pub alt: bool
        pub mac_ctrl: bool
        pub meta: bool
        pub const NONE: Self
        pub const SHIFT: Self
        pub const CTRL: Self
        pub const ALT: Self
        pub const CTRL_SHIFT: Self
        pub const fn has_command(self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl From<Modifiers> for ShortcutMods
        impl Hash
        impl PartialEq

NodeIndex
    struct NodeIndex
        (_)
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for NodeIndex

OffscreenHost
    struct OffscreenHost
        // and private fields
        pub const WINDOW: WindowToken
        pub const fn builder(gpu: Gpu) -> OffscreenHostBuilder
        pub const fn ui(&mut self) -> &mut Ui
        pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta
        pub fn frame<T: App>(&mut self, target: RenderTarget<'_>, system_scale: f32, app: &mut T) -> FrameReport
        pub const fn gpu_pass_stats(&self) -> &GpuPassStats
        impl Debug

OffscreenHostBuilder  [shape differs by feature]
    struct OffscreenHostBuilder
        // and private fields
        pub fn fonts(self, scope: FontScope) -> Self
        pub fn shaper(self, shaper: TextShaper) -> Self
        pub const fn collect_gpu_stats(self, collect: bool) -> Self
        pub fn clock(self, clock: impl Clock + 'static) -> Self
        pub const fn retained_target(self, retained: bool) -> Self
        pub const fn pixel_snap(self, pixel_snap: bool) -> Self
        pub fn build(self) -> OffscreenHost
        impl Debug

Okhsv
    struct Okhsv
        pub h: f32
        pub s: f32
        pub v: f32
        pub const fn new(h: f32, s: f32, v: f32) -> Self
        pub fn to_color(self) -> RgbaF32
        pub fn from_color(color: RgbaF32, fallback_hue: f32) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

OverlayResponse
    struct OverlayResponse<R>
        pub id: WidgetId
        pub dismissed: bool
        pub close_requested: bool
        pub inner: R
        pub const fn closed(&self) -> bool
        impl<R: Clone> Clone for OverlayResponse<R>
        impl<R: Copy> Copy for OverlayResponse<R>
        impl<R: Debug> Debug for OverlayResponse<R>
        impl<R: Default> Default for OverlayResponse<R>

PLATFORM
    const PLATFORM: Platform = { { Platform::Mac } }

Palette
    struct Palette
        pub text: RgbaF32
        pub text_muted: RgbaF32
        pub text_disabled: RgbaF32
        pub window_background: RgbaF32
        pub element: RgbaF32
        pub element_mid: RgbaF32
        pub element_strong: RgbaF32
        pub border_focused: RgbaF32
        pub accent: RgbaF32
        pub const DEFAULT: Self
        pub const fn border_soft(&self) -> RgbaF32
        pub const fn border_mid(&self) -> RgbaF32
        pub const fn border_strong(&self) -> RgbaF32
        pub fn popup_panel(&self) -> Background
        impl Clone
        impl Debug
        impl Default
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for Palette

Panel
    struct Panel
        // and private fields
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R>
        pub fn hstack() -> Self
        pub fn vstack() -> Self
        pub fn stack(axis: Axis) -> Self
        pub fn wrap_hstack() -> Self
        pub fn wrap_vstack() -> Self
        pub fn zstack() -> Self
        pub fn canvas() -> Self
        pub const fn background(self, background: Background) -> Self
        pub const fn default_background(self, background: Background) -> Self
        impl Configure
        impl Debug
        impl ThemeDefaults  (blanket)

PipelineStats
    struct PipelineStats
        pub vertex_shader_invocations: u64
        pub clipper_invocations: u64
        pub clipper_primitives_out: u64
        pub fragment_shader_invocations: u64
        pub compute_shader_invocations: u64
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

Platform
    enum Platform
        Mac
        Windows
        Linux
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

PointerAction
    struct PointerAction
        pub id: WidgetId
        pub button: PointerButton
        pub edge: PointerEdge
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

PointerButton
    enum PointerButton
        Left = 0
        Right = 1
        Middle = 2
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

PointerEdge
    enum PointerEdge
        Pressed { count: u8 }
        Clicked { count: u8 }
        DragStarted
        DragStopped
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

PointerEvent
    enum PointerEvent
        Move(Vec2)
        Down { pos: Vec2, button: PointerButton }
        Up { pos: Vec2, button: PointerButton }
        Scroll { pos: Vec2, pixels: Vec2, lines: Vec2 }
        Zoom { pos: Vec2, factor: f32 }
        Leave
        impl Clone
        impl Copy
        impl Debug
        impl PartialEq

PointerWake
    struct PointerWake
        (_)
        pub const BUTTONS: Self
        pub const MOVE: Self
        pub const SCROLL: Self
        pub const PINCH: Self
        pub const NONE: Self
        pub const ALL: Self
        pub const fn is_empty(self) -> bool
        pub const fn contains(self, other: Self) -> bool
        pub const fn intersects(self, other: Self) -> bool
        pub const fn union(self, other: Self) -> Self
        pub const fn difference(self, other: Self) -> Self
        pub const fn insert(&mut self, other: Self)
        pub const fn remove(&mut self, other: Self)
        pub const fn set(&mut self, other: Self, on: bool)
        impl BitOr
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

Popup
    struct Popup
        // and private fields
        pub fn new(anchor: Anchor) -> Self
        pub const fn layer(self, layer: Layer) -> Self
        pub const fn click_outside(self, m: ClickOutside) -> Self
        pub const fn anchor(self, anchor: Anchor) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &CloseHandle) -> R) -> OverlayResponse<R>
        pub const fn background(self, background: Background) -> Self
        pub const fn default_background(self, background: Background) -> Self
        impl Configure
        impl Debug
        impl ThemeDefaults  (blanket)

PopupTrigger
    struct PopupTrigger
        // and private fields
        pub fn on(snapshot: &ResponseSnapshot) -> Self
        pub const fn background(self, background: Background) -> Self
        pub const fn default_background(self, background: Background) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &CloseHandle) -> R) -> OverlayResponse<Option<R>>
        pub fn open(ui: &mut Ui, for_id: WidgetId)
        pub fn close(ui: &mut Ui, for_id: WidgetId)
        pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool
        impl Configure
        impl Debug
        impl ThemeDefaults  (blanket)

PowerPreference
    enum PowerPreference
        Any
        LowPower
        HighPerformance
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

ProgressBar
    struct ProgressBar<'a>
        // and private fields
        pub fn new(fraction: f32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ProgressBarTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for ProgressBar<'a>

ProgressBarTheme
    struct ProgressBarTheme
        pub track: RgbaF32
        pub fill: RgbaF32
        pub thickness: f32
        pub const fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for ProgressBarTheme

RadialGeometry
    struct RadialGeometry
        pub center: Vec2
        pub radius: Vec2
        impl Clone
        impl Copy
        impl Debug
        impl GradientGeometry  (blanket)
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for RadialGeometry

RadialGradient
    type RadialGradient = Gradient<RadialGeometry>

RadialGradientBuilder
    type RadialGradientBuilder = GradientBuilder<RadialGeometry>

RadioButton
    struct RadioButton<'a, T: PartialEq>
        // and private fields
        pub fn new(current: &'a mut T, value: T) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ToggleTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl ThemeDefaults  (blanket)
        impl<'a, T: Debug + PartialEq> Debug for RadioButton<'a, T>
        impl<T: PartialEq> Configure for RadioButton<'_, T>

RealtimeClock
    struct RealtimeClock
        // and private fields
        pub fn new() -> Self
        impl Clock
        impl Debug
        impl Default

Rect
    struct Rect
        pub min: Vec2
        pub size: Size
        pub const ZERO: Self
        pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self
        pub const fn from_min_max(min: Vec2, max: Vec2) -> Self
        pub const fn max(self) -> Vec2
        pub const fn center(self) -> Vec2
        pub const fn area(self) -> f32
        pub const fn is_paint_empty(self) -> bool
        pub const fn contains(self, p: Vec2) -> bool
        pub const fn contains_rect(self, other: Self) -> bool
        pub const fn inflated(self, amount: f32) -> Self
        pub const fn deflated(self, amount: f32) -> Self
        pub fn inscribed_for_corners(self, corners: Corners) -> Self
        pub fn inflated_by(self, s: Spacing) -> Self
        pub fn deflated_by(self, s: Spacing) -> Self
        pub const fn intersects(self, other: Self) -> bool
        pub const fn intersect(self, other: Self) -> Option<Self>
        pub const fn clamp_to(self, bounds: Self) -> Self
        pub const fn union(self, other: Self) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Hash
        impl PartialEq
        impl Pod
        impl Zeroable

RenderTarget
    struct RenderTarget<'a>
        // and private fields
        pub fn new(texture: &'a Texture) -> Self
        impl<'a> Clone for RenderTarget<'a>
        impl<'a> Copy for RenderTarget<'a>
        impl<'a> Debug for RenderTarget<'a>

RequestedGpu
    struct RequestedGpu
        pub adapter: Adapter
        pub gpu: Gpu
        pub fn headless(power_preference: PowerPreference, optional: Features) -> Result<Self, GpuRequestError>
        impl Debug

Response
    struct Response<'a>
        pub id: WidgetId
        // and private fields
        pub fn new(id: WidgetId, ui: &'a Ui, state: ResponseState) -> Self
        pub fn snapshot(&self) -> ResponseSnapshot
        impl Debug
        impl Deref

ResponseSnapshot
    struct ResponseSnapshot
        pub id: WidgetId
        pub state: ResponseState
        impl Clone
        impl Copy
        impl Debug
        impl Deref

ResponseState
    struct ResponseState
        pub rect: Option<Rect>
        pub layout_rect: Option<Rect>
        pub transform: TranslateScale
        pub pointer_local: Option<Vec2>
        pub pointer_over: bool
        pub disabled: bool
        pub focused: bool
        pub left: ButtonState
        pub right: ButtonState
        pub middle: ButtonState
        pub scroll: ScrollDelta
        pub const fn hovered(&self) -> bool
        pub const fn clicked(&self) -> bool
        pub const fn double_clicked(&self) -> bool
        pub const fn any_clicked(&self) -> bool
        pub const fn button(&self, button: PointerButton) -> &ButtonState
        pub const fn pressed(&self) -> bool
        pub fn press_fraction(&self, band: f32) -> Option<Vec2>
        impl Clone
        impl Copy
        impl Debug
        impl Default

RgbaF32
    struct RgbaF32
        pub r: f32
        pub g: f32
        pub b: f32
        pub a: f32
        pub const TRANSPARENT: Self
        pub const WHITE: Self
        pub const BLACK: Self
        pub const fn is_noop(self) -> bool
        pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self
        pub const fn srgb(r: f32, g: f32, b: f32) -> Self
        pub const fn srgba(r: f32, g: f32, b: f32, a: f32) -> Self
        pub const fn with_alpha(self, a: f32) -> Self
        pub const fn from_srgba(bytes: SrgbaU8) -> Self
        pub const fn hex(rgb: u32) -> Self
        pub fn to_srgba_u8(self) -> SrgbaU8
        impl Animatable
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl From<RgbaF32> for Brush
        impl From<RgbaF32> for SrgbaU8
        impl From<SrgbaU8>
        impl FromStr
        impl Hash
        impl PartialEq
        impl Pod
        impl Serialize
        impl Zeroable
        impl<'de> Deserialize<'de> for RgbaF32

Scroll
    struct Scroll<'a>
        // and private fields
        pub fn vertical() -> Self
        pub fn horizontal() -> Self
        pub fn both() -> Self
        pub fn pan_by(self, delta: Vec2) -> Self
        pub fn zoom_by(self, factor: f32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ScrollbarTheme>>) -> Self
        pub const fn bar_mode(self, mode: BarMode) -> Self
        pub const fn overlay_bars(self) -> Self
        pub const fn hide_bars(self) -> Self
        pub fn content_margin(self, m: impl Into<Spacing>) -> Self
        pub fn zoomable(self) -> Self
        pub fn zoom_config(self, config: ZoomConfig) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R>
        pub const fn background(self, background: Background) -> Self
        pub const fn default_background(self, background: Background) -> Self
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Scroll<'a>

ScrollDelta
    struct ScrollDelta
        pub pixels: Vec2
        pub lines: Vec2
        pub zoom: ZoomFactor
        pub fn pan(self, line_height: f32) -> Vec2
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

ScrollbarTheme
    struct ScrollbarTheme
        pub thickness: f32
        pub gap: f32
        pub min_thumb: f32
        pub track: RgbaF32
        pub thumb: RgbaF32
        pub thumb_hovered: RgbaF32
        pub thumb_active: RgbaF32
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for ScrollbarTheme

Sense
    struct Sense
        (_)
        pub const HOVER: Self
        pub const CLICK: Self
        pub const DRAG: Self
        pub const SCROLL_X: Self
        pub const SCROLL_Y: Self
        pub const PINCH: Self
        pub const NONE: Self
        pub const ALL: Self
        pub const fn is_empty(self) -> bool
        pub const fn contains(self, other: Self) -> bool
        pub const fn intersects(self, other: Self) -> bool
        pub const fn union(self, other: Self) -> Self
        pub const fn difference(self, other: Self) -> Self
        pub const fn insert(&mut self, other: Self)
        pub const fn remove(&mut self, other: Self)
        pub const fn set(&mut self, other: Self, on: bool)
        pub const SCROLL: Self
        pub const ABSORB_POINTER: Self
        impl BitOr
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

Separator
    struct Separator<'a>
        // and private fields
        pub fn horizontal() -> Self
        pub fn vertical() -> Self
        pub fn style(self, s: impl Into<Option<&'a SeparatorTheme>>) -> Self
        pub const fn thickness(self, px: f32) -> Self
        pub const fn color(self, c: RgbaF32) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Separator<'a>

SeparatorTheme
    struct SeparatorTheme
        pub color: RgbaF32
        pub thickness: f32
        pub margin: Spacing
        pub const fn from_palette(p: &Palette) -> Self
        pub fn menu_separator(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for SeparatorTheme

Shadow
    struct Shadow
        pub color: RgbaF32
        pub offset: Vec2
        pub blur: f32
        pub spread: f32
        pub inset: bool
        pub const NONE: Self
        pub const fn drop(color: RgbaF32, offset: Vec2, blur: f32) -> Self
        pub const fn with_spread(self, spread: f32) -> Self
        pub const fn inset(self) -> Self
        pub const fn is_noop(&self) -> bool
        impl Animatable
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for Shadow

Shortcut
    struct Shortcut
        pub mods: ShortcutMods
        pub key: Key
        pub const fn new(mods: ShortcutMods, key: Key) -> Self
        pub const fn key(key: Key) -> Self
        pub const fn ctrl(c: char) -> Self
        pub const fn ctrl_shift(c: char) -> Self
        pub fn matches(self, press: KeyPress) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Display
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

ShortcutMods
    struct ShortcutMods
        pub ctrl: bool
        pub shift: bool
        pub alt: bool
        pub meta: bool
        pub const fn has_command(self) -> bool
        pub const NONE: Self
        pub const SHIFT: Self
        pub const CTRL: Self
        pub const ALT: Self
        pub const CTRL_SHIFT: Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl From<Modifiers>
        impl Hash
        impl PartialEq

Size
    struct Size
        pub w: f32
        pub h: f32
        pub const ZERO: Self
        pub const INF: Self
        pub const fn new(w: f32, h: f32) -> Self
        pub const fn is_approx_zero(self) -> bool
        pub const fn is_paint_empty(self) -> bool
        pub const fn min(self, other: Self) -> Self
        pub const fn max(self, other: Self) -> Self
        pub const fn scaled_by(self, factor: f32) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl From<Size> for SizeSpec
        impl From<Size> for Vec2
        impl Hash
        impl PartialEq
        impl Pod
        impl Serialize
        impl Zeroable
        impl<'de> Deserialize<'de> for Size
        impl<T: Num> From<T> for Size
        impl<W: Num, H: Num> From<(W, H)> for Size

SizeSpec
    struct SizeSpec
        // and private fields
        pub const fn new(w: Sizing, h: Sizing) -> Self
        pub const fn w(self) -> Sizing
        pub const fn h(self) -> Sizing
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl From<Size>
        impl From<Sizing>
        impl Hash
        impl PartialEq
        impl<T: Num> From<T> for SizeSpec
        impl<W: Into<Sizing>, H: Into<Sizing>> From<(W, H)> for SizeSpec

Sizing
    struct Sizing
        (_)
        pub const HUG: Self
        pub const FILL: Self
        pub const fn fixed(value: f32) -> Self
        pub const fn fill(weight: f32) -> Self
        pub const fn split(fraction: f32) -> [Self; 2]
        pub const fn fixed_value(self) -> Option<f32>
        pub const fn fill_weight(self) -> Option<f32>
        pub const fn is_hug(self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl From<Sizing> for SizeSpec
        impl From<Sizing> for Track
        impl Hash
        impl PartialEq
        impl<T: Num> From<T> for Sizing

Slider
    struct Slider<'a>
        // and private fields
        pub fn new(value: impl Into<DragNum<'a>>, range: RangeInclusive<f64>) -> Self
        pub const fn step(self, step: f64) -> Self
        pub const fn decimals(self, n: usize) -> Self
        pub fn style(self, s: impl Into<Option<&'a SliderTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Slider<'a>

SliderTheme
    struct SliderTheme
        pub track: RgbaF32
        pub fill: RgbaF32
        pub knob: RgbaF32
        pub knob_size: f32
        pub track_thickness: f32
        pub const fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for SliderTheme

SlotDefaults
    struct SlotDefaults
        pub padding: Spacing
        pub margin: Spacing
        pub animation: Option<AnimationSpec>
        impl Clone
        impl Copy
        impl Debug
        impl Serialize
        impl<'de> Deserialize<'de> for SlotDefaults

Spacing
    struct Spacing
        (_)
        pub const ZERO: Self
        pub fn as_array(self) -> [f32; 4]
        pub fn all(v: f32) -> Self
        pub fn xy(x: f32, y: f32) -> Self
        pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self
        pub fn from_array(v: [f32; 4]) -> Self
        pub fn sums(self) -> Size
        impl Add
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Pod
        impl Serialize
        impl Zeroable
        impl<'de> Deserialize<'de> for Spacing
        impl<L: Num, T: Num, R: Num, B: Num> From<(L, T, R, B)> for Spacing
        impl<T: Num> From<T> for Spacing
        impl<X: Num, Y: Num> From<(X, Y)> for Spacing

Spinner
    struct Spinner<'a>
        // and private fields
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a SpinnerTheme>>) -> Self
        pub const fn diameter(self, px: f32) -> Self
        pub const fn color(self, c: RgbaF32) -> Self
        pub const fn thickness(self, px: f32) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Spinner<'a>

SpinnerTheme
    struct SpinnerTheme
        pub color: RgbaF32
        pub diameter: f32
        pub sweep: f32
        pub speed: f32
        pub thickness_ratio: f32
        pub min_thickness: f32
        pub const fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for SpinnerTheme

SplitDirection
    enum SplitDirection
        Row
        Column
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for SplitDirection

SplitHalf
    enum SplitHalf
        First
        Second
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

SplitSide
    enum SplitSide
        Left
        Right
        Top
        Bottom
        pub const fn direction(self) -> SplitDirection
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for SplitSide

Splitter
    struct Splitter<'a>
        // and private fields
        pub fn row(ratio: &'a mut f32) -> Self
        pub fn column(ratio: &'a mut f32) -> Self
        pub const fn min_pane(self, px: f32) -> Self
        pub fn style(self, s: impl Into<Option<&'a SplitterTheme>>) -> Self
        pub fn show(self, ui: &mut Ui, body: impl FnMut(&mut Ui, SplitHalf)) -> ValueResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Splitter<'a>

SplitterTheme
    struct SplitterTheme
        pub grab_thickness: f32
        pub rule: RgbaF32
        pub rule_thickness: f32
        pub hovered: RgbaF32
        pub active: RgbaF32
        pub const fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for SplitterTheme

Spread
    enum Spread
        Pad = 0
        Repeat = 1
        Reflect = 2
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for Spread

SrgbaU8
    struct SrgbaU8
        pub r: u8
        pub g: u8
        pub b: u8
        pub a: u8
        pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self
        pub const fn rgb(r: u8, g: u8, b: u8) -> Self
        pub const fn hex(rgb: u32) -> Self
        pub const fn hexa(rgba: u32) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl From<RgbaF32>
        impl From<SrgbaU8> for Brush
        impl From<SrgbaU8> for RgbaF32
        impl Hash
        impl PartialEq
        impl Pod
        impl Zeroable

StatefulLook
    struct StatefulLook
        pub normal: WidgetLook
        pub hovered: WidgetLook
        pub active: WidgetLook
        pub disabled: WidgetLook
        pub const fn pick(&self, state: &ResponseState, active: bool) -> &WidgetLook
        impl Clone
        impl Debug
        impl Default
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for StatefulLook

Stop
    struct Stop
        // and private fields
        pub fn new(offset: f32, color: RgbaF32) -> Self
        pub const fn offset(self) -> f32
        pub const fn color(self) -> RgbaF32
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for Stop

Stroke
    struct Stroke
        pub color: RgbaF32
        pub width: f32
        pub const NONE: Self
        pub const fn is_noop(&self) -> bool
        pub const fn new(color: RgbaF32, width: f32) -> Self
        impl Animatable
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for Stroke

SurfaceError  [feature: winit]
    #[non_exhaustive]
    enum SurfaceError
        Create { source: DriverError }
        Device { source: GpuRequestError }
        Incompatible
        MissingSrgb
        MissingUsages { missing: String }
        impl Debug
        impl Display
        impl Error
        impl From<GpuRequestError>

Switch
    struct Switch<'a>
        // and private fields
        pub fn new(value: &'a mut bool) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ToggleTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Switch<'a>

TabAddress
    struct TabAddress
        pub group: TabGroupId
        pub index: usize
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

TabBadge
    enum TabBadge
        None
        Idle
        On
        pub const fn is_reserved(self) -> bool
        pub const fn is_inked(self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

TabGroup
    struct TabGroup<T>
        pub id: TabGroupId
        pub tabs: Vec<T>
        pub active: usize
        impl<T: Copy> TabGroup<T>
            pub fn active_tab(&self) -> T
        impl<'de, T> Deserialize<'de> for TabGroup<T> where T: Deserialize<'de>
        impl<T: Clone> Clone for TabGroup<T>
        impl<T: Debug> Debug for TabGroup<T>
        impl<T: PartialEq> PartialEq for TabGroup<T>
        impl<T> Serialize for TabGroup<T> where T: Serialize

TabGroupId
    struct TabGroupId
        (_)
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for TabGroupId

TabItem
    struct TabItem
        pub key: u64
        pub label: InternedStr
        pub closable: bool
        pub draggable: bool
        pub badge: TabBadge
        pub icon: Option<IconHandle>
        pub const fn new(key: u64, label: InternedStr) -> Self
        impl Clone
        impl Copy
        impl Debug

TabOverflow
    enum TabOverflow
        Scroll
        Menu
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

TabStrip
    struct TabStrip<'a>
        // and private fields
        pub fn new(items: &'a [TabItem]) -> Self
        pub fn selected(self, selected: impl Into<Option<usize>>) -> Self
        pub const fn focused(self, focused: bool) -> Self
        pub const fn overflow(self, overflow: TabOverflow) -> Self
        pub fn style(self, s: impl Into<Option<&'a TabsTheme>>) -> Self
        pub fn chip_id(strip: WidgetId, key: u64) -> WidgetId
        pub fn close_id(strip: WidgetId, key: u64) -> WidgetId
        pub fn show(self, ui: &mut Ui) -> TabStripResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for TabStrip<'a>

TabStripResponse
    struct TabStripResponse<'a>
        pub response: Response<'a>
        pub clicked: Option<usize>
        pub keyed: Option<usize>
        pub menu_picked: Option<usize>
        pub closed: Option<usize>
        pub drag_started: Option<usize>
        pub drag_stopped: Option<usize>
        pub fn activated(&self) -> Option<usize>
        impl<'a> Debug for TabStripResponse<'a>

TabbedView
    struct TabbedView<'a, S, L, K = fn(usize, &S) -> u64>
        // and private fields
        impl<'a, S: AsRef<str>> TabbedView<'a, S, fn(&S) -> &str>
            pub fn new(selected: &'a mut usize, options: &'a [S]) -> Self
        impl<'a, S, L: Fn(&S) -> &str> TabbedView<'a, S, L>
            pub fn labeled(selected: &'a mut usize, options: &'a [S], label: L) -> Self
            pub fn keyed<H: Hash>(self, key: impl Fn(&S) -> H) -> TabbedView<'a, S, L, impl Fn(usize, &S) -> u64>
        impl<'a, S, L: Fn(&S) -> &str, K: Fn(usize, &S) -> u64> TabbedView<'a, S, L, K>
            pub const fn closable(self, closable: bool) -> Self
            pub const fn reorderable(self, reorderable: bool) -> Self
            pub const fn overflow(self, overflow: TabOverflow) -> Self
            pub fn style(self, s: impl Into<Option<&'a TabsTheme>>) -> Self
            pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, usize)) -> TabbedViewResponse<'_>
        impl ThemeDefaults  (blanket)
        impl<'a, S: Debug, L: Debug, K: Debug> Debug for TabbedView<'a, S, L, K>
        impl<S, L, K> Configure for TabbedView<'_, S, L, K>

TabbedViewResponse
    struct TabbedViewResponse<'a>
        pub response: Response<'a>
        pub action: Option<TabsAction>
        impl<'a> Debug for TabbedViewResponse<'a>

TabsAction
    enum TabsAction
        Activated { index: usize }
        Closed { index: usize }
        Reordered { from: usize, to: usize }
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

TabsTheme
    struct TabsTheme
        pub active: StatefulLook
        pub inactive: StatefulLook
        pub accent: RgbaF32
        pub accent_idle: RgbaF32
        pub accent_thickness: f32
        pub strip: Background
        pub strip_padding: Spacing
        pub gap: f32
        pub rule: RgbaF32
        pub rule_thickness: f32
        pub radius: f32
        pub chip_padding: Spacing
        pub trailing_inset: f32
        pub min_width: f32
        pub max_width: f32
        pub close: StatefulLook
        pub close_size: f32
        pub badge: RgbaF32
        pub badge_size: f32
        pub label_gap: f32
        pub defaults: SlotDefaults
        pub const fn cap(&self, focused: bool) -> RgbaF32
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl ThemeSlot
        impl<'de> Deserialize<'de> for TabsTheme

TargetFormat
    struct TargetFormat
        (_)
        pub fn new(format: TextureFormat) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

Text
    struct Text<'a>
        // and private fields
        pub fn new(text: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a TextStyle>>) -> Self
        pub const fn color(self, color: RgbaF32) -> Self
        pub const fn font_size(self, px: f32) -> Self
        pub const fn line_height_factor(self, factor: f32) -> Self
        pub const fn family(self, family: FontFamily) -> Self
        pub const fn weight(self, weight: FontWeight) -> Self
        pub const fn slant(self, slant: FontSlant) -> Self
        pub const fn bold(self) -> Self
        pub const fn italic(self) -> Self
        pub const fn text_wrap(self, wrap: TextWrap) -> Self
        pub const fn text_align(self, a: Align) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Text<'a>

TextEdit
    struct TextEdit<'a>
        // and private fields
        pub fn new(text: &'a mut String) -> Self
        pub fn style(self, s: impl Into<Option<&'a TextEditTheme>>) -> Self
        pub const fn color(self, color: RgbaF32) -> Self
        pub const fn font_size(self, px: f32) -> Self
        pub const fn line_height_factor(self, factor: f32) -> Self
        pub const fn family(self, family: FontFamily) -> Self
        pub const fn weight(self, weight: FontWeight) -> Self
        pub const fn slant(self, slant: FontSlant) -> Self
        pub const fn bold(self) -> Self
        pub const fn italic(self) -> Self
        pub const fn select_all_on_focus(self, on: bool) -> Self
        pub const fn escape_falls_through(self, on: bool) -> Self
        pub const fn max_chars(self, n: usize) -> Self
        pub const fn text_align(self, a: Align) -> Self
        pub const fn multiline(self, on: bool) -> Self
        pub fn placeholder(self, text: impl Into<TextInput<'a>>) -> Self
        pub fn show(self, ui: &mut Ui) -> TextEditResponse<'_>
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for TextEdit<'a>

TextEditResponse
    struct TextEditResponse<'a>
        pub response: Response<'a>
        pub changed: bool
        pub committed: bool
        pub submitted: bool
        pub canceled: bool
        pub focus_gained: bool
        pub focus_lost: bool
        impl<'a> Debug for TextEditResponse<'a>

TextEditTheme
    struct TextEditTheme
        pub looks: StatefulLook
        pub placeholder: RgbaF32
        pub caret: RgbaF32
        pub caret_width: f32
        pub selection: RgbaF32
        pub defaults: SlotDefaults
        pub fn corner_centering(&self, text: Size, at: Vec2) -> Vec2
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl ThemeSlot
        impl<'de> Deserialize<'de> for TextEditTheme

TextInput
    enum TextInput<'a>
        Borrowed(&'a str)
        Owned(String)
        Interned(InternedStr)
        impl Default
        impl From<InternedStr>
        impl From<String>
        impl<'a, T: AsRef<str> + ?Sized> From<&'a T> for TextInput<'a>
        impl<'a> Debug for TextInput<'a>
        impl<'a> From<Cow<'a, str>> for TextInput<'a>

TextShaper
    struct TextShaper
        // and private fields
        pub fn new() -> Self
        pub fn with_fonts(scope: FontScope) -> Self
        pub fn load_font(&self, source: impl Into<FontSource>) -> Result<FontFamily, FontLoadError>
        pub fn has_font(&self, family: FontFamily) -> bool
        pub fn font_families(&self) -> Vec<FontFamily>
        pub fn glyphs(&self) -> TextGlyphs<'_>
        impl Clone
        impl Debug
        impl Default

TextStyle
    struct TextStyle
        pub font_size: f32
        pub color: RgbaF32
        pub line_height_factor: f32
        pub family: FontFamily
        pub weight: FontWeight
        pub slant: FontSlant
        pub fn font(&self) -> GlyphFont
        pub fn line_height_for(&self, font_size: f32) -> f32
        pub const fn with_font_size(self, px: f32) -> Self
        pub const fn with_color(self, c: RgbaF32) -> Self
        pub const fn with_line_height_factor(self, factor: f32) -> Self
        pub const fn with_family(self, family: FontFamily) -> Self
        pub const fn with_weight(self, weight: FontWeight) -> Self
        pub const fn with_slant(self, slant: FontSlant) -> Self
        pub const fn bold(self) -> Self
        pub const fn italic(self) -> Self
        impl Animatable
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for TextStyle

TextStyleOverrides
    struct TextStyleOverrides
        pub color: Option<RgbaF32>
        pub font_size: Option<f32>
        pub line_height_factor: Option<f32>
        pub family: Option<FontFamily>
        pub weight: Option<FontWeight>
        pub slant: Option<FontSlant>
        pub const NONE: Self
        pub fn apply(self, base: &TextStyle) -> TextStyle
        pub const fn with_font_size(self, px: f32) -> Self
        pub const fn with_color(self, c: RgbaF32) -> Self
        pub const fn with_line_height_factor(self, factor: f32) -> Self
        pub const fn with_family(self, family: FontFamily) -> Self
        pub const fn with_weight(self, weight: FontWeight) -> Self
        pub const fn with_slant(self, slant: FontSlant) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for TextStyleOverrides

TextWrap
    enum TextWrap
        SingleLine
        Scroll
        Truncate
        Ellipsis
        Wrap
        WrapWithOverflow
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

Theme
    struct Theme
        pub button: ButtonTheme
        pub checkbox: ToggleTheme
        pub radio: ToggleTheme
        pub switch: ToggleTheme
        pub scrollbar: ScrollbarTheme
        pub text_edit: TextEditTheme
        pub drag_value: DragValueTheme
        pub context_menu: ContextMenuTheme
        pub combo_box: ComboBoxTheme
        pub modal: ModalTheme
        pub color_picker: ColorPickerTheme
        pub tooltip: TooltipTheme
        pub progress_bar: ProgressBarTheme
        pub separator: SeparatorTheme
        pub slider: SliderTheme
        pub spinner: SpinnerTheme
        pub splitter: SplitterTheme
        pub tabs: TabsTheme
        pub dock: DockTheme
        pub expander: ExpanderTheme
        pub focus_ring: FocusRingTheme
        pub text: TextStyle
        pub window_clear: RgbaF32
        pub panel_background: Option<Background>
        pub panel_clip: ClipMode
        pub fn scale_text(&mut self, factor: f32)
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for Theme

ToggleTheme
    struct ToggleTheme
        pub unchecked: StatefulLook
        pub checked: StatefulLook
        pub indicator: RgbaF32
        pub box_size: f32
        pub indicator_width: f32
        pub check_points: [Vec2; 3]
        pub indicator_inset: f32
        pub gap: f32
        pub track_aspect: f32
        pub defaults: SlotDefaults
        pub fn checkbox(p: &Palette) -> Self
        pub fn radio(p: &Palette) -> Self
        pub fn switch(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Serialize
        impl ThemeSlot
        impl<'de> Deserialize<'de> for ToggleTheme

Tooltip
    struct Tooltip<'a>
        // and private fields
        pub fn on(snapshot: &'a ResponseSnapshot, text: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a TooltipTheme>>) -> Self
        pub const fn delay(self, delay: Duration) -> Self
        pub const fn when_disabled(self, yes: bool) -> Self
        pub fn show(self, ui: &mut Ui) -> TooltipResponse
        pub const fn background(self, background: Background) -> Self
        pub const fn default_background(self, background: Background) -> Self
        impl Configure
        impl ThemeDefaults  (blanket)
        impl<'a> Debug for Tooltip<'a>

TooltipResponse
    struct TooltipResponse
        pub visible: bool
        impl Clone
        impl Copy
        impl Debug

TooltipTheme
    struct TooltipTheme
        pub panel: Background
        pub text: TextStyleOverrides
        pub padding: Spacing
        pub max_size: Size
        pub delay: Duration
        pub warmup: Duration
        pub gap: f32
        pub fn from_palette(p: &Palette) -> Self
        impl Clone
        impl Debug
        impl Default
        impl Serialize
        impl<'de> Deserialize<'de> for TooltipTheme

Track
    struct Track
        // and private fields
        pub const fn new(size: Sizing) -> Self
        pub const HUG: Self
        pub const FILL: Self
        pub const fn fixed(v: f32) -> Self
        pub const fn fill(weight: f32) -> Self
        pub const fn with_min(self, min: f32) -> Self
        pub const fn with_max(self, max: f32) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl From<Sizing>
        impl Hash
        impl PartialEq

TranslateScale
    struct TranslateScale
        // and private fields
        pub const IDENTITY: Self
        pub const fn is_identity(self) -> bool
        pub const fn new(translation: Vec2, scale: f32) -> Self
        pub const fn from_translation(t: Vec2) -> Self
        pub const fn from_scale(s: f32) -> Self
        pub const fn anchored_at(self, origin: Vec2) -> Self
        pub const fn compose(self, other: Self) -> Self
        pub const fn apply_point(self, p: Vec2) -> Vec2
        pub const fn inverse_vector(self, v: Vec2) -> Vec2
        pub const fn apply_rect(self, r: Rect) -> Rect
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq

UVec2
    pub use glam::UVec2 as UVec2

Ui
    struct Ui
        // and private fields
        pub const fn theme(&self) -> &Rc<Theme>
        pub fn set_theme(&mut self, theme: impl Into<Rc<Theme>>)
        pub const fn watch_pointer(&mut self, flags: PointerWake)
        pub const fn watch_keyboard(&mut self, flags: KeyboardWake)
        pub fn watch_key(&mut self, shortcut: Shortcut)
        pub fn pointer_events(&self) -> &[PointerEvent]
        pub fn keyboard_events(&self) -> &[KeyPress]
        pub fn key_pressed(&mut self, shortcut: Shortcut) -> bool
        pub fn request_relayout(&mut self)
        pub const fn now(&self) -> Duration
        pub const fn set_cursor(&mut self, cursor: CursorIcon)
        pub const fn cursor(&self) -> CursorIcon
        pub const fn set_vsync(&mut self, vsync: Vsync)
        pub const fn vsync(&self) -> Vsync
        pub fn request_repaint(&mut self)
        pub fn request_repaint_after(&mut self, after: Duration)
        pub fn open_window(&mut self, token: WindowToken, config: WindowConfig)
        pub fn close_window(&mut self, token: WindowToken)
        pub const fn close_requested(&self) -> bool
        pub const fn keep_open(&mut self)
        pub fn window_geometry(&self) -> WindowGeometry
        pub fn debug_overlay(&self) -> DebugOverlayConfig
        pub fn set_debug_overlay(&mut self, overlay: DebugOverlayConfig)
        pub fn is_window_open(&self, token: WindowToken) -> bool
        pub fn add_shape<S: Lower>(&mut self, shape: S)
        pub fn load_icons(&self, table: impl Into<Rc<IconTable>>) -> IconSet
        pub fn load_image(&self, image: &Image) -> Result<ImageHandle, ImageTooLarge>
        pub fn load_font(&self, source: impl Into<FontSource>) -> Result<FontFamily, FontLoadError>
        pub fn has_font(&self, family: FontFamily) -> bool
        pub fn font_families(&self) -> Vec<FontFamily>
        pub const fn max_image_dimension(&self) -> Option<NonZeroU32>
        pub fn clipboard(&self) -> Clipboard
        pub fn fmt(&mut self, args: Arguments<'_>) -> InternedStr
        pub fn intern<'a>(&mut self, text: impl Into<TextInput<'a>>) -> InternedStr
        pub fn text(&self, text: InternedStr) -> &str
        pub fn add_shape_animated<S: Lower>(&mut self, shape: S, animation: PaintAnimation)
        pub fn layer(&mut self, layer: Layer) -> LayerScope<'_>
        pub fn release_input_scope(&mut self, id: WidgetId)
        pub fn response_for(&self, id: WidgetId) -> ResponseState
        pub fn state<S: 'static>(&self, id: WidgetId) -> Option<&S>
        pub fn with_state<S: Default + 'static, R>(&mut self, id: WidgetId, body: impl FnOnce(&mut Self, &mut S) -> R) -> R
        pub fn singleton<S: 'static>(&self) -> Option<&S>
        pub fn with_singleton<S: Default + 'static, R>(&mut self, body: impl FnOnce(&mut Self, &mut S) -> R) -> R
        pub fn animate<V: Animatable>(&mut self, id: WidgetId, slot: impl Into<AnimationSlot>, target: V, spec: impl Into<Option<AnimationSpec>>) -> V
        pub const fn focus(&self) -> Option<WidgetId>
        pub fn is_focus_within(&self, ancestor: WidgetId) -> bool
        pub fn is_hover_within(&self, ancestor: WidgetId) -> bool
        pub const fn display(&self) -> Display
        pub fn user_scale(&self) -> UserScale
        pub fn set_user_scale(&mut self, scale: UserScale)
        pub const fn frame_id(&self) -> u64
        pub const fn render_frame_id(&self) -> u64
        pub fn probe_text<'a>(&'a mut self, run: TextRun<'a>) -> TextProbe<'a>
        pub fn pointer_actions(&self) -> impl Iterator<Item = PointerAction> + '_
        pub const fn set_focus(&mut self, id: WidgetId)
        pub const fn clear_focus(&mut self)
        pub const fn request_ime(&mut self, caret: Rect)
        pub fn ime_preedit(&self) -> Option<ImePreedit<'_>>
        pub const fn is_focus_visible(&self) -> bool
        pub const fn focus_first_within(&mut self, ancestor: WidgetId)
        pub const fn pointer_pos(&mut self) -> Option<Vec2>
        pub fn pointer_local(&mut self, id: WidgetId) -> Option<Vec2>
        pub const fn modifiers(&mut self) -> Modifiers
        pub const fn peek_pointer_pos(&self) -> Option<Vec2>
        pub fn peek_pointer_local(&self, id: WidgetId) -> Option<Vec2>
        pub const fn peek_modifiers(&self) -> Modifiers
        pub const fn focus_policy(&self) -> FocusPolicy
        pub const fn set_focus_policy(&mut self, p: FocusPolicy)
        pub const fn input_policy(&self) -> InputPolicy
        pub const fn set_input_policy(&mut self, p: InputPolicy)
        impl Debug

UnmetRequirements
    enum UnmetRequirements
        Features { missing: String }
        Limit { name: &'static str, required: u64, available: u64 }
        impl Clone
        impl Debug
        impl Display
        impl Eq
        impl Error
        impl PartialEq

UserScale
    struct UserScale
        (_)
        pub const ONE: Self
        pub const LADDER: [f32; 13]
        pub const MIN: f32
        pub const MAX: f32
        pub const fn new(factor: f32) -> Option<Self>
        pub const fn get(self) -> f32
        pub const fn applied_to(self, system_scale: f32) -> f32
        pub fn stepped_up(self) -> Self
        pub fn stepped_down(self) -> Self
        pub const fn percent(self) -> u32
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq
        impl PartialOrd

VAlign
    enum VAlign
        Auto = 0
        Top = 1
        Center = 2
        Bottom = 3
        Stretch = 4
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

ValueResponse
    struct ValueResponse<'a>
        pub response: Response<'a>
        pub changed: bool
        pub committed: bool
        impl<'a> Debug for ValueResponse<'a>

Vec2
    pub use glam::Vec2 as Vec2

Visibility
    enum Visibility
        Visible = 0
        Hidden = 1
        Collapsed = 2
        pub const fn is_visible(self) -> bool
        pub const fn is_collapsed(self) -> bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl Ord
        impl PartialEq
        impl PartialOrd

Vsync
    enum Vsync
        On
        Off
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

WidgetId
    struct WidgetId
        (_)
        pub fn from_hash(h: impl Hash) -> Self
        pub fn with(self, h: impl Hash) -> Self
        pub fn auto() -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq
        impl Pod
        impl Zeroable

WidgetLook
    struct WidgetLook
        pub background: Background
        pub text: TextStyleOverrides
        pub fn to_animated(&self, ambient_text: TextStyle) -> AnimatedLook
        impl Clone
        impl Debug
        impl Default
        impl PartialEq
        impl Serialize
        impl<'de> Deserialize<'de> for WidgetLook

WindowConfig
    struct WindowConfig
        pub title: String
        pub inner_size: Option<UVec2>
        pub min_inner_size: Option<UVec2>
        pub placement: WindowPlacement
        pub icon: Option<Image>
        pub app_id: Option<String>
        pub fn new(title: impl Into<String>) -> Self
        pub const fn with_inner_size(self, size: UVec2) -> Self
        pub const fn with_min_inner_size(self, size: UVec2) -> Self
        pub const fn with_position(self, position: IVec2) -> Self
        pub const fn with_placement(self, placement: WindowPlacement) -> Self
        pub const fn with_maximized(self, maximized: bool) -> Self
        pub fn with_icon(self, icon: Image) -> Self
        pub fn with_app_id(self, app_id: impl Into<String>) -> Self
        impl Clone
        impl Debug
        impl Default

WindowGeometry
    struct WindowGeometry
        pub inner_size: UVec2
        pub placement: WindowPlacement
        impl Clone
        impl Copy
        impl Debug
        impl Default

WindowPlacement
    struct WindowPlacement
        pub position: Option<IVec2>
        pub maximized: bool
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

WindowToken
    struct WindowToken
        (pub u64)
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

WinitHost  [feature: winit]
    struct WinitHost<T: 'static>
        // and private fields
        impl<T> WinitHost<T> where T: App + 'static
            pub fn builder(first_token: WindowToken) -> WinitHostBuilder<T>
            pub fn handle(&self) -> HostHandle<T>
            pub fn run(self) -> Result<(), WinitHostError>
        impl<T: 'static> Debug for WinitHost<T>

WinitHostBuilder  [feature: winit]
    struct WinitHostBuilder<T>
        // and private fields
        impl<T> WinitHostBuilder<T> where T: App + 'static
            pub fn window(self, window: WindowConfig) -> Self
            pub fn title(self, title: impl Into<String>) -> Self
            pub const fn fonts(self, scope: FontScope) -> Self
            pub const fn vsync(self, vsync: Vsync) -> Self
            pub const fn power_preference(self, preference: PowerPreference) -> Self
            pub const fn collect_gpu_stats(self, collect: bool) -> Self
            pub const fn pixel_snap(self, pixel_snap: bool) -> Self
            pub fn build(self, create_app: impl FnOnce(&mut Ui, HostHandle<T>) -> T + 'static) -> Result<WinitHost<T>, WinitHostError>
        impl<T: Debug> Debug for WinitHostBuilder<T>

WinitHostError  [feature: winit]
    #[non_exhaustive]
    enum WinitHostError
        CreateEventLoop { source: EventLoopError }
        RunEventLoop { source: EventLoopError }
        CreateWindow { token: WindowToken, source: OsError }
        Surface { token: WindowToken, source: SurfaceError }
        impl Debug
        impl Display
        impl Error

ZoomConfig
    struct ZoomConfig
        // and private fields
        pub const fn new(range: RangeInclusive<f32>, step: f32) -> Self
        pub const fn with_modifier(self, modifier: ZoomModifier) -> Self
        pub const fn with_pivot(self, pivot: ZoomPivot) -> Self
        impl Clone
        impl Debug
        impl Default

ZoomFactor
    struct ZoomFactor
        (_)
        pub const ONE: Self
        pub const fn new(factor: f32) -> Option<Self>
        pub fn from_wheel(step: f32, notches: f32) -> Self
        pub fn combine(self, rhs: Self) -> Self
        pub const fn get(self) -> f32
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq
        impl PartialOrd

ZoomModifier
    enum ZoomModifier
        Ctrl
        Always
        PinchOnly
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

ZoomPivot
    enum ZoomPivot
        Pointer
        Center
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

fmt
    macro fmt!

golden  [feature: golden]
    mod

golden::DiffReport  [feature: golden]
    struct DiffReport
        pub max_channel_delta: u8
        pub differing_pixels: u32
        pub diff_image: RgbaImage
        pub tolerance: Tolerance
        pub const fn passes(&self) -> bool
        impl Debug

golden::Goldens  [feature: golden]
    struct Goldens
        // and private fields
        pub fn new(root: impl Into<PathBuf>) -> Self
        pub fn with_adapter(self, adapter: impl Into<String>) -> Self
        pub fn orphans<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Vec<PathBuf>
        pub const fn with_tolerance(self, tolerance: Tolerance) -> Self
        pub fn assert_matches(&self, name: &str, actual: &RgbaImage)
        pub fn assert_same(&self, name: &str, actual: &RgbaImage, expected: &RgbaImage)
        impl Clone
        impl Debug

golden::Tolerance  [feature: golden]
    struct Tolerance
        pub max_delta: u8
        pub max_pixels: u32
        pub const EXACT: Self
        pub fn diff(self, actual: &RgbaImage, expected: &RgbaImage) -> DiffReport
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

golden::image  [feature: golden]
    pub use image as image

wgpu
    pub use wgpu as wgpu

widget
    mod

widget::Animatable
    trait Animatable: Clone + PartialEq + 'static
        fn lerp(a: Self, b: Self, t: f32) -> Self
        fn sub(self, other: Self) -> Self
        fn add(self, other: Self) -> Self
        fn scale(self, k: f32) -> Self
        fn magnitude_squared(self) -> f32
        fn settle_distance_squared(self) -> f32 { .. }
        fn zero() -> Self
        fn normalize_for_spring(&mut self, _target: &Self, _velocity: &mut Self) { .. }

widget::AnimationSlot
    struct AnimationSlot
        // and private fields
        pub const fn new(name: &'static str) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl From<&'static str>
        impl Hash
        impl PartialEq

widget::Caret
    struct Caret
        pub x: f32
        pub y_top: f32
        pub line_height: f32
        impl Clone
        impl Copy
        impl Debug
        impl PartialEq

widget::ConfigureWidget
    struct ConfigureWidget<'a>
        // and private fields
        pub fn id_salt(&mut self, key: impl Hash) -> &mut Self
        pub const fn id(&mut self, id: WidgetId) -> &mut Self
        pub const fn auto_id(&mut self) -> &mut Self
        pub fn size(&mut self, s: impl Into<SizeSpec>) -> &mut Self
        pub fn default_size(&mut self, s: impl Into<SizeSpec>) -> &mut Self
        pub fn min_size(&mut self, s: impl Into<Size>) -> &mut Self
        pub fn max_size(&mut self, s: impl Into<Size>) -> &mut Self
        pub fn padding(&mut self, p: impl Into<Spacing>) -> &mut Self
        pub fn margin(&mut self, m: impl Into<Spacing>) -> &mut Self
        pub const fn transform(&mut self, t: TranslateScale) -> &mut Self
        pub fn position(&mut self, p: impl Into<Vec2>) -> &mut Self
        pub fn grid_cell(&mut self, cell: impl Into<GridCell>) -> &mut Self
        pub fn adopt_placement(&mut self, from: &Widget) -> &mut Self
        pub fn gap(&mut self, g: f32) -> &mut Self
        pub fn line_gap(&mut self, g: f32) -> &mut Self
        pub const fn justify(&mut self, j: Justify) -> &mut Self
        pub const fn align(&mut self, a: Align) -> &mut Self
        pub const fn child_align(&mut self, a: Align) -> &mut Self
        pub const fn sense(&mut self, s: Sense) -> &mut Self
        pub fn add_sense(&mut self, s: Sense) -> &mut Self
        pub const fn disabled(&mut self, d: bool) -> &mut Self
        pub const fn focusable(&mut self, f: bool) -> &mut Self
        pub const fn tab_stop(&mut self, stop: bool) -> &mut Self
        pub const fn arrow_focus(&mut self, axis: Axis) -> &mut Self
        pub const fn tab_index(&mut self, index: i16) -> &mut Self
        pub const fn input_scope(&mut self, takes: KeyFilter) -> &mut Self
        pub const fn visibility(&mut self, v: Visibility) -> &mut Self
        pub const fn hidden(&mut self) -> &mut Self
        pub const fn collapsed(&mut self) -> &mut Self
        pub const fn clip(&mut self, mode: ClipMode) -> &mut Self
        pub const fn clip_rect(&mut self) -> &mut Self
        pub const fn clip_rounded(&mut self) -> &mut Self
        pub const fn default_id(&mut self, id: WidgetId) -> &mut Self
        pub fn default_padding(&mut self, p: impl Into<Spacing>) -> &mut Self
        pub fn default_margin(&mut self, m: impl Into<Spacing>) -> &mut Self
        pub const fn default_align(&mut self, a: Align) -> &mut Self
        pub fn default_gap(&mut self, g: f32) -> &mut Self
        pub fn default_min_size(&mut self, s: impl Into<Size>) -> &mut Self
        pub fn default_max_size(&mut self, s: impl Into<Size>) -> &mut Self
        pub fn default_clip(&mut self, mode: ClipMode) -> &mut Self
        impl<'a> Debug for ConfigureWidget<'a>

widget::ContentType
    enum ContentType
        Mask = 0
        Color = 1
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

widget::CurveShape
    struct CurveShape
        // and private fields
        pub const fn ramp(self, ramp: ColorRamp) -> Self
        pub const fn cap(self, cap: LineCap) -> Self
        impl Clone
        impl Debug
        impl Lower  (blanket)

widget::GlyphFont
    struct GlyphFont
        pub size: f32
        pub line_height: f32
        pub family: FontFamily
        pub weight: FontWeight
        pub slant: FontSlant
        pub const fn new(size: f32) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl PartialEq

widget::GlyphRasterKey
    struct GlyphRasterKey
        (_)
        impl Clone
        impl Copy
        impl Debug
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

widget::IconFit
    enum IconFit
        Contain
        Fill
        None
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl PartialEq

widget::IconShape
    struct IconShape
        // and private fields
        pub const fn at(self, rect: Rect) -> Self
        pub const fn fit(self, fit: IconFit) -> Self
        pub const fn tint(self, tint: RgbaF32) -> Self
        pub const fn desaturate(self, desaturate: bool) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Lower  (blanket)

widget::ImageShape
    struct ImageShape
        // and private fields
        pub const fn at(self, rect: Rect) -> Self
        pub const fn fit(self, fit: ImageFit) -> Self
        pub const fn min_filter(self, min_filter: ImageFilter) -> Self
        pub const fn mag_filter(self, mag_filter: ImageFilter) -> Self
        pub const fn downsample(self, downsample: ImageDownsample) -> Self
        pub const fn tint(self, tint: RgbaF32) -> Self
        impl Clone
        impl Debug
        impl Lower  (blanket)

widget::LineCap
    enum LineCap
        Butt = 0
        Square = 1
        Round = 2
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

widget::LineJoin
    enum LineJoin
        Miter = 0
        Bevel = 1
        Round = 2
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl DockTab  (blanket)
        impl Eq
        impl Hash
        impl PartialEq

widget::LookPlan
    struct LookPlan
        // and private fields
        pub fn apply(self, ui: &mut Ui, widget: &mut Widget) -> AnimatedLook
        impl Debug

widget::Lower
    trait Lower: LowerShape
        impl<T: LowerShape> Lower for T

widget::Mesh
    struct Mesh
        // and private fields
        pub const fn new() -> Self
        pub fn with_capacity(vertices: usize, indices: usize) -> Self
        pub fn clear(&mut self)
        pub fn is_noop(&self) -> bool
        pub fn content_hash(&self) -> u64
        pub fn vertex(&mut self, pos: Vec2, color: impl Into<SrgbaU8>) -> u32
        pub fn triangle(&mut self, a: u32, b: u32, c: u32)
        pub fn append(&mut self, other: &Mesh)
        pub fn bbox(&self) -> Rect
        pub fn filled_triangle(a: Vec2, b: Vec2, c: Vec2, color: impl Into<SrgbaU8>) -> Self
        pub fn filled_polygon(points: &[Vec2], color: impl Into<SrgbaU8>) -> Self
        impl Clone
        impl Debug
        impl Default

widget::MeshShape
    struct MeshShape<'a>
        // and private fields
        pub const fn at(self, rect: Rect) -> Self
        pub const fn tint(self, tint: RgbaF32) -> Self
        impl Lower  (blanket)
        impl<'a> Clone for MeshShape<'a>
        impl<'a> Debug for MeshShape<'a>

widget::MeshVertex
    struct MeshVertex
        pub pos: Vec2
        pub color: SrgbaU8
        pub fn new(pos: Vec2, color: impl Into<SrgbaU8>) -> Self
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl PartialEq
        impl Pod
        impl Zeroable

widget::PaintAnimation
    struct PaintAnimation
        pub channel: PaintChannel
        pub timing: PaintTiming
        pub curve: PaintCurve
        pub fn alpha(from: f32, to: f32) -> Self
        pub fn turn(from: f32, to: f32) -> Self
        pub const fn with_alpha(self, from: f32, to: f32) -> Self
        pub const fn with_turn(self, from: f32, to: f32) -> Self
        pub const fn with_period(self, period: Duration) -> Self
        pub const fn with_started_at(self, at: Duration) -> Self
        pub const fn with_repeat(self, repeat: PaintRepeat) -> Self
        pub const fn with_steps(self, n: u32) -> Self
        pub const fn with_curve(self, curve: PaintCurve) -> Self
        impl Clone
        impl Copy
        impl Debug

widget::PaintChannel
    struct PaintChannel
        pub alpha: Option<(f32, f32)>
        pub turn: Option<(f32, f32)>
        impl Clone
        impl Copy
        impl Debug
        impl PartialEq

widget::PaintCurve
    type PaintCurve = fn(f32) -> f32

widget::PaintRepeat
    enum PaintRepeat
        Once
        Forever
        Settle(Duration)
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

widget::PaintSteps
    enum PaintSteps
        Continuous
        Steps(NonZeroU32)
        impl Clone
        impl Copy
        impl Debug
        impl Eq
        impl PartialEq

widget::PaintTiming
    struct PaintTiming
        pub started_at: Duration
        pub period: Duration
        pub repeat: PaintRepeat
        pub steps: PaintSteps
        impl Clone
        impl Copy
        impl Debug
        impl PartialEq

widget::PlacedGlyph
    struct PlacedGlyph
        pub raster_key: GlyphRasterKey
        pub x: i32
        pub y: i32
        impl Clone
        impl Copy
        impl Debug

widget::PolylineShape
    struct PolylineShape<'a>
        // and private fields
        pub const fn per_point(self, colors: &'a [RgbaF32]) -> Self
        pub const fn per_segment(self, colors: &'a [RgbaF32]) -> Self
        pub const fn cap(self, cap: LineCap) -> Self
        pub const fn join(self, join: LineJoin) -> Self
        impl Lower  (blanket)
        impl<'a> Clone for PolylineShape<'a>
        impl<'a> Debug for PolylineShape<'a>

widget::RasterImage
    struct RasterImage<'a>
        pub content: ContentType
        pub size: UVec2
        pub bearing: IVec2
        pub data: &'a [u8]
        impl<'a> Clone for RasterImage<'a>
        impl<'a> Copy for RasterImage<'a>
        impl<'a> Debug for RasterImage<'a>

widget::RectShape
    struct RectShape
        // and private fields
        pub fn fill(self, fill: impl Into<Brush>) -> Self
        pub const fn border(self, border: Stroke) -> Self
        pub fn corners(self, corners: impl Into<Corners>) -> Self
        impl Clone
        impl Debug
        impl Lower  (blanket)

widget::ShadowShape
    struct ShadowShape
        // and private fields
        pub const fn at(self, rect: Rect) -> Self
        pub fn corners(self, corners: impl Into<Corners>) -> Self
        impl Clone
        impl Debug
        impl Lower  (blanket)

widget::Shape
    struct Shape
        pub const fn rect(rect: Rect) -> RectShape
        pub const fn owner_rect() -> RectShape
        pub const fn windowed_rect(rect: Rect) -> RectShape
        pub const fn owner_windowed_rect() -> RectShape
        pub const fn triangle(a: Vec2, b: Vec2, c: Vec2) -> TriangleShape
        pub const fn line(a: Vec2, b: Vec2, stroke: Stroke) -> CurveShape
        pub fn polyline(points: &[Vec2], stroke: Stroke) -> PolylineShape<'_>
        pub const fn cubic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, stroke: Stroke) -> CurveShape
        pub const fn quadratic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, stroke: Stroke) -> CurveShape
        pub const fn arc(center: Vec2, radius: f32, start_angle: f32, sweep: f32, stroke: Stroke) -> CurveShape
        pub const fn circle(center: Vec2, radius: f32, stroke: Stroke) -> CurveShape
        pub const fn text(text: InternedStr, font: GlyphFont) -> TextShape
        pub const fn shadow(shadow: Shadow) -> ShadowShape
        pub fn image(handle: ImageHandle) -> ImageShape
        pub fn icon(handle: IconHandle) -> IconShape
        pub const fn mesh(mesh: &Mesh) -> MeshShape<'_>
        impl Clone
        impl Copy
        impl Debug

widget::Span
    struct Span
        pub start: u32
        pub len: u32
        pub const fn new(start: u32, len: u32) -> Self
        pub const fn range(self) -> Range<usize>
        impl Clone
        impl Copy
        impl Debug
        impl Default
        impl Eq
        impl From<Range<u32>>
        impl From<Range<usize>>
        impl From<Span> for Range<u32>
        impl From<Span> for Range<usize>
        impl PartialEq
        impl Pod
        impl Zeroable

widget::TextGlyphs
    struct TextGlyphs<'a>
        // and private fields
        pub fn line(&mut self, text: &str, font: GlyphFont, scale: f32, out: &mut Vec<PlacedGlyph>)
        pub fn measure(&mut self, text: &str, font: GlyphFont) -> Size
        pub fn rasterize(&mut self, glyph: GlyphRasterKey) -> Option<RasterImage<'_>>
        impl<'a> Debug for TextGlyphs<'a>

widget::TextProbe
    struct TextProbe<'a>
        // and private fields
        pub const fn size(&self) -> Size
        pub fn text_hash(&self) -> Option<NonZeroU64>
        pub fn hash_of(text: &str) -> NonZeroU64
        pub fn caret_at(&self, byte_offset: usize) -> Caret
        pub fn byte_at(&self, x: f32, y: f32) -> usize
        pub fn selection_rects(&self, range: Range<usize>, out: impl FnMut(Rect))
        impl<'a> Debug for TextProbe<'a>

widget::TextRun
    struct TextRun<'a>
        pub text: &'a str
        pub font: GlyphFont
        pub wrap: TextWrap
        pub align: Align
        pub max_width: Option<f32>
        impl<'a> Clone for TextRun<'a>
        impl<'a> Copy for TextRun<'a>
        impl<'a> Debug for TextRun<'a>

widget::TextShape
    struct TextShape
        // and private fields
        pub const fn at_origin(self, origin: Vec2) -> Self
        pub const fn color(self, color: RgbaF32) -> Self
        pub const fn wrap(self, wrap: TextWrap) -> Self
        pub const fn align(self, align: Align) -> Self
        pub const fn family(self, family: FontFamily) -> Self
        pub const fn weight(self, weight: FontWeight) -> Self
        pub const fn slant(self, slant: FontSlant) -> Self
        impl Clone
        impl Debug
        impl Lower  (blanket)

widget::ThemeDefaults
    trait ThemeDefaults: Configure
        fn default_id(self, id: WidgetId) -> Self { .. }
        fn default_size(self, s: impl Into<SizeSpec>) -> Self { .. }
        fn default_padding(self, p: impl Into<Spacing>) -> Self { .. }
        fn default_margin(self, m: impl Into<Spacing>) -> Self { .. }
        fn default_align(self, a: Align) -> Self { .. }
        fn default_gap(self, g: f32) -> Self { .. }
        fn default_min_size(self, s: impl Into<Size>) -> Self { .. }
        fn default_max_size(self, s: impl Into<Size>) -> Self { .. }
        fn default_clip(self, mode: ClipMode) -> Self { .. }
        impl<T: Configure> ThemeDefaults for T

widget::ThemeSlot
    trait ThemeSlot
        type Pick: Copy
        fn look(&self, response: &ResponseState, pick: Self::Pick) -> &WidgetLook
        fn defaults(&self) -> SlotDefaults
        fn plan(&self, response: &ResponseState, pick: Self::Pick, text: TextStyle) -> LookPlan { .. }

widget::TriangleShape
    struct TriangleShape
        // and private fields
        pub const fn fill(self, fill: RgbaF32) -> Self
        pub const fn border(self, border: Stroke) -> Self
        pub const fn radius(self, radius: f32) -> Self
        impl Clone
        impl Debug
        impl Lower  (blanket)

widget::Widget
    struct Widget
        // and private fields
        pub fn leaf() -> Self
        pub fn hstack() -> Self
        pub fn vstack() -> Self
        pub fn stack(axis: Axis) -> Self
        pub fn wrap_hstack() -> Self
        pub fn wrap_vstack() -> Self
        pub fn zstack() -> Self
        pub fn canvas() -> Self
        pub fn grid() -> Self
        pub fn resolve(&mut self, ui: &mut Ui) -> WidgetId
        pub fn response(&mut self, ui: &mut Ui) -> ResponseState
        pub fn key_pressed(&mut self, ui: &mut Ui, shortcut: Shortcut) -> bool
        pub fn record<R>(self, ui: &mut Ui, chrome: Option<&Background>, body: impl FnOnce(&mut Ui) -> R) -> R
        pub fn show<'a, R>(self, ui: &'a mut Ui, chrome: Option<&Background>, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'a, R>
        pub const fn authored_size(&self) -> Option<SizeSpec>
        pub const fn authored_min_size(&self) -> Option<Size>
        pub const fn authored_max_size(&self) -> Option<Size>
        pub const fn authored_padding(&self) -> Option<Spacing>
        pub const fn authored_margin(&self) -> Option<Spacing>
        pub const fn authored_transform(&self) -> TranslateScale
        pub const fn authored_position(&self) -> Vec2
        pub const fn authored_grid_cell(&self) -> GridCell
        pub fn authored_gap(&self) -> Option<f32>
        pub fn authored_line_gap(&self) -> Option<f32>
        pub const fn authored_justify(&self) -> Justify
        pub const fn authored_align(&self) -> Align
        pub const fn authored_child_align(&self) -> Align
        pub const fn authored_sense(&self) -> Sense
        pub const fn authored_disabled(&self) -> bool
        pub const fn authored_focusable(&self) -> bool
        pub const fn authored_tab_stop(&self) -> bool
        pub const fn authored_arrow_focus(&self) -> Option<Axis>
        pub const fn authored_tab_index(&self) -> i16
        pub const fn authored_input_scope(&self) -> KeyFilter
        pub const fn authored_visibility(&self) -> Visibility
        pub const fn authored_clip(&self) -> Option<ClipMode>
        pub fn grid_tracks(&mut self, ui: &mut Ui, rows: &[Track], cols: &[Track])
        impl Configure
        impl Debug
        impl ThemeDefaults  (blanket)

widget::curves
    mod

widget::curves::linear
    const fn linear(t: f32) -> f32

widget::curves::sine
    fn sine(t: f32) -> f32

widget::curves::square
    const fn square(t: f32) -> f32

widget::domain
    mod

widget::domain::EPS
    const EPS: f32 = 1.0e-4

widget::domain::MAX_GAP
    const MAX_GAP: f32 = F16x4::MAX_LANE

widget::domain::angle
    const fn angle(v: f32) -> f32

widget::domain::approx_eq
    const fn approx_eq(a: f32, b: f32) -> bool

widget::domain::band_fraction
    const fn band_fraction(pos: f32, extent: f32, band: f32) -> f32

widget::domain::color
    const fn color(c: RgbaF32) -> RgbaF32

widget::domain::count
    const fn count(n: u32) -> u32

widget::domain::extent
    const fn extent(v: f32) -> f32

widget::domain::f64
    mod

widget::domain::f64::is_positive
    const fn is_positive(v: f64) -> bool

widget::domain::f64::is_range
    const fn is_range(r: &RangeInclusive<f64>) -> bool

widget::domain::f64::positive
    const fn positive(v: f64) -> f64

widget::domain::f64::range
    const fn range(r: RangeInclusive<f64>) -> RangeInclusive<f64>

widget::domain::fraction
    const fn fraction(v: f32) -> f32

widget::domain::fraction_or
    const fn fraction_or(v: f32, fallback: f32) -> f32

widget::domain::gap
    const fn gap(v: f32) -> f32

widget::domain::index
    const fn index(i: usize, len: usize) -> Option<usize>

widget::domain::is_angle
    const fn is_angle(v: f32) -> bool

widget::domain::is_approx_zero
    const fn is_approx_zero(v: f32) -> bool

widget::domain::is_color
    const fn is_color(c: RgbaF32) -> bool

widget::domain::is_count
    const fn is_count(n: u32) -> bool

widget::domain::is_extent
    const fn is_extent(v: f32) -> bool

widget::domain::is_fraction
    const fn is_fraction(v: f32) -> bool

widget::domain::is_gap
    const fn is_gap(v: f32) -> bool

widget::domain::is_invisible
    const fn is_invisible(v: f32) -> bool

widget::domain::is_length
    const fn is_length(v: f32) -> bool

widget::domain::is_offset
    const fn is_offset(v: f32) -> bool

widget::domain::is_positive
    const fn is_positive(v: f32) -> bool

widget::domain::is_power_of_two_in
    const fn is_power_of_two_in(n: u32, max: u32) -> bool

widget::domain::length
    const fn length(v: f32) -> f32

widget::domain::length_at_least
    const fn length_at_least(v: f32, min: f32) -> f32

widget::domain::offset
    const fn offset(v: f32) -> f32

widget::domain::positive
    const fn positive(v: f32) -> f32

widget::domain::power_of_two_in
    const fn power_of_two_in(n: u32, max: u32) -> u32

widget::domain::share_of
    const fn share_of(n: f32, d: f32) -> f32

widget::domain::turn
    const fn turn(v: f32) -> f32

widget::domain::vec2
    mod

widget::domain::vec2::approx_eq
    const fn approx_eq(a: Vec2, b: Vec2) -> bool

widget::domain::vec2::band_fraction
    const fn band_fraction(pos: Vec2, extent: Vec2, band: Vec2) -> Vec2

widget::domain::vec2::fraction_or
    const fn fraction_or(v: Vec2, fallback: Vec2) -> Vec2

widget::domain::vec2::is_length
    const fn is_length(v: Vec2) -> bool

widget::domain::vec2::is_offset
    const fn is_offset(v: Vec2) -> bool

widget::domain::vec2::length
    const fn length(v: Vec2) -> Vec2

widget::domain::vec2::length_at_least
    const fn length_at_least(v: Vec2, min: Vec2) -> Vec2

widget::domain::vec2::offset
    const fn offset(v: Vec2) -> Vec2

```

## Public but not exported

Items declared `pub` that no path from the crate root reaches. A caller cannot name
them; each one is either dead surface or a type that leaks through a signature.

```text
(none)
```

## Rustdoc JSON the renderer does not know

```text
(none)
```
