# Public API surface

Every item the crate exports, rendered as its declaration from rustdoc JSON
(`cargo drive surface`), sorted by path. The surface is the union of a build with
no features and one build per public feature; a tag names the builds an item or a
line needs or loses. `internals` is the crate's own test surface and is left out.

```text
ActError
    struct ActError: Clone, Debug, Display, Error, PartialEq
        pub ran: usize
        pub error: PlayError

Action
    enum Action<X = Infallible>
        Move(Locator<X>)
        Click { at: Locator<X>, button: PointerButton, mods: ShortcutMods }
        DoubleClick(Locator<X>)
        Drag { from: Locator<X>, to: Locator<X>, button: PointerButton, mods: ShortcutMods, steps: u32 }
        Scroll { at: Locator<X>, delta: Vec2 }
        Pinch { at: Locator<X>, factor: f32 }
        Key(Shortcut)
        Type(String)
        Wait(Duration)
        impl Action
            pub const DRAG_STEPS: u32
            pub fn click(at: impl Into<Locator>) -> Self
            pub const fn key(shortcut: Shortcut) -> Self
        impl<X: Clone> Clone for Action<X>
        impl<X: Debug> Debug for Action<X>
        impl<X: PartialEq> PartialEq for Action<X>

Align
    struct Align: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
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

AllowedSplits
    enum AllowedSplits: Clone, Copy, Debug, Default, Eq, PartialEq
        All
        Row
        Col
        None
        pub fn allows(self, side: SplitSide) -> bool

Anchor
    struct Anchor: Clone, Copy, Debug
        pub const fn at_point(point: Vec2) -> Self
        pub const fn above(rect: Rect) -> Self
        pub const fn below(rect: Rect) -> Self
        pub const fn left_of(rect: Rect) -> Self
        pub const fn right_of(rect: Rect) -> Self
        pub const fn with_align(self, align: AnchorAlign) -> Self
        pub const fn with_gap(self, px: f32) -> Self

AnchorAlign
    enum AnchorAlign: Clone, Copy, Debug, Default, Eq, PartialEq
        Start
        Center
        End

AnimatedLook
    struct AnimatedLook: Animatable, Clone, Debug, Default, PartialEq
        pub chrome: Chrome
        pub text: TextStyle

AnimationSpec
    struct AnimationSpec: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub const FAST: Self
        pub const MEDIUM: Self
        pub const SNAP: Self
        pub const SPRING: Self
        pub const fn duration(length: Duration, ease: Easing) -> Self
        pub fn spring(stiffness: f32, damping: f32) -> Self
        pub const fn is_instant(self) -> bool

App
    trait App
        fn update(&mut self, _window: WindowToken, _ui: &Ui) { .. }
        fn record(&mut self, window: WindowToken, ui: &mut Ui)
        impl<F: FnMut(&mut Ui)> App for F

Axis
    enum Axis: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        X
        Y

BackdropFilter
    struct BackdropFilter: Animatable, Clone, Copy, Debug, Default, PartialEq, Serialize
        pub blur: f32
        pub saturation: f32
        pub noise: f32
        pub max_luminance: Option<f32>
        pub lens: Option<Lens>
        pub const NONE: Self
        pub const fn blur(sigma: f32) -> Self
        pub const fn frosted(sigma: f32) -> Self
        pub const fn glass(lens: Lens) -> Self
        pub const fn with_blur(self, sigma: f32) -> Self
        pub const fn with_saturation(self, saturation: f32) -> Self
        pub const fn with_noise(self, noise: f32) -> Self
        pub const fn with_max_luminance(self, luminance: f32) -> Self
        pub const fn with_lens(self, lens: Lens) -> Self
        pub const fn is_noop(&self) -> bool
        impl<'de> Deserialize<'de> where BackdropFilter: Default

BarMode
    enum BarMode: Clone, Copy, Debug, Default, Eq, PartialEq
        Reserved
        Overlay
        Hidden

BatchKind
    enum BatchKind: Clone, Copy, Debug, Eq, PartialEq
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

Block
    struct Block: Configure, Debug, ThemeDefaults (blanket)
        pub fn new() -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub const fn default_chrome(self, chrome: Chrome) -> Self

Brush
    enum Brush: Animatable, Clone, Debug, Default, Deserialize<'de>, From<Gradient<ConicGeometry>>, From<Gradient<LinearGeometry>>, From<Gradient<RadialGeometry>>, From<GradientBuilder<ConicGeometry>>, From<GradientBuilder<LinearGeometry>>, From<GradientBuilder<RadialGeometry>>, From<RgbaF32>, From<SrgbaU8>, PartialEq, Serialize
        Solid(RgbaF32)
        Linear(LinearGradient)
        Radial(RadialGradient)
        Conic(ConicGradient)
        pub const TRANSPARENT: Self
        pub const fn is_noop(&self) -> bool
        pub const fn as_solid(&self) -> Option<RgbaF32>

Button
    struct Button<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a ButtonTheme>>) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub const fn text_wrap(self, wrap: TextWrap) -> Self
        pub const fn text_align(self, a: Align) -> Self
        pub const fn selected(self, selected: bool) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>
        pub fn show_with<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &TextStyle) -> R) -> InnerResponse<'_, R>

ButtonPhase
    enum ButtonPhase: Clone, Copy, Debug, Default, Eq, PartialEq
        Idle
        Down { count: u8 }
        Held
        Up { click: Option<u8> }

ButtonState
    struct ButtonState: Clone, Copy, Debug, Default, PartialEq
        pub phase: ButtonPhase
        pub drag: Drag
        pub const fn held(self) -> bool
        pub const fn clicked(self) -> bool
        pub const fn released(self) -> bool
        pub const fn press_count(self) -> u8
        pub const fn click_count(self) -> u8
        pub const fn double_clicked(self) -> bool

ButtonTheme
    struct ButtonTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize, ThemeSlot
        pub looks: StatefulLook
        pub selected: StatefulLook
        pub defaults: SlotDefaults
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self
        pub fn menu_button(p: &Palette, m: &Metrics) -> Self

CheckState
    enum CheckState: Clone, Copy, Debug, Eq, From<bool>, PartialEq
        Unchecked
        Checked

Checkbox
    struct Checkbox<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(value: &'a mut bool) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ToggleTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>

Chrome
    struct Chrome: Animatable, Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub fill: Brush
        pub border: Stroke
        pub corners: Corners
        pub shadow: Shadow
        pub backdrop_filter: BackdropFilter
        pub const NONE: Self
        pub const fn is_noop(&self) -> bool
        pub fn fill<I: Into<Brush>>(brush: I) -> Self
        pub fn rounded<I: Into<Brush>>(brush: I, corners: Corners) -> Self
        pub const fn with_border(self, border: Stroke) -> Self
        pub const fn with_shadow(self, shadow: Shadow) -> Self
        pub const fn with_backdrop_filter(self, filter: BackdropFilter) -> Self

ClickOutside
    enum ClickOutside: Clone, Copy, Debug, Eq, PartialEq
        Block
        Dismiss
        PassThrough

ClipMode
    enum ClipMode: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, Ord, PartialEq, PartialOrd, Serialize
        None = 0
        Rect = 1
        Rounded = 2
        pub const fn is_clip(self) -> bool

Clipboard
    struct Clipboard: Clone, Debug
        pub fn text(&self) -> Result<String, ClipboardUnavailable>
        pub fn set_text(&self, text: &str) -> Result<(), ClipboardUnavailable>
        pub fn set_image(&self, image: &Image) -> Result<(), ClipboardUnavailable>

ClipboardUnavailable
    struct ClipboardUnavailable: Clone, Copy, Debug, Display, Eq, Error, PartialEq

CloseHandle
    struct CloseHandle: Debug, Default
        pub fn close(&self)
        pub const fn requested(&self) -> bool

CloseShown
    enum CloseShown: Clone, Copy, Debug, Default, Deserialize<'de>, Eq, PartialEq, Serialize
        Always
        SelectedOrHovered

ColorButton
    struct ColorButton<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(picker: ColorPicker<'a>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>

ColorCoords
    enum ColorCoords: Clone, Copy, Debug, Default, PartialEq
        Okhsv(Okhsv)
        Hsv(Hsv)
        pub fn from_color(color: RgbaF32, model: ColorModel, fallback_hue: f32) -> Self
        pub const fn model(self) -> ColorModel
        pub fn to_color(self) -> RgbaF32
        pub fn with_model(self, model: ColorModel) -> Self
        pub const fn hue(self) -> f32
        pub const fn saturation(self) -> f32
        pub const fn value(self) -> f32
        pub const fn set_hue(&mut self, h: f32)
        pub const fn set_saturation(&mut self, s: f32)
        pub const fn set_value(&mut self, v: f32)

ColorField
    struct ColorField<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(coords: &'a mut ColorCoords) -> Self
        pub const fn texel_size(self, n: u32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>

ColorModel
    enum ColorModel: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Okhsv
        Hsv
        pub const ALL: [Self; 2]
        pub const fn label(self) -> &'static str
        pub fn slice(self, hue: f32) -> HueSlice

ColorPicker
    struct ColorPicker<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(color: &'a mut RgbaF32) -> Self
        pub const fn alpha(self, on: bool) -> Self
        pub const fn model(self, model: ColorModel) -> Self
        pub const fn history(self, on: bool) -> Self
        pub const fn swatches(self, colors: &'a [RgbaF32]) -> Self
        pub const fn texel_size(self, n: u32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub const fn color(&self) -> RgbaF32
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>

ColorPickerTheme
    struct ColorPickerTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
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
        pub popup: Chrome
        pub popup_padding: Spacing
        pub value: DragValueTheme
        pub hex: TextEditTheme
        pub label: TextStyleOverrides
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

ColorRamp
    struct ColorRamp: Clone, Copy, Debug, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        pub stops: GradientStops
        pub interpolation: Interpolation
        pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self
        pub fn two_stop(c0: RgbaF32, c1: RgbaF32) -> Self
        pub fn color_at(&self, t: f32) -> RgbaF32
        pub const fn with_interpolation(self, interpolation: Interpolation) -> Self
        pub const fn is_noop(&self) -> bool

ColorStrip
    struct ColorStrip<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn for_hue(coords: &'a mut ColorCoords) -> Self
        pub fn for_alpha(color: &'a mut RgbaF32) -> Self
        pub const fn texel_size(self, n: u32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>

ColorSwatch
    struct ColorSwatch<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(color: RgbaF32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>

ComboBox
    struct ComboBox<'a, S, L>: Configure, ThemeDefaults (blanket)
        impl<'a, S: AsRef<str>> ComboBox<'a, S, fn(&S) -> &str>
            pub fn new(selected: &'a mut usize, options: &'a [S]) -> Self
        impl<'a, S, L: Fn(&S) -> &str> ComboBox<'a, S, L>
            pub fn labeled(selected: &'a mut usize, options: &'a [S], label: L) -> Self
            pub fn style(self, s: impl Into<Option<&'a ComboBoxTheme>>) -> Self
            pub fn button_style(self, s: impl Into<Option<&'a ButtonTheme>>) -> Self
            pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl ComboBox<'static, &'static str, fn(&&'static str) -> &'static str>
            pub fn open(ui: &mut Ui, id: WidgetId)
            pub fn close(ui: &mut Ui, id: WidgetId)
            pub fn is_open(ui: &Ui, id: WidgetId) -> bool
        impl<'a, S: Debug, L: Debug> Debug for ComboBox<'a, S, L>

ComboBoxTheme
    struct ComboBoxTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub gap: f32
        pub arrow_size: Vec2
        pub arrow_width: f32
        pub const fn from_metrics(m: &Metrics) -> Self

Configure
    trait Configure: Sized
        fn configure(&mut self) -> Configurator<'_>
        fn id_salt(self, key: impl Hash) -> Self { .. }
        fn id(self, id: WidgetId) -> Self { .. }
        fn auto_id(self) -> Self { .. }
        fn size(self, s: impl Into<SizeSpec>) -> Self { .. }
        fn min_size(self, s: impl Into<Size>) -> Self { .. }
        fn max_size(self, s: impl Into<Size>) -> Self { .. }
        fn padding(self, p: impl Into<Spacing>) -> Self { .. }
        fn margin(self, m: impl Into<Spacing>) -> Self { .. }
        fn transform(self, t: TranslateScale) -> Self { .. }
        fn pos(self, p: impl Into<Vec2>) -> Self { .. }
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
        fn tab_idx(self, idx: i16) -> Self { .. }
        fn input_scope(self, takes: KeyFilter) -> Self { .. }
        fn visibility(self, v: Visibility) -> Self { .. }
        fn hidden(self) -> Self { .. }
        fn collapsed(self) -> Self { .. }
        fn clip(self, mode: ClipMode) -> Self { .. }
        fn clip_rect(self) -> Self { .. }
        fn clip_rounded(self) -> Self { .. }

ConicGeometry
    struct ConicGeometry: Clone, Copy, Debug, Deserialize<'de>, GradientGeometry (blanket), PartialEq, Serialize
        pub center: Vec2
        pub start_angle: f32

ConicGradient
    type ConicGradient = Gradient<ConicGeometry>

ConicGradientBuilder
    type ConicGradientBuilder = GradientBuilder<ConicGeometry>

ContextMenu
    struct ContextMenu<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn for_id(for_id: WidgetId) -> Self
        pub fn on(snapshot: &ResponseSnapshot) -> Self
        pub fn style(self, s: impl Into<Option<&'a ContextMenuTheme>>) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &CloseHandle) -> R) -> OverlayResponse<Option<R>>
        pub fn open(ui: &mut Ui, for_id: WidgetId, point: Vec2)
        pub fn close(ui: &mut Ui, for_id: WidgetId)
        pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub const fn default_chrome(self, chrome: Chrome) -> Self

ContextMenuTheme
    struct ContextMenuTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub panel: Chrome
        pub padding: Spacing
        pub min_width: f32
        pub gap: f32
        pub item: MenuItemTheme
        pub separator: SeparatorTheme
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

Corners
    struct Corners: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Pod, Serialize, Zeroable
        pub const ZERO: Self
        pub fn to_array(self) -> [f32; 4]
        pub fn all(r: f32) -> Self
        pub fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self
        pub fn top(r: f32) -> Self
        pub fn bottom(r: f32) -> Self
        pub fn left(r: f32) -> Self
        pub fn right(r: f32) -> Self
        pub fn scaled_by(self, factor: f32) -> Self
        pub const fn is_approx_zero(&self) -> bool
        impl<T: Number, B: Number> From<(T, B)> for Corners
        impl<T: Number> From<T> for Corners
        impl<TL: Number, TR: Number, BR: Number, BL: Number> From<(TL, TR, BR, BL)> for Corners

CursorIcon
    enum CursorIcon: Clone, Copy, Debug, Default, Eq, PartialEq
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

DebugOverlayConfig
    struct DebugOverlayConfig: Clone, Copy, Debug, Default, Eq, PartialEq
        pub damage_rect: bool
        pub dim_undamaged: bool
        pub frame_stats: bool

Depth
    enum Depth: Debug
        Record
        Compose
        Gpu(Gpu)

DeviceFeatures
    struct DeviceFeatures: BitOr, Clone, Copy, Debug, Default, Display, DockTab (blanket), Eq, Hash, PartialEq
        pub const IMMEDIATES: Self
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

DeviceRequirements
    struct DeviceRequirements: Clone, Debug
        pub features: Features
        pub limits: Limits
        pub const FEATURES: Features
        pub const GPU_TIMING_FEATURES: Features
        pub fn negotiate(adapter: &Adapter, optional: Features) -> Result<Self, UnmetRequirements>
        pub fn met_by(device: &Device) -> Result<(), UnmetRequirements>

Display
    struct Display: Clone, Copy, Debug, Default, PartialEq
        pub physical: UVec2
        pub system_scale: f32
        pub user_scale: UserScale
        pub pixel_snap: bool
        pub refresh_millihertz: Option<u32>
        pub const fn scale_factor(&self) -> f32
        pub fn logical_size(&self) -> Size
        pub fn system_logical_size(&self) -> Size
        pub fn logical_rect(&self) -> Rect
        pub fn raster_eq(&self, other: &Self) -> bool

DockDrop
    enum DockDrop: Clone, Copy, Debug, Deserialize<'de>, Eq, PartialEq, Serialize
        Into { group: TabGroupId, idx: usize }
        Split { group: TabGroupId, side: SplitSide }

DockError
    enum DockError<T>
        NonCanonical
        NodeOutOfRange { idx: u32 }
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
        impl<'de, T> Deserialize<'de> where T: Deserialize<'de>
        impl<T: Clone> Clone for DockNode<T>
        impl<T: Debug> Debug for DockNode<T>
        impl<T: PartialEq> PartialEq for DockNode<T>
        impl<T> Serialize where T: Serialize

DockOperation
    enum DockOperation<T>
        ActivateTab { tab: T }
        OpenTab { tab: T }
        CloseTab { tab: T }
        MoveTab { tab: T, to: DockDrop }
        SetRatio { split: DockPath, ratio: f32 }
        FocusPane { group: TabGroupId }
        impl<'de, T> Deserialize<'de> where T: Deserialize<'de>
        impl<T: Clone> Clone for DockOperation<T>
        impl<T: Copy> Copy for DockOperation<T>
        impl<T: Debug> Debug for DockOperation<T>
        impl<T: PartialEq> PartialEq for DockOperation<T>
        impl<T> Serialize where T: Serialize

DockPath
    struct DockPath: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        pub const ROOT: Self
        pub fn first(self) -> Self
        pub fn second(self) -> Self

DockSplit
    struct DockSplit: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub const fn direction(self) -> SplitDirection
        pub const fn ratio(self) -> f32
        pub const fn first(self) -> NodeIdx
        pub const fn second(self) -> NodeIdx

DockState
    struct DockState<T>
        impl<T: DockTab> DockState<T>
            pub const ROOT: NodeIdx
            pub const RATIO_MIN: f32
            pub const RATIO_MAX: f32
            pub fn new(seed: impl Hash, pinned: T) -> Self
            pub fn with_max_depth(self, depth: u32) -> Self
            pub const fn with_allowed_splits(self, allowed: AllowedSplits) -> Self
            pub const fn with_edge_fraction(self, fraction: f32) -> Self
            pub const fn pinned(&self) -> T
            pub const fn seed(&self) -> u64
            pub const fn allowed_splits(&self) -> AllowedSplits
            pub const fn edge_fraction(&self) -> f32
            pub const fn focused(&self) -> TabGroupId
            pub fn node(&self, idx: NodeIdx) -> &DockNode<T>
            pub fn groups(&self) -> impl Iterator<Item = &TabGroup<T>>
            pub fn all_tabs(&self) -> impl Iterator<Item = T> + '_
            pub fn active_tabs(&self) -> impl Iterator<Item = T> + '_
            pub fn primary(&self) -> &TabGroup<T>
            pub fn find_tab(&self, tab: T) -> Option<TabAddress>
            pub fn group(&self, id: TabGroupId) -> Option<&TabGroup<T>>
            pub fn apply(&mut self, operation: DockOperation<T>)
            pub fn retain_tabs(&mut self, keep: impl FnMut(T) -> bool)
            pub fn can_split(&self, group: TabGroupId) -> bool
        impl<'de, T> Deserialize<'de> where T: DockTab + Deserialize<'de>
        impl<T: Clone> Clone for DockState<T>
        impl<T: Debug> Debug for DockState<T>
        impl<T: PartialEq> PartialEq for DockState<T>
        impl<T> Serialize where T: Serialize

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
        fn tab_content(&mut self, ui: &mut Ui, _tab: Self::Tab, chip: &TabChip) { .. }
        fn has_tab_menu(&mut self, _tab: Self::Tab) -> bool { .. }
        fn tab_menu(&mut self, _ui: &mut Ui, _menu: DockTabMenu<'_, Self::Tab>) { .. }

DockTheme
    struct DockTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub preview_fill: RgbaF32
        pub preview_stroke: Stroke
        pub preview_radius: f32
        pub ghost: WidgetLook
        pub ghost_padding: Spacing
        pub ghost_offset: Vec2
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

DockView
    struct DockView<'a, T>: Configure, ThemeDefaults (blanket)
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
        impl<'a, T: Debug> Debug for DockView<'a, T>

Drag
    enum Drag: Clone, Copy, Debug, Default, PartialEq
        None
        Started { delta: Vec2 }
        Active { delta: Vec2 }
        Stopped
        pub const fn delta(self) -> Option<Vec2>
        pub const fn is_live(self) -> bool
        pub const fn started(self) -> bool
        pub const fn stopped(self) -> bool

DragNum
    enum DragNum<'a>: Debug, From<&'a mut f64>, From<&'a mut i64>
        I64(&'a mut i64)
        F64(&'a mut f64)

DragValue
    struct DragValue<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(value: impl Into<DragNum<'a>>) -> Self
        pub const fn text_align(self, a: Align) -> Self
        pub const fn speed(self, speed: f64) -> Self
        pub const fn range(self, range: RangeInclusive<f64>) -> Self
        pub const fn decimals(self, n: usize) -> Self
        pub fn suffix(self, text: impl Into<TextInput<'a>>) -> Self
        pub const fn editable(self, on: bool) -> Self
        pub fn style(self, s: impl Into<Option<&'a DragValueTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>

DragValueTheme
    struct DragValueTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub chip: ButtonTheme
        pub editor: TextEditTheme
        pub fn from_chip(chip: ButtonTheme, text_edit: &TextEditTheme) -> Self
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

Easing
    enum Easing: Clone, Copy, Debug, Deserialize<'de>, Eq, PartialEq, Serialize
        Linear
        OutCubic
        InOutCubic
        OutQuart
        OutBack
        pub const fn apply(self, t: f32) -> f32

EditEnd
    enum EditEnd: Clone, Copy, Debug, Eq, PartialEq
        Submitted
        Canceled

EditableLabel
    struct EditableLabel<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(text: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a TextEditTheme>>) -> Self
        pub const fn editable(self, on: bool) -> Self
        pub const fn max_chars(self, n: usize) -> Self
        pub fn show(self, ui: &mut Ui) -> EditableLabelResponse<'_>
        pub fn label_id(id: WidgetId) -> WidgetId

EditableLabelResponse
    struct EditableLabelResponse<'a>: Debug
        pub response: Response<'a>
        pub editing: bool
        pub committed_text: Option<String>

Engagement
    enum Engagement: Clone, Copy, Debug, Eq, PartialEq
        Pressed
        Focused

Expander
    struct Expander<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(label: impl Into<TextInput<'a>>) -> Self
        pub const fn start_open(self, open: bool) -> Self
        pub const fn open(self, open: &'a mut bool) -> Self
        pub const fn keep_body(self, keep: bool) -> Self
        pub fn style(self, s: impl Into<Option<&'a ExpanderTheme>>) -> Self
        pub fn header_id(expander: WidgetId) -> WidgetId
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> ExpanderResponse<'_, R>

ExpanderResponse
    struct ExpanderResponse<'a, R>
        pub response: Response<'a>
        pub inner: Option<R>
        pub changed: bool
        pub openness: f32
        impl<'a, R: Debug> Debug for ExpanderResponse<'a, R>

ExpanderTheme
    struct ExpanderTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize, ThemeSlot
        pub looks: StatefulLook
        pub arrow_size: Vec2
        pub arrow_radius: f32
        pub arrow_closed_angle: f32
        pub arrow_open_angle: f32
        pub gap: f32
        pub indent: f32
        pub body_padding: Spacing
        pub defaults: SlotDefaults
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

FocusPolicy
    enum FocusPolicy: Clone, Copy, Debug, Default, Eq, PartialEq
        PreserveOnMiss
        ClearOnMiss

FocusRingTheme
    struct FocusRingTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub color: RgbaF32
        pub width: f32
        pub const fn from_parts(p: &Palette, m: &Metrics) -> Self

FontFamily
    struct FontFamily: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        pub const MAX_NAME_LEN: usize
        pub const SANS: Self
        pub const MONO: Self
        pub const fn named(name: &str) -> Option<Self>
        pub fn name(&self) -> &str

FontLoadError
    enum FontLoadError: Debug, Display, Error
        Io { path: PathBuf, source: Error }
        NoFaces
        FamilyNameTooLong

FontScope
    enum FontScope: Clone, Copy, Debug, Eq, PartialEq
        Bundled
        System

FontSlant
    enum FontSlant: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Normal = 0
        Italic = 1

FontSource
    enum FontSource: Clone, Debug, From<&'static [u8]>, From<&Path>, From<&str>, From<Cow<'static, [u8]>>, From<PathBuf>, From<Vec<u8>>
        Bytes(Cow<'static, [u8]>)
        File(PathBuf)
        impl<const N: usize> From<&'static [u8; N]> for FontSource

FontWeight
    struct FontWeight: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, Ord, PartialEq, PartialOrd, Serialize
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

Found
    struct Found: Clone, Copy, Debug, PartialEq
        pub rect: Rect
        pub point: Vec2
        pub const fn rect(rect: Rect) -> Self

FramePaint
    enum FramePaint: Clone, Copy, Debug, Eq, PartialEq
        Skip
        Full
        Partial

FrameReport
    struct FrameReport: Debug
        pub repaint_requested: bool
        pub repaint_after: Option<Duration>
        pub const fn paint(&self) -> FramePaint

FrameSummary  [features: winit]
    struct FrameSummary: Clone, Copy, Debug, Eq, PartialEq
        pub frame_id: u64
        pub sent_inputs: u64
        pub repaint_requested: bool
        pub repaint_after: Option<Duration>

Gpu
    struct Gpu: Clone, Debug
        pub fn new(device: Device, queue: Queue) -> Self
        pub const fn device(&self) -> &Device
        pub const fn queue(&self) -> &Queue

GpuFrameCtx
    struct GpuFrameCtx<'a>: Debug
        pub device: &'a Device
        pub queue: &'a Queue
        pub encoder: &'a mut CommandEncoder
        pub target: &'a TextureView
        pub physical_size: UVec2
        pub physical_full_size: UVec2
        pub physical_offset: UVec2
        pub display_scale: f32
        pub raster_factor: f32
        pub dt: Duration

GpuInitCtx
    struct GpuInitCtx<'a>: Debug
        pub device: &'a Device
        pub target_format: TextureFormat
        pub text: &'a TextShaper

GpuPaint
    trait GpuPaint: 'static
        fn init(&mut self, ctx: &GpuInitCtx<'_>) { .. }
        fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>)

GpuPassStats
    struct GpuPassStats: Clone, Debug, Default
        pub fn last_pass(&self) -> Option<Duration>
        pub fn last_kind(&self, kind: BatchKind) -> Option<Duration>
        pub fn last_pipeline_stats(&self) -> Option<PipelineStats>
        pub fn last_copy_out(&self) -> Option<Duration>
        pub fn last_main_pass_cpu(&self) -> Option<Duration>

GpuRequestError
    #[non_exhaustive]
    enum GpuRequestError: Debug, Display, Error
        NoBackend
        RequestAdapter { source: RequestAdapterError }
        Requirements { source: UnmetRequirements }
        RequestDevice { source: RequestDeviceError }

GpuView
    struct GpuView: Configure, Debug, ThemeDefaults (blanket)
        pub fn new<T: GpuPaint + 'static>(paint: &Rc<RefCell<T>>) -> Self
        pub const fn repaint(self, repaint: bool) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>

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
        impl<'de, G> Deserialize<'de> where G: Deserialize<'de>
        impl<G: Clone> Clone for Gradient<G>
        impl<G: Debug> Debug for Gradient<G>
        impl<G: Eq> Eq for Gradient<G>
        impl<G: GradientGeometry> From<GradientBuilder<G>> for Gradient<G>
        impl<G: GradientGeometry> Hash for Gradient<G>
        impl<G: PartialEq> PartialEq for Gradient<G>
        impl<G> Serialize where G: Serialize

GradientBuilder
    struct GradientBuilder<G>
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
    struct GradientStops: Clone, Copy, Debug, Deref, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self
        impl<'a> IntoIterator for &'a GradientStops

Grid
    struct Grid<Rows = [Track; 0], Cols = [Track; 0]>: ThemeDefaults (blanket)
        impl Grid
            pub fn new() -> Self
        impl<Rows, Cols> Grid<Rows, Cols>
            pub fn rows<NewRows: AsRef<[Track]>>(self, rows: NewRows) -> Grid<NewRows, Cols>
            pub fn cols<NewCols: AsRef<[Track]>>(self, cols: NewCols) -> Grid<Rows, NewCols>
            pub const fn chrome(self, chrome: Chrome) -> Self
            pub const fn default_chrome(self, chrome: Chrome) -> Self
            pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R> where Rows: AsRef<[Track]>, Cols: AsRef<[Track]>
        impl<Rows, Cols> Configure for Grid<Rows, Cols>
        impl<Rows: Debug, Cols: Debug> Debug for Grid<Rows, Cols>

GridCell
    struct GridCell: Clone, Copy, Debug, Default, DockTab (blanket), Eq, From<(u16, u16)>, Hash, NoUninit, PartialEq
        pub const fn at(row: u16, col: u16) -> Self
        pub const fn with_span(self, row_span: u16, col_span: u16) -> Self
        pub const fn row(self) -> u16
        pub const fn col(self) -> u16
        pub const fn row_span(self) -> u16
        pub const fn col_span(self) -> u16
        pub const fn along(axis: Axis, main: u16) -> Self

HAlign
    enum HAlign: Clone, Copy, Debug, Default, Eq, PartialEq
        Auto = 0
        Left = 1
        Center = 2
        Right = 3
        Stretch = 4

Harness
    struct Harness: Debug
        pub fn play<A: App, X: Locate<A>>(&mut self, app: &mut A, actions: &[Action<X>]) -> Result<u32, ActError>
        pub fn settle(&mut self, app: &mut impl App) -> Result<u32, PlayError>
        pub fn shot(&mut self, app: &mut impl App, region: Option<Rect>) -> Result<Screenshot, PlayError>
        pub fn new(size: UVec2) -> Self
        pub const fn builder(size: UVec2) -> HarnessBuilder
        pub fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport
        pub fn frame_app(&mut self, app: &mut impl App) -> FrameReport
        pub fn step(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport
        pub fn frame_value<R>(&mut self, record: impl FnMut(&mut Ui) -> R) -> R
        pub fn response_in(&mut self, id: WidgetId, record: impl FnMut(&mut Ui)) -> ResponseState
        pub fn prime(&mut self, frames: u32, record: impl FnMut(&mut Ui))
        pub fn advance(&mut self, dt: Duration) -> &mut Self
        pub const fn at(&mut self, time: Duration) -> &mut Self
        pub fn advance_frames(&mut self, frames: u32, dt: Duration, record: impl FnMut(&mut Ui))
        pub fn advance_past_double_click(&mut self) -> &mut Self
        pub fn resize(&mut self, size: UVec2) -> &mut Self
        pub fn set_display(&mut self, display: Display) -> &mut Self
        pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta
        pub fn hover(&mut self, at: impl Into<Locator>) -> InputDelta
        pub fn press(&mut self, at: impl Into<Locator>, button: PointerButton) -> InputDelta
        pub fn release(&mut self, button: PointerButton) -> InputDelta
        pub fn click(&mut self, at: impl Into<Locator>)
        pub fn right_click(&mut self, at: impl Into<Locator>)
        pub fn drag_to(&mut self, at: impl Into<Locator>) -> InputDelta
        pub fn scroll_pixels(&mut self, at: impl Into<Locator>, delta: Vec2) -> InputDelta
        pub fn scroll_lines(&mut self, at: impl Into<Locator>, delta: Vec2) -> InputDelta
        pub fn pinch(&mut self, at: impl Into<Locator>, factor: f32) -> InputDelta
        pub fn key(&mut self, key: impl Into<Shortcut>) -> InputDelta
        pub fn key_up(&mut self, key: Key) -> InputDelta
        pub fn set_modifiers(&mut self, mods: Modifiers)
        pub fn type_text(&mut self, text: &str)
        pub fn find<A, X: Locate<A>>(&self, app: &A, at: &Locator<X>) -> Result<Found, LocateError>
        pub fn widgets(&self, filter: Option<&str>) -> Vec<InspectedWidget>
        pub fn rect(&self, id: WidgetId) -> Option<Rect>
        pub fn arranged(&self, id: WidgetId) -> Rect
        pub fn point_in(&self, id: WidgetId, local: Vec2) -> Vec2
        pub fn hit_at(&self, pos: Vec2) -> Option<WidgetId>
        pub fn state<S: 'static>(&self, id: WidgetId) -> &S
        pub fn window_commands(&self) -> &[WindowCommand]
        pub fn clipboard_text(&self) -> String
        pub fn set_clipboard_text(&mut self, text: &str)
        pub fn wait_for_gpu(&self)
        pub const fn ui(&mut self) -> &mut Ui

HarnessBuilder
    struct HarnessBuilder: Debug
        pub const fn scale(self, scale: f32) -> Self
        pub const fn user_scale(self, scale: UserScale) -> Self
        pub const fn refresh_millihertz(self, millihertz: u32) -> Self
        pub const fn pixel_snap(self, pixel_snap: bool) -> Self
        pub const fn fonts(self, scope: FontScope) -> Self
        pub fn shaper(self, shaper: TextShaper) -> Self
        pub fn depth(self, depth: Depth) -> Self
        pub fn gpu(self, gpu: Gpu) -> Self
        pub const fn frame_dt(self, dt: Duration) -> Self
        pub const fn settle_frames(self, frames: u32) -> Self
        pub fn build(self) -> Harness

HostDisconnected  [features: winit]
    struct HostDisconnected: Clone, Copy, Debug, Display, Eq, Error, PartialEq

HostHandle  [features: winit]
    struct HostHandle<T: 'static>: Clone, Debug
        pub fn request_repaint(&self, window: WindowToken)
        pub fn send_input(&self, window: WindowToken, event: OwnedInputEvent)
        pub fn last_frame(&self, window: WindowToken) -> Option<FrameSummary>
        pub fn is_hidden(&self, window: WindowToken) -> bool
        pub fn run_on_main(&self, f: impl FnOnce(&mut T) -> bool + Send + 'static) -> Result<(), HostDisconnected>
        pub fn quit(&self)
        pub fn remote(&self, window: WindowToken) -> Remote<T>

Hsv
    struct Hsv: Clone, Copy, Debug, Default, PartialEq
        pub h: f32
        pub s: f32
        pub v: f32
        pub const fn new(h: f32, s: f32, v: f32) -> Self
        pub fn to_color(self) -> RgbaF32
        pub fn from_color(color: RgbaF32, fallback_hue: f32) -> Self

HueSlice
    struct HueSlice: Clone, Copy, Debug
        pub fn color(self, s: f32, v: f32) -> RgbaF32

IconDefinition
    struct IconDefinition: Clone, Debug
        pub name: Cow<'static, str>
        pub view_box: Vec2
        pub svg: Span
        pub tintable: bool

IconHandle
    struct IconHandle: Clone, Copy, Debug, PartialEq
        pub const fn view_box(&self) -> Vec2

IconSet
    struct IconSet: Clone, Debug
        pub fn by_name(&self, name: &str) -> Option<IconHandle>

IconTable
    struct IconTable: Debug
        pub const fn baked(icons: &'static [IconDefinition], svg: &'static [u8]) -> Self
        pub fn from_svgs<'a, N: Into<Cow<'static, str>>>(sources: impl IntoIterator<Item = (N, &'a str)>) -> Result<Self, IconTableError>

IconTableError
    enum IconTableError: Debug, Display, Error
        Unreadable { name: Cow<'static, str>, source: SvgParseError }
        TooMany { count: usize }
        DuplicateName { name: Cow<'static, str> }
        TooLarge { bytes: u128 }

Image
    struct Image: Clone, Debug, Eq, PartialEq
        pub fn from_srgba8(size: UVec2, pixels: Vec<u8>) -> Result<Self, ImageDataError>
        pub fn blank(size: UVec2) -> Self
        pub const fn size(&self) -> UVec2
        pub fn texels(&self) -> &[SrgbaU8]
        pub fn texels_mut(&mut self) -> &mut [SrgbaU8]
        pub fn fill_with(&mut self, texel: impl FnMut(UVec2) -> SrgbaU8)
        pub fn row_mut(&mut self, row: u32) -> &mut [SrgbaU8]
        pub fn repeat_row(&mut self, row: u32)
        pub fn fitted(&self, max_side: u32) -> Self

ImageDataError
    enum ImageDataError: Clone, Copy, Debug, Display, Eq, Error, PartialEq
        ZeroSize { size: UVec2 }
        TooLarge { size: UVec2 }
        LengthMismatch { size: UVec2, expected: usize, actual: usize }

ImageDownsample
    enum ImageDownsample: Clone, Copy, Debug, Default, Eq, PartialEq
        Single
        Mean
        Peak

ImageFilter
    enum ImageFilter: Clone, Copy, Debug, Default, Deserialize<'de>, Eq, PartialEq, Serialize
        Linear
        Nearest

ImageFit
    enum ImageFit: Clone, Copy, Debug, Default, PartialEq
        Fill
        Contain
        Cover
        None
        Tile { offset: Vec2, scale: Vec2 }

ImageHandle
    struct ImageHandle: Clone, Debug
        pub fn size(&self) -> UVec2
        pub fn update(&self, image: &Image)

ImageTooLarge
    struct ImageTooLarge: Clone, Copy, Debug, Display, Eq, Error, PartialEq
        pub size: UVec2
        pub max_dimension: u32

ImePreedit
    struct ImePreedit<'a>: Clone, Copy, Debug, Eq, PartialEq
        pub text: &'a str
        pub cursor: Option<Span>

InnerResponse
    struct InnerResponse<'a, R>
        pub response: Response<'a>
        pub inner: R
        impl<'a, R: Debug> Debug for InnerResponse<'a, R>

InputDelta
    struct InputDelta: Clone, Copy, Debug, Default, Eq, PartialEq
        pub repaint_requested: bool

InputEvent
    enum InputEvent<'a>: Clone, Copy, Debug
        PointerMoved(Vec2)
        PointerLeft
        PointerPressed(PointerButton)
        PointerReleased(PointerButton)
        ScrollPixels(Vec2)
        ScrollLines(Vec2)
        Zoom(f32)
        KeyDown { key: Key, repeat: bool, physical: Key, text: KeyText }
        KeyUp { key: Key }
        ModifiersChanged(Modifiers)
        ImePreedit(ImePreedit<'a>)
        ImeCommit(&'a str)
        SurfaceFocusLost
        impl From<InputEvent<'_>> for OwnedInputEvent

InputPolicy
    enum InputPolicy: Clone, Copy, Debug, Default, Eq, PartialEq
        Always
        OnDelta

InspectedWidget
    struct InspectedWidget: Clone, Debug, PartialEq
        pub id: WidgetId
        pub rect: Rect
        pub sense: Sense
        pub focusable: bool
        pub disabled: bool
        pub clipped: bool
        pub text: String

InternedStr
    struct InternedStr: Clone, Copy, Debug
        pub const fn is_empty(&self) -> bool
        impl From<InternedStr> for TextInput<'_>

Interpolation
    enum Interpolation: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Oklab
        Linear

Justify
    enum Justify: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
        Start
        Center
        End
        SpaceBetween
        SpaceAround

Key
    enum Key: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
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
        impl From<Key> for Shortcut

KeyClass
    enum KeyClass: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        Text
        Edit
        Caret
        Page
        Focus
        Cycle
        Escape
        Accel
        pub fn of(press: KeyPress) -> Self

KeyFilter
    struct KeyFilter: BitOr, Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
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

KeyPress
    struct KeyPress: Clone, Copy, Debug, Eq, PartialEq
        pub key: Key
        pub mods: Modifiers
        pub repeat: bool
        pub physical: Key
        pub text: KeyText

KeyText
    struct KeyText: Clone, Copy, Debug, Default, Eq, PartialEq
        pub const CAPACITY: usize
        pub const EMPTY: Self
        pub fn new(text: &str) -> Self
        pub fn from_char(c: char) -> Self
        pub fn as_str(&self) -> &str
        pub fn is_empty(&self) -> bool

KeyboardWake
    struct KeyboardWake: BitOr, Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
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

Layer
    enum Layer: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, Ord, PartialEq, PartialOrd
        Main = 0
        Popup = 1
        Modal = 2
        Menu = 3
        Tooltip = 4
        Debug = 5

LayerScope
    struct LayerScope<'a>: Debug
        pub const fn fixed_at(self, point: Vec2) -> Self
        pub const fn anchor(self, anchor: Anchor) -> Self
        pub fn max_size(self, size: impl Into<Size>) -> Self
        pub fn show<R>(self, body: impl FnOnce(&mut Ui) -> R) -> R

Lens
    struct Lens: Clone, Copy, Debug, Default, PartialEq, Serialize
        pub bezel: f32
        pub thickness: f32
        pub refractive_index: f32
        pub dispersion: f32
        pub highlight: f32
        pub const fn is_refractive_index(n: f32) -> bool
        impl<'de> Deserialize<'de> where Lens: Default

LinearGeometry
    struct LinearGeometry: Clone, Copy, Debug, Deserialize<'de>, GradientGeometry (blanket), PartialEq, Serialize
        pub angle: f32

LinearGradient
    type LinearGradient = Gradient<LinearGeometry>

LinearGradientBuilder
    type LinearGradientBuilder = GradientBuilder<LinearGeometry>

Locate
    trait Locate<A>: Debug
        fn find_all(&self, app: &A, ui: &Ui, out: &mut Vec<Found>) -> Result<(), LocateError>

LocateError
    enum LocateError: Clone, Debug, Display, Error, PartialEq
        NotFound { locator: String, nearest: Vec<String> }
        Ambiguous { locator: String, matches: Vec<String> }
        OffScreen { locator: String }
        Covered { locator: String, point: Vec2 }
        NoNth { locator: String, count: usize }
        impl From<LocateError> for PlayError

Locator
    enum Locator<X = Infallible>
        Text(String)
        Id(WidgetId)
        Point(Vec2)
        Offset(Box<Self>, Vec2)
        Nth(Box<Self>, usize)
        App(X)
        impl<X> Locator<X>
            pub fn find<A>(&self, app: &A, ui: &Ui) -> Result<Found, LocateError> where X: Locate<A>
        impl<X: Clone> Clone for Locator<X>
        impl<X: Debug> Debug for Locator<X>
        impl<X: PartialEq> PartialEq for Locator<X>
        impl<X> From<&str> for Locator<X>
        impl<X> From<String> for Locator<X>
        impl<X> From<Vec2> for Locator<X>
        impl<X> From<WidgetId> for Locator<X>

MenuBar
    struct MenuBar<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a MenuBarTheme>>) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut MenuBarMenus<'_>) -> R) -> InnerResponse<'_, R>

MenuBarMenus
    struct MenuBarMenus<'u>: Debug
        pub fn menu<'l, R>(&mut self, label: impl Into<TextInput<'l>>, body: impl FnOnce(&mut Ui, &CloseHandle) -> R) -> Option<R>

MenuBarTheme
    struct MenuBarTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize, ThemeSlot
        pub title: StatefulLook
        pub open: WidgetLook
        pub bar: Chrome
        pub bar_padding: Spacing
        pub gap: f32
        pub defaults: SlotDefaults
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

MenuItem
    struct MenuItem<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a MenuItemTheme>>) -> Self
        pub const fn shortcut(self, s: Shortcut) -> Self
        pub const fn shortcut_hint(self, shortcut: Shortcut) -> Self
        pub fn show<'ui>(self, ui: &'ui mut Ui, popup: &CloseHandle) -> Response<'ui>

MenuItemTheme
    struct MenuItemTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize, ThemeSlot
        pub looks: StatefulLook
        pub shortcut: RgbaF32
        pub gap: f32
        pub defaults: SlotDefaults
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

MenuSeparator
    struct MenuSeparator<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a SeparatorTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>

MenuState
    enum MenuState: Clone, Copy, Debug, Eq, PartialEq
        Shut
        Open

Metrics
    struct Metrics: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub space: SpaceSteps
        pub radius: RadiusSteps
        pub stroke: StrokeSteps
        pub float_shadow: Shadow
        pub transition: Option<AnimationSpec>
        pub const DEFAULT: Self

Modal
    struct Modal<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a ModalTheme>>) -> Self
        pub const fn backdrop(self, c: RgbaF32) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &CloseHandle) -> R) -> OverlayResponse<R>
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub const fn default_chrome(self, chrome: Chrome) -> Self

ModalTheme
    struct ModalTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub panel: Chrome
        pub backdrop: RgbaF32
        pub padding: Spacing
        pub min_width: f32
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

Modifiers
    struct Modifiers: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
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

NodeIdx
    struct NodeIdx: Clone, Copy, Debug, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize

Number
    trait Number: Scalar
        impl<T: Scalar> Number for T

OffscreenHost
    struct OffscreenHost: Debug
        pub const WINDOW: WindowToken
        pub const fn builder(gpu: Gpu) -> OffscreenHostBuilder
        pub const fn ui(&mut self) -> &mut Ui
        pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta
        pub fn advance(&mut self, dt: Duration) -> &mut Self
        pub fn at(&mut self, time: Duration) -> &mut Self
        pub fn wait_for_gpu(&self)
        pub fn is_device_lost(&self) -> bool
        pub fn recover(&mut self, gpu: Gpu)
        pub fn frame<T: App>(&mut self, target: RenderTarget<'_>, system_scale: f32, app: &mut T) -> FrameReport
        pub const fn gpu_pass_stats(&self) -> &GpuPassStats

OffscreenHostBuilder
    struct OffscreenHostBuilder: Debug
        pub const fn fonts(self, scope: FontScope) -> Self
        pub fn shaper(self, shaper: TextShaper) -> Self
        pub const fn collect_gpu_stats(self, collect: bool) -> Self
        pub const fn time(self, time: OffscreenTime) -> Self
        pub const fn retained_target(self, retained: bool) -> Self
        pub const fn pixel_snap(self, pixel_snap: bool) -> Self
        pub const fn system_clipboard(self, system: bool) -> Self  [features: system-clipboard or winit]
        pub fn build(self) -> OffscreenHost

OffscreenTime
    enum OffscreenTime: Clone, Copy, Debug, Eq, PartialEq
        Realtime
        Manual

Okhsv
    struct Okhsv: Clone, Copy, Debug, Default, PartialEq
        pub h: f32
        pub s: f32
        pub v: f32
        pub const fn new(h: f32, s: f32, v: f32) -> Self
        pub fn to_color(self) -> RgbaF32
        pub fn from_color(color: RgbaF32, fallback_hue: f32) -> Self

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

OwnedInputEvent
    struct OwnedInputEvent: Clone, Debug, From<InputEvent<'_>>
        pub fn as_event(&self) -> InputEvent<'_>

PLATFORM
    const PLATFORM: Platform

Palette
    struct Palette: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub text: RgbaF32
        pub text_muted: RgbaF32
        pub text_disabled: RgbaF32
        pub window_bg: RgbaF32
        pub surround: RgbaF32
        pub element: RgbaF32
        pub element_mid: RgbaF32
        pub element_strong: RgbaF32
        pub border_focused: RgbaF32
        pub accent: RgbaF32
        pub const DEFAULT: Self
        pub const fn border_soft(&self) -> RgbaF32
        pub const fn border_mid(&self) -> RgbaF32
        pub const fn border_strong(&self) -> RgbaF32
        pub fn popup_panel(&self, m: &Metrics) -> Chrome

Panel
    struct Panel: Configure, Debug, ThemeDefaults (blanket)
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R>
        pub fn hstack() -> Self
        pub fn vstack() -> Self
        pub fn stack(axis: Axis) -> Self
        pub fn wrap_hstack() -> Self
        pub fn wrap_vstack() -> Self
        pub fn wrap_stack(axis: Axis) -> Self
        pub fn zstack() -> Self
        pub fn canvas() -> Self
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub const fn default_chrome(self, chrome: Chrome) -> Self

ParseColorError
    enum ParseColorError: Clone, Copy, Debug, Display, Eq, Error, PartialEq
        Length
        Digit
        LinearForm
        Channel

ParseShortcutError
    enum ParseShortcutError: Clone, Copy, Debug, Display, Eq, Error, PartialEq
        NoKey
        Modifier
        Key

PipelineStats
    struct PipelineStats: Clone, Copy, Debug, Default, Eq, PartialEq
        pub vertex_shader_invocations: u64
        pub clipper_invocations: u64
        pub clipper_primitives_out: u64
        pub fragment_shader_invocations: u64
        pub compute_shader_invocations: u64

Platform
    enum Platform: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        Mac
        Windows
        Linux

PlayError
    enum PlayError: Clone, Debug, Display, Error, From<LocateError>, PartialEq
        Locate(LocateError)
        Unsettled { step: String, frames: u32 }
        BadPinch { factor: f32 }
        EmptyRegion
        NoPixels
        Disconnected
        NoFrame { seconds: f32 }

PointerAction
    struct PointerAction: Clone, Copy, Debug, Eq, PartialEq
        pub id: WidgetId
        pub button: PointerButton
        pub edge: PointerEdge

PointerButton
    enum PointerButton: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        Left = 0
        Right = 1
        Middle = 2

PointerEdge
    enum PointerEdge: Clone, Copy, Debug, Eq, PartialEq
        Pressed { count: u8 }
        Clicked { count: u8 }
        DragStarted
        DragStopped

PointerEvent
    enum PointerEvent: Clone, Copy, Debug, PartialEq
        Move(Vec2)
        Down { pos: Vec2, button: PointerButton }
        Up { pos: Vec2, button: PointerButton }
        Scroll { pos: Vec2, pixels: Vec2, lines: Vec2 }
        Zoom { pos: Vec2, factor: ZoomFactor }
        Leave

PointerWake
    struct PointerWake: BitOr, Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
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

Popup
    struct Popup: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(anchor: Anchor) -> Self
        pub const fn layer(self, layer: Layer) -> Self
        pub const fn click_outside(self, m: ClickOutside) -> Self
        pub const fn anchor(self, anchor: Anchor) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &CloseHandle) -> R) -> OverlayResponse<R>
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub const fn default_chrome(self, chrome: Chrome) -> Self

PopupTrigger
    struct PopupTrigger: Configure, Debug, ThemeDefaults (blanket)
        pub fn on(snapshot: &ResponseSnapshot) -> Self
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub const fn default_chrome(self, chrome: Chrome) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &CloseHandle) -> R) -> OverlayResponse<Option<R>>
        pub fn open(ui: &mut Ui, for_id: WidgetId)
        pub fn close(ui: &mut Ui, for_id: WidgetId)
        pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool

PowerPreference
    enum PowerPreference: Clone, Copy, Debug, Default, Eq, PartialEq
        Any
        LowPower
        HighPerformance

ProgressBar
    struct ProgressBar<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(fraction: f32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ProgressBarTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>

ProgressBarTheme
    struct ProgressBarTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub track: RgbaF32
        pub fill: RgbaF32
        pub thickness: f32
        pub const fn from_palette(p: &Palette) -> Self

RadialGeometry
    struct RadialGeometry: Clone, Copy, Debug, Deserialize<'de>, GradientGeometry (blanket), PartialEq, Serialize
        pub center: Vec2
        pub radius: Vec2

RadialGradient
    type RadialGradient = Gradient<RadialGeometry>

RadialGradientBuilder
    type RadialGradientBuilder = GradientBuilder<RadialGeometry>

RadioButton
    struct RadioButton<'a, T>: Configure, ThemeDefaults (blanket)
        pub fn new(cur: &'a mut T, value: T) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ToggleTheme>>) -> Self
        impl<T: PartialEq> RadioButton<'_, T>
            pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>
        impl<'a, T: Debug> Debug for RadioButton<'a, T>

RadiusSteps
    struct RadiusSteps: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub mark: f32
        pub control: f32
        pub card: f32
        pub float: f32

Rect
    struct Rect: Clone, Copy, Debug, Default, Hash, PartialEq, Pod, Zeroable
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

Remote  [features: winit]
    struct Remote<T: 'static>: Debug
        pub fn with_blocked(self, blocked: impl Fn() -> bool + Send + 'static) -> Self
        pub fn query<R: Send + 'static>(&self, f: impl FnOnce(&mut T, &mut Ui) -> R + Send + 'static) -> Result<R, PlayError>
        pub fn play<X: Locate<T> + Clone + Send + 'static>(&self, actions: &[Action<X>]) -> Result<u32, ActError>
        pub fn shot(&self, region: Option<Rect>) -> Result<Screenshot, PlayError>
        pub fn widgets(&self, filter: Option<String>) -> Result<Vec<InspectedWidget>, PlayError>

RenderTarget
    struct RenderTarget<'a>: Clone, Copy, Debug
        pub fn new(texture: &'a Texture) -> Self

RequestedGpu
    struct RequestedGpu: Debug
        pub adapter: Adapter
        pub gpu: Gpu
        pub fn headless(power_preference: PowerPreference, optional: Features) -> Result<Self, GpuRequestError>

Response
    struct Response<'a>: Debug, Deref
        pub id: WidgetId
        pub fn new(ui: &'a Ui, snapshot: ResponseSnapshot) -> Self
        pub fn snapshot(&self) -> ResponseSnapshot

ResponseSnapshot
    struct ResponseSnapshot: Clone, Copy, Debug, Deref
        pub id: WidgetId
        pub state: ResponseState

ResponseState
    struct ResponseState: Clone, Copy, Debug, Default
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

RgbaF32
    struct RgbaF32: Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, From<SrgbaU8>, FromStr, Hash, PartialEq, Pod, Serialize, Zeroable
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
        pub const fn hexa(rgba: u32) -> Self
        pub const fn to_srgba(self) -> SrgbaU8
        impl From<RgbaF32> for Brush

Screenshot
    struct Screenshot: Clone, Debug, PartialEq
        pub const fn image(&self) -> &Image
        pub const fn scale(&self) -> f32
        pub const fn region(&self) -> Rect
        pub fn place(&self, rect: Rect, picture: UVec2) -> Option<Rect>

ScreenshotId
    struct ScreenshotId: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq

Scroll
    struct Scroll<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn vertical() -> Self
        pub fn horizontal() -> Self
        pub fn both() -> Self
        pub fn pan_by(self, delta: Vec2) -> Self
        pub const fn reveal(self, child: WidgetId) -> Self
        pub fn zoom_by(self, factor: f32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ScrollbarTheme>>) -> Self
        pub const fn bar_mode(self, mode: BarMode) -> Self
        pub const fn overlay_bars(self) -> Self
        pub const fn hide_bars(self) -> Self
        pub fn content_margin(self, m: impl Into<Spacing>) -> Self
        pub fn zoomable(self) -> Self
        pub fn zoom_config(self, config: ZoomConfig) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R>
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub const fn default_chrome(self, chrome: Chrome) -> Self
        pub fn offset(ui: &Ui, id: WidgetId) -> Vec2
        pub fn set_offset(ui: &mut Ui, id: WidgetId, offset: Vec2)
        pub fn zoom(ui: &Ui, id: WidgetId) -> f32

ScrollDelta
    struct ScrollDelta: Clone, Copy, Debug, Default, PartialEq
        pub pixels: Vec2
        pub lines: Vec2
        pub zoom: ZoomFactor
        pub fn pan(self, line_height: f32) -> Vec2

ScrollbarTheme
    struct ScrollbarTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub thickness: f32
        pub gap: f32
        pub min_thumb: f32
        pub track: RgbaF32
        pub thumb: RgbaF32
        pub thumb_hovered: RgbaF32
        pub thumb_active: RgbaF32
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

Selection
    enum Selection: Clone, Copy, Debug, Default, Eq, From<bool>, PartialEq
        Unselected
        Selected

Sense
    struct Sense: BitOr, Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
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

Separator
    struct Separator<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn horizontal() -> Self
        pub fn vertical() -> Self
        pub fn style(self, s: impl Into<Option<&'a SeparatorTheme>>) -> Self
        pub const fn thickness(self, px: f32) -> Self
        pub const fn color(self, c: RgbaF32) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>

SeparatorTheme
    struct SeparatorTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub color: RgbaF32
        pub thickness: f32
        pub margin: Spacing
        pub const fn from_parts(p: &Palette, m: &Metrics) -> Self
        pub fn menu_separator(p: &Palette, m: &Metrics) -> Self

Shadow
    struct Shadow: Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, PartialEq, Serialize
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

Shortcut
    struct Shortcut: Clone, Copy, Debug, Display, DockTab (blanket), Eq, From<Key>, FromStr, Hash, PartialEq
        pub mods: ShortcutMods
        pub key: Key
        pub const fn new(mods: ShortcutMods, key: Key) -> Self
        pub const fn key(key: Key) -> Self
        pub const fn ctrl(c: char) -> Self
        pub const fn ctrl_shift(c: char) -> Self
        pub fn matches(self, press: KeyPress) -> bool

ShortcutMods
    struct ShortcutMods: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
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

Size
    struct Size: Clone, Copy, Debug, Default, Deserialize<'de>, Hash, PartialEq, Pod, Serialize, Zeroable
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
        impl From<Size> for SizeSpec
        impl From<Size> for Vec2
        impl<T: Number> From<T> for Size
        impl<W: Number, H: Number> From<(W, H)> for Size

SizeSpec
    struct SizeSpec: Clone, Copy, Debug, Default, From<Size>, From<Sizing>, Hash, PartialEq
        pub const fn new(w: Sizing, h: Sizing) -> Self
        pub const fn w(self) -> Sizing
        pub const fn h(self) -> Sizing
        impl<T: Number> From<T> for SizeSpec
        impl<W: Into<Sizing>, H: Into<Sizing>> From<(W, H)> for SizeSpec

Sizing
    struct Sizing: Clone, Copy, Debug, Default, Hash, PartialEq
        pub const HUG: Self
        pub const FILL: Self
        pub const fn fixed(value: f32) -> Self
        pub const fn fill(weight: f32) -> Self
        pub const fn split(fraction: f32) -> [Self; 2]
        pub const fn fixed_value(self) -> Option<f32>
        pub const fn fill_weight(self) -> Option<f32>
        pub const fn is_hug(self) -> bool
        impl From<Sizing> for SizeSpec
        impl From<Sizing> for Track
        impl<T: Number> From<T> for Sizing

Slider
    struct Slider<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(value: impl Into<DragNum<'a>>, range: RangeInclusive<f64>) -> Self
        pub const fn step(self, step: f64) -> Self
        pub const fn decimals(self, n: usize) -> Self
        pub fn style(self, s: impl Into<Option<&'a SliderTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>

SliderTheme
    struct SliderTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub track: RgbaF32
        pub fill: RgbaF32
        pub knob: RgbaF32
        pub knob_size: f32
        pub track_thickness: f32
        pub const fn from_palette(p: &Palette) -> Self

SlotDefaults
    struct SlotDefaults: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub padding: Spacing
        pub margin: Spacing
        pub animation: Option<AnimationSpec>

SpaceSteps
    struct SpaceSteps: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub xs: f32
        pub sm: f32
        pub md: f32
        pub lg: f32
        pub xl: f32

Spacing
    struct Spacing: Add, Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Pod, Serialize, Zeroable
        pub const ZERO: Self
        pub fn to_array(self) -> [f32; 4]
        pub fn all(v: f32) -> Self
        pub fn xy(x: f32, y: f32) -> Self
        pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self
        pub fn from_array(v: [f32; 4]) -> Self
        pub fn sums(self) -> Size
        impl<L: Number, T: Number, R: Number, B: Number> From<(L, T, R, B)> for Spacing
        impl<T: Number> From<T> for Spacing
        impl<X: Number, Y: Number> From<(X, Y)> for Spacing

Spinner
    struct Spinner<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a SpinnerTheme>>) -> Self
        pub const fn diameter(self, px: f32) -> Self
        pub const fn color(self, c: RgbaF32) -> Self
        pub const fn width(self, px: f32) -> Self
        pub fn show(self, ui: &mut Ui) -> Response<'_>

SpinnerTheme
    struct SpinnerTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub color: RgbaF32
        pub diameter: f32
        pub sweep: f32
        pub period: Duration
        pub width_ratio: f32
        pub min_width: f32
        pub const fn from_palette(p: &Palette) -> Self

SplitDirection
    enum SplitDirection: Clone, Copy, Debug, Deserialize<'de>, Eq, PartialEq, Serialize
        Row
        Col

SplitHalf
    enum SplitHalf: Clone, Copy, Debug, Eq, PartialEq
        First
        Second

SplitSide
    enum SplitSide: Clone, Copy, Debug, Deserialize<'de>, Eq, PartialEq, Serialize
        Left
        Right
        Top
        Bottom
        pub const fn direction(self) -> SplitDirection

Splitter
    struct Splitter<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn row(ratio: &'a mut f32) -> Self
        pub fn col(ratio: &'a mut f32) -> Self
        pub const fn min_pane(self, px: f32) -> Self
        pub fn style(self, s: impl Into<Option<&'a SplitterTheme>>) -> Self
        pub fn show(self, ui: &mut Ui, body: impl FnMut(&mut Ui, SplitHalf)) -> ValueResponse<'_>

SplitterTheme
    struct SplitterTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub grab_thickness: f32
        pub rule: RgbaF32
        pub rule_thickness: f32
        pub hovered: RgbaF32
        pub active: RgbaF32
        pub const fn from_parts(p: &Palette, m: &Metrics) -> Self

Spread
    enum Spread: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Pad = 0
        Repeat = 1
        Reflect = 2

SrgbaU8
    struct SrgbaU8: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq, Pod, Zeroable
        pub r: u8
        pub g: u8
        pub b: u8
        pub a: u8
        pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self
        pub const fn rgb(r: u8, g: u8, b: u8) -> Self
        pub const fn hex(rgb: u32) -> Self
        pub const fn hexa(rgba: u32) -> Self
        impl From<SrgbaU8> for Brush
        impl From<SrgbaU8> for RgbaF32

StatefulLook
    struct StatefulLook: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub normal: WidgetLook
        pub hovered: WidgetLook
        pub active: WidgetLook
        pub disabled: WidgetLook
        pub const fn pick(&self, state: &ResponseState, engagement: Engagement) -> &WidgetLook
        pub const fn each_mut(&mut self) -> [&mut WidgetLook; 4]

Stop
    struct Stop: Clone, Copy, Debug, Default, Deserialize<'de>, Eq, PartialEq, Serialize
        pub fn new(offset: f32, color: impl Into<RgbaF32>) -> Self
        pub const fn offset(self) -> f32
        pub const fn color(self) -> RgbaF32

Stroke
    struct Stroke: Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub color: RgbaF32
        pub width: f32
        pub const NONE: Self
        pub const fn is_noop(&self) -> bool
        pub const fn new(color: RgbaF32, width: f32) -> Self

StrokeSteps
    struct StrokeSteps: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub hairline: f32
        pub fine: f32
        pub line: f32

SurfaceError  [features: winit]
    #[non_exhaustive]
    enum SurfaceError: Debug, Display, Error
        Create { source: CreateSurfaceError }
        Incompatible
        MissingSrgb
        MissingUsages { missing: SurfaceUsages }

SurfaceUsages  [features: winit]
    struct SurfaceUsages: BitOr, Clone, Copy, Debug, Default, Display, DockTab (blanket), Eq, Hash, PartialEq
        pub const RENDER_ATTACHMENT: Self
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

SvgParseError
    struct SvgParseError: Debug, Display, Error

Switch
    struct Switch<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(value: &'a mut bool) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ToggleTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse<'_>

TabAddress
    struct TabAddress: Clone, Copy, Debug, Eq, PartialEq
        pub group: TabGroupId
        pub idx: usize

TabChip
    struct TabChip: Clone, Copy, Debug
        pub slot: usize
        pub key: u64
        pub label: InternedStr
        pub selected: bool
        pub text: TextStyle
        pub id: WidgetId
        pub fn show_label(&self, ui: &mut Ui)

TabGroup
    struct TabGroup<T>
        pub const fn id(&self) -> TabGroupId
        pub fn tabs(&self) -> &[T]
        pub const fn active(&self) -> usize
        impl<T: Copy> TabGroup<T>
            pub fn active_tab(&self) -> T
        impl<'de, T> Deserialize<'de> where T: Deserialize<'de>
        impl<T: Clone> Clone for TabGroup<T>
        impl<T: Debug> Debug for TabGroup<T>
        impl<T: Eq> Eq for TabGroup<T>
        impl<T: PartialEq> PartialEq for TabGroup<T>
        impl<T> Serialize where T: Serialize

TabGroupId
    struct TabGroupId: Clone, Copy, Debug, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize

TabItem
    struct TabItem: Clone, Copy, Debug
        pub key: u64
        pub label: InternedStr
        pub closable: bool
        pub draggable: bool
        pub const fn new(key: u64, label: InternedStr) -> Self

TabOverflow
    enum TabOverflow: Clone, Copy, Debug, Default, Eq, PartialEq
        Scroll
        Menu

TabStrip
    struct TabStrip<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(items: &'a [TabItem]) -> Self
        pub fn selected(self, selected: impl Into<Option<usize>>) -> Self
        pub const fn focused(self, focused: bool) -> Self
        pub const fn overflow(self, overflow: TabOverflow) -> Self
        pub fn style(self, s: impl Into<Option<&'a TabsTheme>>) -> Self
        pub fn chip_id(strip: WidgetId, key: u64) -> WidgetId
        pub fn close_id(strip: WidgetId, key: u64) -> WidgetId
        pub fn show(self, ui: &mut Ui, content: impl FnMut(&mut Ui, &TabChip)) -> TabStripResponse<'_>

TabStripFocus
    enum TabStripFocus: Clone, Copy, Debug, Eq, PartialEq
        Focused
        Unfocused

TabStripResponse
    struct TabStripResponse<'a>: Debug
        pub response: Response<'a>
        pub clicked: Option<usize>
        pub keyed: Option<usize>
        pub menu_picked: Option<usize>
        pub closed: Option<usize>
        pub drag_started: Option<usize>
        pub drag_stopped: Option<usize>
        pub fn activated(&self) -> Option<usize>

TabbedView
    struct TabbedView<'a, S, L, K = fn(usize, &S) -> u64>: ThemeDefaults (blanket)
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
        impl<'a, S: Debug, L: Debug, K: Debug> Debug for TabbedView<'a, S, L, K>
        impl<S, L, K> Configure for TabbedView<'_, S, L, K>

TabbedViewResponse
    struct TabbedViewResponse<'a>: Debug
        pub response: Response<'a>
        pub action: Option<TabsAction>

TabsAction
    enum TabsAction: Clone, Copy, Debug, Eq, PartialEq
        Activated { idx: usize }
        Closed { idx: usize }
        Reordered { from: usize, to: usize }

TabsTheme
    struct TabsTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize, ThemeSlot
        pub selected: StatefulLook
        pub unselected: StatefulLook
        pub cap_focused: RgbaF32
        pub cap_unfocused: RgbaF32
        pub cap_thickness: f32
        pub strip: Chrome
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
        pub close_glyph: f32
        pub close_stroke: f32
        pub close_shown: CloseShown
        pub caret: RgbaF32
        pub caret_width: f32
        pub label_gap: f32
        pub defaults: SlotDefaults
        pub const fn cap(&self, focus: TabStripFocus) -> RgbaF32
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

TargetFormat
    struct TargetFormat: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        pub fn new(format: TextureFormat) -> Self

Text
    struct Text<'a>: Configure, Debug, ThemeDefaults (blanket)
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

TextEdit
    struct TextEdit<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn new(text: &'a mut String) -> Self
        pub fn buffered(current: impl Into<TextInput<'a>>) -> Self
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

TextEditResponse
    struct TextEditResponse<'a>: Debug
        pub response: Response<'a>
        pub changed: bool
        pub committed: bool
        pub committed_text: Option<String>
        pub ended: Option<EditEnd>
        pub focus_gained: bool
        pub focus_lost: bool

TextEditTheme
    struct TextEditTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize, ThemeSlot
        pub looks: StatefulLook
        pub placeholder: RgbaF32
        pub caret: RgbaF32
        pub caret_width: f32
        pub selection: RgbaF32
        pub defaults: SlotDefaults
        pub fn corner_centering(&self, text: Size, at: Vec2) -> Vec2
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

TextInput
    enum TextInput<'a>: Debug, Default, From<Cow<'a, str>>, From<InternedStr>, From<String>
        Borrowed(&'a str)
        Owned(String)
        Interned(InternedStr)
        impl<'a, T: AsRef<str> + ?Sized> From<&'a T> for TextInput<'a>

TextShaper
    struct TextShaper: Clone, Debug, Default
        pub fn new() -> Self
        pub fn with_fonts(scope: FontScope) -> Self
        pub fn load_font(&self, source: impl Into<FontSource>) -> Result<FontFamily, FontLoadError>
        pub fn has_font(&self, family: FontFamily) -> bool
        pub fn font_families(&self) -> Vec<FontFamily>
        pub fn glyphs(&self) -> TextGlyphs<'_>

TextStyle
    struct TextStyle: Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, PartialEq, Serialize
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

TextStyleOverrides
    struct TextStyleOverrides: Clone, Copy, Debug, Default, Deserialize<'de>, PartialEq, Serialize
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

TextWrap
    enum TextWrap: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
        SingleLine
        Scroll
        Truncate
        Ellipsis
        EllipsisMiddle
        Wrap
        WrapWithOverflow

Theme
    struct Theme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub button: ButtonTheme
        pub checkbox: ToggleTheme
        pub radio: ToggleTheme
        pub switch: ToggleTheme
        pub scrollbar: ScrollbarTheme
        pub text_edit: TextEditTheme
        pub drag_value: DragValueTheme
        pub context_menu: ContextMenuTheme
        pub menu_bar: MenuBarTheme
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
        pub panel_chrome: Option<Chrome>
        pub panel_clip: ClipMode
        pub fn scale_text(&mut self, factor: f32)
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

ToggleTheme
    struct ToggleTheme: Clone, Debug, Deserialize<'de>, PartialEq, Serialize, ThemeSlot
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
        pub fn checkbox(p: &Palette, m: &Metrics) -> Self
        pub fn radio(p: &Palette, m: &Metrics) -> Self
        pub fn switch(p: &Palette, m: &Metrics) -> Self

Tooltip
    struct Tooltip<'a>: Configure, Debug, ThemeDefaults (blanket)
        pub fn on(snapshot: &'a ResponseSnapshot, text: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a TooltipTheme>>) -> Self
        pub const fn delay(self, delay: Duration) -> Self
        pub const fn when_disabled(self, yes: bool) -> Self
        pub fn show(self, ui: &mut Ui) -> TooltipResponse
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub const fn default_chrome(self, chrome: Chrome) -> Self

TooltipResponse
    struct TooltipResponse: Clone, Copy, Debug
        pub visible: bool

TooltipTheme
    struct TooltipTheme: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub panel: Chrome
        pub text: TextStyleOverrides
        pub padding: Spacing
        pub max_size: Size
        pub delay: Duration
        pub warmup: Duration
        pub gap: f32
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

Track
    struct Track: Clone, Copy, Debug, From<Sizing>, Hash, PartialEq
        pub const fn new(sizing: Sizing) -> Self
        pub const HUG: Self
        pub const FILL: Self
        pub const fn fixed(v: f32) -> Self
        pub const fn fill(weight: f32) -> Self
        pub const fn with_min(self, min: f32) -> Self
        pub const fn with_max(self, max: f32) -> Self
        pub const fn sizing(self) -> Sizing
        pub const fn min(self) -> f32
        pub const fn max(self) -> f32

TranslateScale
    struct TranslateScale: Clone, Copy, Debug, Default, PartialEq
        pub const IDENTITY: Self
        pub const fn is_identity(self) -> bool
        pub const fn new(translation: Vec2, scale: f32) -> Self
        pub const fn from_translation(t: Vec2) -> Self
        pub const fn from_scale(s: f32) -> Self
        pub const fn translation(self) -> Vec2
        pub const fn scale(self) -> f32
        pub const fn anchored_at(self, origin: Vec2) -> Self
        pub const fn compose(self, other: Self) -> Self
        pub const fn apply_point(self, p: Vec2) -> Vec2
        pub const fn inverse_point(self, p: Vec2) -> Vec2
        pub const fn inverse_vector(self, v: Vec2) -> Vec2
        pub const fn apply_rect(self, r: Rect) -> Rect

Ui
    struct Ui: Debug
        pub const fn theme(&self) -> &Rc<Theme>
        pub fn set_theme(&mut self, theme: impl Into<Rc<Theme>>)
        pub const fn watch_pointer(&mut self, flags: PointerWake)
        pub const fn watch_keyboard(&mut self, flags: KeyboardWake)
        pub fn pointer_events(&self) -> &[PointerEvent]
        pub fn keyboard_events(&self) -> &[KeyPress]
        pub fn key_pressed(&mut self, key: Key) -> bool
        pub fn key_held(&mut self, key: Key) -> bool
        pub fn key_released(&mut self, key: Key) -> bool
        pub fn shortcut_pressed(&mut self, shortcut: impl Into<Shortcut>) -> bool
        pub fn request_relayout(&mut self)
        pub const fn now(&self) -> Duration
        pub const fn set_cursor(&mut self, cursor: CursorIcon)
        pub const fn cursor(&self) -> CursorIcon
        pub const fn set_vsync(&mut self, vsync: Vsync)
        pub const fn vsync(&self) -> Vsync
        pub fn request_repaint(&mut self)
        pub fn request_screenshot(&mut self, region: Option<Rect>) -> Option<ScreenshotId>
        pub fn screenshot_taken(&self, id: ScreenshotId) -> Option<&Screenshot>
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
        pub fn max_image_dimension(&self) -> Option<NonZeroU32>
        pub const fn device_restored(&self) -> bool
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
        pub fn animate<V: Animatable>(&mut self, request: AnimationRequest<V>, spec: impl Into<Option<AnimationSpec>>) -> V
        pub const fn focus(&self) -> Option<WidgetId>
        pub fn is_focus_within(&self, ancestor: WidgetId) -> bool
        pub fn is_hover_within(&self, ancestor: WidgetId) -> bool
        pub const fn display(&self) -> Display
        pub fn user_scale(&self) -> UserScale
        pub fn set_user_scale(&mut self, scale: UserScale)
        pub const fn frame_id(&self) -> u64
        pub fn inspect(&self, out: &mut Vec<InspectedWidget>)
        pub fn double_click_window(&self) -> Duration
        pub const fn render_frame_id(&self) -> u64
        pub fn probe_text<'a>(&'a mut self, run: TextRun<'a>) -> TextProbe<'a>
        pub fn pointer_actions(&self) -> impl Iterator<Item = PointerAction> + '_
        pub const fn set_focus(&mut self, id: WidgetId)
        pub const fn clear_focus(&mut self)
        pub const fn request_ime(&mut self, caret: Rect)
        pub const fn ime_area(&self) -> Option<Rect>
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

UnmetRequirements
    enum UnmetRequirements: Clone, Debug, Display, Eq, Error, PartialEq
        Features { missing: DeviceFeatures }
        Limit { name: &'static str, required: u64, available: u64 }

UserScale
    struct UserScale: Clone, Copy, Debug, Default, PartialEq, PartialOrd
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

VAlign
    enum VAlign: Clone, Copy, Debug, Default, Eq, PartialEq
        Auto = 0
        Top = 1
        Center = 2
        Bottom = 3
        Stretch = 4

ValueResponse
    struct ValueResponse<'a>: Debug
        pub response: Response<'a>
        pub changed: bool
        pub committed: bool

Visibility
    enum Visibility: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, Ord, PartialEq, PartialOrd
        Visible = 0
        Hidden = 1
        Collapsed = 2
        pub const fn is_visible(self) -> bool
        pub const fn is_collapsed(self) -> bool

Vsync
    enum Vsync: Clone, Copy, Debug, Default, Eq, PartialEq
        On
        Off

WidgetId
    struct WidgetId: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq, Pod, Zeroable
        pub fn from_hash(h: impl Hash) -> Self
        pub fn with(self, h: impl Hash) -> Self
        pub fn auto() -> Self
        impl<X> From<WidgetId> for Locator<X>

WidgetLook
    struct WidgetLook: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub chrome: Chrome
        pub text: TextStyleOverrides
        pub fn to_animated(&self, ambient_text: TextStyle) -> AnimatedLook

WindowCommand
    enum WindowCommand: Debug
        Open { token: WindowToken, config: WindowConfig }
        Close(WindowToken)

WindowConfig
    struct WindowConfig: Clone, Debug, Default
        pub title: String
        pub inner_size: Option<UVec2>
        pub min_inner_size: Option<UVec2>
        pub placement: WindowPlacement
        pub icon: Option<Image>
        pub app_id: Option<String>
        pub fn new(title: impl Into<String>) -> Self
        pub const fn with_inner_size(self, size: UVec2) -> Self
        pub const fn with_min_inner_size(self, size: UVec2) -> Self
        pub const fn with_physical_pos(self, pos: IVec2) -> Self
        pub const fn with_placement(self, placement: WindowPlacement) -> Self
        pub const fn with_maximized(self, maximized: bool) -> Self
        pub fn with_icon(self, icon: Image) -> Self
        pub fn with_app_id(self, app_id: impl Into<String>) -> Self

WindowGeometry
    struct WindowGeometry: Clone, Copy, Debug, Default
        pub inner_size: UVec2
        pub placement: WindowPlacement

WindowPlacement
    struct WindowPlacement: Clone, Copy, Debug, Default, Eq, PartialEq
        pub physical_pos: Option<IVec2>
        pub maximized: bool

WindowToken
    struct WindowToken: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        pub 0: u64

WinitHost  [features: winit]
    struct WinitHost<T: 'static>: Debug
        impl<T> WinitHost<T> where T: App + 'static
            pub fn builder(first_token: WindowToken) -> WinitHostBuilder<T>
            pub fn handle(&self) -> HostHandle<T>
            pub fn run(self) -> Result<(), WinitHostError>

WinitHostBuilder  [features: winit]
    struct WinitHostBuilder<T>
        impl<T> WinitHostBuilder<T> where T: App + 'static
            pub fn window(self, window: WindowConfig) -> Self
            pub fn title(self, title: impl Into<String>) -> Self
            pub const fn fonts(self, scope: FontScope) -> Self
            pub fn shaper(self, shaper: TextShaper) -> Self
            pub const fn vsync(self, vsync: Vsync) -> Self
            pub const fn power_preference(self, preference: PowerPreference) -> Self
            pub const fn collect_gpu_stats(self, collect: bool) -> Self
            pub const fn pixel_snap(self, pixel_snap: bool) -> Self
            pub const fn system_clipboard(self, system: bool) -> Self
            pub fn build(self, create_app: impl FnOnce(&mut Ui, HostHandle<T>) -> T + 'static) -> Result<WinitHost<T>, WinitHostError>
        impl<T: Debug> Debug for WinitHostBuilder<T>

WinitHostError  [features: winit]
    #[non_exhaustive]
    enum WinitHostError: Debug, Display, Error
        CreateEventLoop { source: EventLoopError }
        RunEventLoop { source: EventLoopError }
        CreateWindow { token: WindowToken, source: OsError }
        Gpu { source: GpuRequestError }
        DeviceLost { source: GpuRequestError }
        Surface { token: WindowToken, source: SurfaceError }

ZoomConfig
    struct ZoomConfig: Clone, Copy, Debug, Default, PartialEq
        pub const fn new(range: RangeInclusive<f32>, step: f32) -> Self
        pub const fn with_modifier(self, modifier: ZoomModifier) -> Self
        pub const fn with_pivot(self, pivot: ZoomPivot) -> Self
        pub const fn min(self) -> f32
        pub const fn max(self) -> f32
        pub const fn step(self) -> f32
        pub const fn modifier(self) -> ZoomModifier
        pub const fn pivot(self) -> ZoomPivot

ZoomFactor
    struct ZoomFactor: Clone, Copy, Debug, Default, PartialEq, PartialOrd
        pub const ONE: Self
        pub const fn new(factor: f32) -> Option<Self>
        pub fn from_wheel(step: f32, notches: f32) -> Self
        pub fn combine(self, rhs: Self) -> Self
        pub const fn get(self) -> f32

ZoomModifier
    enum ZoomModifier: Clone, Copy, Debug, Eq, PartialEq
        Ctrl
        Always
        PinchOnly

ZoomPivot
    enum ZoomPivot: Clone, Copy, Debug, Eq, PartialEq
        Pointer
        Center

fmt
    macro fmt!

glam
    pub use glam as glam

golden  [features: golden]
    mod

golden::DiffReport  [features: golden]
    struct DiffReport: Debug
        pub max_channel_delta: u8
        pub differing_pixels: u32
        pub diff_image: Image
        pub tolerance: Tolerance
        pub const fn passes(&self) -> bool

golden::Goldens  [features: golden]
    struct Goldens: Clone, Debug
        pub fn new(root: impl Into<PathBuf>) -> Self
        pub fn with_adapter(self, adapter: impl Into<String>) -> Self
        pub fn orphans<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Vec<PathBuf>
        pub const fn with_tolerance(self, tolerance: Tolerance) -> Self
        pub fn assert_matches(&self, name: &str, actual: &Image)
        pub fn assert_same(&self, name: &str, actual: &Image, expected: &Image)

golden::Tolerance  [features: golden]
    struct Tolerance: Clone, Copy, Debug, Default, Eq, PartialEq
        pub max_delta: u8
        pub max_pixels: u32
        pub const EXACT: Self
        pub fn diff(self, actual: &Image, expected: &Image) -> DiffReport

wgpu
    pub use wgpu as wgpu

widget
    mod

widget::Activation
    enum Activation: Clone, Copy, Debug, Eq, PartialEq
        Button
        Toggle

widget::Animatable
    trait Animatable: Clone + PartialEq + 'static
        fn lerp(a: Self, b: Self, t: f32) -> Self
        fn sub(self, other: Self) -> Self
        fn add(self, other: Self) -> Self
        fn scale(self, k: f32) -> Self
        fn magnitude_squared(self) -> f32
        fn settle_distance_squared(self) -> f32 { .. }
        fn zero() -> Self
        fn to_motion(self) -> Self { .. }
        fn from_motion(motion: Self) -> Self { .. }
        fn normalize_for_spring(&mut self, _target: &Self, _velocity: &mut Self) { .. }

widget::AnimationRequest
    struct AnimationRequest<T: Animatable>
        pub id: WidgetId
        pub slot: AnimationSlot
        pub target: T
        pub fn new(id: WidgetId, slot: impl Into<AnimationSlot>, target: T) -> Self
        impl<T: Clone + Animatable> Clone for AnimationRequest<T>
        impl<T: Debug + Animatable> Debug for AnimationRequest<T>

widget::AnimationSlot
    struct AnimationSlot: Clone, Copy, Debug, DockTab (blanket), Eq, From<&'static str>, Hash, PartialEq
        pub const fn new(name: &'static str) -> Self

widget::Backdrop
    enum Backdrop: Clone, Copy, Debug
        None
        Root
        Eater(WidgetId)

widget::Caret
    struct Caret: Clone, Copy, Debug, PartialEq
        pub x: f32
        pub y_top: f32
        pub line_height: f32

widget::CircularArc
    struct CircularArc: Clone, Copy, Debug, PartialEq
        pub center: Vec2
        pub radius: f32
        pub start_angle: f32
        pub sweep: f32

widget::Configurator
    struct Configurator<'a>: Debug
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
        pub fn pos(&mut self, p: impl Into<Vec2>) -> &mut Self
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
        pub const fn tab_idx(&mut self, idx: i16) -> &mut Self
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
        pub const fn default_child_align(&mut self, a: Align) -> &mut Self
        pub fn default_gap(&mut self, g: f32) -> &mut Self
        pub fn default_min_size(&mut self, s: impl Into<Size>) -> &mut Self
        pub fn default_max_size(&mut self, s: impl Into<Size>) -> &mut Self
        pub fn default_clip(&mut self, mode: ClipMode) -> &mut Self

widget::ContentType
    enum ContentType: Clone, Copy, Debug, Eq, PartialEq
        Mask = 0
        Color = 1

widget::CubicBezier
    struct CubicBezier: Clone, Copy, Debug, From<QuadraticBezier>, PartialEq
        pub p0: Vec2
        pub p1: Vec2
        pub p2: Vec2
        pub p3: Vec2

widget::CurveShape
    struct CurveShape: Clone, Debug, Lower (blanket)
        pub const fn ramp(self, ramp: ColorRamp) -> Self
        pub const fn cap(self, cap: LineCap) -> Self

widget::GlyphFont
    struct GlyphFont: Clone, Copy, Debug, PartialEq
        pub size: f32
        pub line_height: f32
        pub family: FontFamily
        pub weight: FontWeight
        pub slant: FontSlant
        pub const fn new(size: f32) -> Self

widget::GlyphLine
    struct GlyphLine<'a>: Clone, Copy, Debug
        pub text: &'a str
        pub font: GlyphFont
        pub raster_factor: f32

widget::GlyphRasterKey
    struct GlyphRasterKey: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq

widget::IconFit
    enum IconFit: Clone, Copy, Debug, Default, Eq, PartialEq
        Contain
        Fill
        None

widget::IconShape
    struct IconShape: Clone, Copy, Debug, Lower (blanket)
        pub const fn at(self, rect: Rect) -> Self
        pub const fn fit(self, fit: IconFit) -> Self
        pub const fn tint(self, tint: RgbaF32) -> Self
        pub const fn desaturate(self, desaturate: bool) -> Self

widget::ImageShape
    struct ImageShape: Clone, Debug, Lower (blanket)
        pub const fn at(self, rect: Rect) -> Self
        pub const fn fit(self, fit: ImageFit) -> Self
        pub const fn min_filter(self, min_filter: ImageFilter) -> Self
        pub const fn mag_filter(self, mag_filter: ImageFilter) -> Self
        pub const fn downsample(self, downsample: ImageDownsample) -> Self
        pub const fn tint(self, tint: RgbaF32) -> Self

widget::LineCap
    enum LineCap: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
        Butt = 0
        Square = 1
        Round = 2

widget::LineJoin
    enum LineJoin: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
        Miter = 0
        Bevel = 1
        Round = 2

widget::LookPlan
    struct LookPlan: Debug

widget::Lower
    trait Lower: LowerShape
        impl<T: LowerShape> Lower for T

widget::Mesh
    struct Mesh: Clone, Debug, Default
        pub const fn new() -> Self
        pub fn with_capacity(vertices: usize, idxs: usize) -> Self
        pub fn clear(&mut self)
        pub const fn is_noop(&self) -> bool
        pub fn content_hash(&self) -> u64
        pub fn vertex(&mut self, pos: Vec2, color: impl Into<RgbaF32>) -> u32
        pub fn triangle(&mut self, a: u32, b: u32, c: u32)
        pub fn append(&mut self, other: &Self)
        pub fn bbox(&self) -> Rect
        pub fn filled_triangle(corners: [Vec2; 3], color: impl Into<RgbaF32>) -> Self
        pub fn filled_polygon(points: &[Vec2], color: impl Into<RgbaF32>) -> Self

widget::MeshShape
    struct MeshShape<'a>: Clone, Debug, Lower (blanket)
        pub const fn at(self, rect: Rect) -> Self
        pub const fn tint(self, tint: RgbaF32) -> Self

widget::MeshVertex
    struct MeshVertex: Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable
        pub pos: Vec2
        pub color: SrgbaU8
        pub fn new(pos: Vec2, color: impl Into<RgbaF32>) -> Self

widget::OverlayScope
    struct OverlayScope: Debug
        pub fn claim(ui: &mut Ui, spec: OverlaySpec, root: &mut Widget) -> Self
        pub fn record<R>(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayTurn<R>
        pub fn finish<R>(self, ui: &mut Ui, response: OverlayResponse<R>) -> OverlayResponse<R>
        pub fn withdraw(self, ui: &mut Ui)

widget::OverlaySpec
    struct OverlaySpec: Clone, Copy, Debug
        pub layer: Layer
        pub anchor: Option<Anchor>
        pub backdrop: Backdrop

widget::OverlayTurn
    struct OverlayTurn<R>
        pub inner: R
        pub escape_pressed: bool
        pub pressed_outside: bool
        impl<R: Debug> Debug for OverlayTurn<R>

widget::PaintAnimation
    struct PaintAnimation: Clone, Copy, Debug
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

widget::PaintChannel
    struct PaintChannel: Clone, Copy, Debug, PartialEq
        pub alpha: Option<PaintRange>
        pub turn: Option<PaintRange>

widget::PaintCurve
    type PaintCurve = fn(f32) -> f32

widget::PaintRange
    struct PaintRange: Clone, Copy, Debug, PartialEq
        pub from: f32
        pub to: f32

widget::PaintRepeat
    enum PaintRepeat: Clone, Copy, Debug, Eq, PartialEq
        Once
        Forever
        Settle(Duration)

widget::PaintSteps
    enum PaintSteps: Clone, Copy, Debug, Eq, PartialEq
        Continuous
        Steps(NonZeroU32)

widget::PaintTiming
    struct PaintTiming: Clone, Copy, Debug, Eq, PartialEq
        pub started_at: Duration
        pub period: Duration
        pub repeat: PaintRepeat
        pub steps: PaintSteps

widget::PlacedGlyph
    struct PlacedGlyph: Clone, Copy, Debug
        pub raster_key: GlyphRasterKey
        pub physical: IVec2

widget::PolylineShape
    struct PolylineShape<'a>: Clone, Debug, Lower (blanket)
        pub const fn per_point(self, colors: &'a [RgbaF32]) -> Self
        pub const fn per_segment(self, colors: &'a [RgbaF32]) -> Self
        pub const fn cap(self, cap: LineCap) -> Self
        pub const fn join(self, join: LineJoin) -> Self

widget::QuadraticBezier
    struct QuadraticBezier: Clone, Copy, Debug, PartialEq
        pub p0: Vec2
        pub p1: Vec2
        pub p2: Vec2
        impl From<QuadraticBezier> for CubicBezier

widget::RasterImage
    struct RasterImage<'a>: Clone, Copy, Debug
        pub content: ContentType
        pub size: UVec2
        pub bearing: IVec2
        pub data: &'a [u8]

widget::RectShape
    struct RectShape: Clone, Debug, Lower (blanket)
        pub fn fill(self, fill: impl Into<Brush>) -> Self
        pub const fn border(self, border: Stroke) -> Self
        pub fn corners(self, corners: impl Into<Corners>) -> Self
        pub const fn backdrop_filter(self, filter: BackdropFilter) -> Self

widget::ShadowShape
    struct ShadowShape: Clone, Debug, Lower (blanket)
        pub const fn at(self, rect: Rect) -> Self
        pub fn corners(self, corners: impl Into<Corners>) -> Self

widget::Shape
    struct Shape: Clone, Copy, Debug
        pub const fn rect(rect: Rect) -> RectShape
        pub const fn owner_rect() -> RectShape
        pub const fn windowed_rect(rect: Rect) -> RectShape
        pub const fn owner_windowed_rect() -> RectShape
        pub const fn triangle(a: Vec2, b: Vec2, c: Vec2) -> TriangleShape
        pub const fn line(a: Vec2, b: Vec2, stroke: Stroke) -> CurveShape
        pub fn polyline(points: &[Vec2], stroke: Stroke) -> PolylineShape<'_>
        pub const fn cubic_bezier(bezier: CubicBezier, stroke: Stroke) -> CurveShape
        pub const fn quadratic_bezier(bezier: QuadraticBezier, stroke: Stroke) -> CurveShape
        pub const fn arc(arc: CircularArc, stroke: Stroke) -> CurveShape
        pub const fn circle(center: Vec2, radius: f32, stroke: Stroke) -> CurveShape
        pub const fn text(text: InternedStr, font: GlyphFont) -> TextShape
        pub const fn shadow(shadow: Shadow) -> ShadowShape
        pub fn image(handle: ImageHandle) -> ImageShape
        pub fn icon(handle: IconHandle) -> IconShape
        pub const fn mesh(mesh: &Mesh) -> MeshShape<'_>

widget::Span
    struct Span: Clone, Copy, Debug, Default, Eq, From<Range<u32>>, From<Range<usize>>, PartialEq, Pod, Zeroable
        pub start: u32
        pub len: u32
        pub const fn new(start: u32, len: u32) -> Self
        pub const fn range(self) -> Range<usize>
        impl From<Span> for Range<u32>
        impl From<Span> for Range<usize>

widget::TextGlyphs
    struct TextGlyphs<'a>: Debug
        pub fn line(&mut self, line: GlyphLine<'_>, out: &mut Vec<PlacedGlyph>)
        pub fn measure(&mut self, text: &str, font: GlyphFont) -> Size
        pub fn rasterize(&mut self, glyph: GlyphRasterKey) -> Option<RasterImage<'_>>

widget::TextProbe
    struct TextProbe<'a>: Debug
        pub const fn size(&self) -> Size
        pub fn text_hash(&self) -> Option<NonZeroU64>
        pub fn hash_of(text: &str) -> NonZeroU64
        pub fn caret_at(&self, byte_offset: usize) -> Caret
        pub fn byte_at(&self, pos: Vec2) -> usize
        pub fn selection_rects(&self, range: Range<usize>, out: &mut Vec<Rect>)

widget::TextRun
    struct TextRun<'a>: Clone, Copy, Debug
        pub text: &'a str
        pub font: GlyphFont
        pub wrap: TextWrap
        pub align: Align
        pub max_width: Option<f32>

widget::TextShape
    struct TextShape: Clone, Debug, Lower (blanket)
        pub const fn at_origin(self, origin: Vec2) -> Self
        pub const fn color(self, color: RgbaF32) -> Self
        pub const fn wrap(self, wrap: TextWrap) -> Self
        pub const fn align(self, align: Align) -> Self
        pub const fn family(self, family: FontFamily) -> Self
        pub const fn weight(self, weight: FontWeight) -> Self
        pub const fn slant(self, slant: FontSlant) -> Self

widget::ThemeDefaults
    trait ThemeDefaults: Configure
        fn default_id(self, id: WidgetId) -> Self { .. }
        fn default_size(self, s: impl Into<SizeSpec>) -> Self { .. }
        fn default_padding(self, p: impl Into<Spacing>) -> Self { .. }
        fn default_margin(self, m: impl Into<Spacing>) -> Self { .. }
        fn default_align(self, a: Align) -> Self { .. }
        fn default_child_align(self, a: Align) -> Self { .. }
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
    struct TriangleShape: Clone, Debug, Lower (blanket)
        pub const fn fill(self, fill: RgbaF32) -> Self
        pub const fn border(self, border: Stroke) -> Self
        pub const fn radius(self, radius: f32) -> Self

widget::Widget
    struct Widget: Configure, Debug, ThemeDefaults (blanket)
        pub fn leaf() -> Self
        pub fn hstack() -> Self
        pub fn vstack() -> Self
        pub fn stack(axis: Axis) -> Self
        pub fn wrap_hstack() -> Self
        pub fn wrap_vstack() -> Self
        pub fn wrap_stack(axis: Axis) -> Self
        pub fn zstack() -> Self
        pub fn canvas() -> Self
        pub fn grid() -> Self
        pub fn resolve(&mut self, ui: &mut Ui) -> WidgetId
        pub fn response(&mut self, ui: &mut Ui) -> ResponseSnapshot
        pub fn activation_response(&mut self, ui: &mut Ui, activation: Activation) -> ResponseSnapshot
        pub fn wear(&mut self, ui: &mut Ui, plan: LookPlan) -> AnimatedLook
        pub fn shortcut_pressed(&mut self, ui: &mut Ui, shortcut: impl Into<Shortcut>) -> bool
        pub fn key_pressed(&mut self, ui: &mut Ui, key: Key) -> bool
        pub fn record<R>(self, ui: &mut Ui, chrome: Option<&Chrome>, body: impl FnOnce(&mut Ui) -> R) -> R
        pub fn show<'a, R>(self, ui: &'a mut Ui, chrome: Option<&Chrome>, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'a, R>
        pub const fn authored_size(&self) -> Option<SizeSpec>
        pub const fn authored_min_size(&self) -> Option<Size>
        pub const fn authored_max_size(&self) -> Option<Size>
        pub const fn authored_padding(&self) -> Option<Spacing>
        pub const fn authored_margin(&self) -> Option<Spacing>
        pub const fn authored_transform(&self) -> TranslateScale
        pub const fn authored_pos(&self) -> Vec2
        pub const fn authored_grid_cell(&self) -> GridCell
        pub const fn authored_gap(&self) -> Option<f32>
        pub const fn authored_line_gap(&self) -> Option<f32>
        pub const fn authored_justify(&self) -> Justify
        pub const fn authored_align(&self) -> Align
        pub const fn authored_child_align(&self) -> Align
        pub const fn authored_sense(&self) -> Sense
        pub const fn authored_disabled(&self) -> bool
        pub const fn authored_focusable(&self) -> bool
        pub const fn authored_tab_stop(&self) -> bool
        pub const fn authored_arrow_focus(&self) -> Option<Axis>
        pub const fn authored_tab_idx(&self) -> i16
        pub const fn authored_input_scope(&self) -> KeyFilter
        pub const fn authored_visibility(&self) -> Visibility
        pub const fn authored_clip(&self) -> Option<ClipMode>
        pub fn grid_tracks(&mut self, ui: &mut Ui, rows: &[Track], cols: &[Track])

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
    const EPS: f32

widget::domain::MAX_FILE_LENGTH
    const MAX_FILE_LENGTH: f32

widget::domain::MAX_GAP
    const MAX_GAP: f32

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

widget::domain::factor
    const fn factor(v: f32) -> f32

widget::domain::fraction
    const fn fraction(v: f32) -> f32

widget::domain::fraction_or
    const fn fraction_or(v: f32, fallback: f32) -> f32

widget::domain::gap
    const fn gap(v: f32) -> f32

widget::domain::idx
    const fn idx(i: usize, len: usize) -> Option<usize>

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

widget::domain::is_factor
    const fn is_factor(v: f32) -> bool

widget::domain::is_fraction
    const fn is_fraction(v: f32) -> bool

widget::domain::is_gap
    const fn is_gap(v: f32) -> bool

widget::domain::is_invisible
    const fn is_invisible(v: f32) -> bool

widget::domain::is_lane
    const fn is_lane(v: f32) -> bool

widget::domain::is_length
    const fn is_length(v: f32) -> bool

widget::domain::is_offset
    const fn is_offset(v: f32) -> bool

widget::domain::is_positive
    const fn is_positive(v: f32) -> bool

widget::domain::is_power_of_two_in
    const fn is_power_of_two_in(n: u32, max: u32) -> bool

widget::domain::lane
    const fn lane(v: f32) -> f32

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

widget::domain::vec2::is_lane
    const fn is_lane(v: Vec2) -> bool

widget::domain::vec2::is_length
    const fn is_length(v: Vec2) -> bool

widget::domain::vec2::is_offset
    const fn is_offset(v: Vec2) -> bool

widget::domain::vec2::lane
    const fn lane(v: Vec2) -> Vec2

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
