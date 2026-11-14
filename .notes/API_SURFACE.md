# Public API surface

Every item the crate exports, rendered as its declaration from rustdoc JSON
(`cargo drive surface`), sorted by path. The surface is the union of a build with
no features and one build per public feature; a tag names the builds an item or a
line needs or loses. `internals` is the crate's own test surface and is left out.

```text
Action
    enum Action<X = Infallible>
        Hover(Locator<X>)
        Click { at: Locator<X>, button: PointerButton, mods: ShortcutMods }
        DoubleClick(Locator<X>)
        Drag { from: Locator<X>, to: Locator<X>, button: PointerButton, mods: ShortcutMods, steps: NonZeroU32 }
        ScrollPixels { at: Locator<X>, delta: Vec2 }
        ScrollLines { at: Locator<X>, delta: Vec2 }
        Pinch { at: Locator<X>, factor: ZoomFactor }
        Key(Shortcut)
        KeyDown(Shortcut)
        KeyUp(Shortcut)
        Type(String)
        InsertText(String)
        Wait(Duration)
        impl Action
            pub const DRAG_STEPS: NonZeroU32
            pub fn click(at: impl Into<Locator>) -> Self
        impl<X: Clone> Clone for Action<X>
        impl<X: Debug> Debug for Action<X>
        impl<X: PartialEq> PartialEq for Action<X>

AddedShape
    struct AddedShape<'u>: Debug
        pub fn animate(self, animation: PaintAnimation)

AgentWindow
    trait AgentWindow<A>
        fn play<X: Locate<A>>(&mut self, actions: &[Action<X>]) -> Result<u32, StepError>
        fn settle(&mut self) -> Result<u32, PlayError>
        fn shot(&mut self, region: Option<Rect>) -> Result<Screenshot, PlayError>
        fn read<R: Send + 'static>(&mut self, f: impl FnOnce(&A, &Ui) -> R + Send + 'static) -> Result<R, PlayError>
        fn query<R: Send + 'static>(&mut self, f: impl FnOnce(&mut A, &mut Ui) -> R + Send + 'static) -> Result<R, PlayError>
        fn lose_device(&mut self) -> Result<(), PlayError>
        fn find<X: Locate<A>>(&mut self, at: &Locator<X>) -> Result<Found, PlayError> { .. }
        fn widgets(&mut self, filter: Option<&str>) -> Result<Vec<InspectedWidget>, PlayError> { .. }

Align
    struct Align: Clone, Copy, Debug, Default, DockTab (blanket), Eq, From<(HAlign, VAlign)>, From<HAlign>, From<VAlign>, Hash, PartialEq
        pub const fn new(h: HAlign, v: VAlign) -> Self
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
    struct AllowedSplits: BitOr, Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
        pub const ROW: Self
        pub const COL: Self
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

Anchor
    struct Anchor: Clone, Copy, Debug, PartialEq
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

Animated
    struct Animated<T: Animatable>
        pub value: T
        pub settled: bool
        impl<T: Clone + Animatable> Clone for Animated<T>
        impl<T: Copy + Animatable> Copy for Animated<T>
        impl<T: Debug + Animatable> Debug for Animated<T>
        impl<T: Eq + Animatable> Eq for Animated<T>
        impl<T: PartialEq + Animatable> PartialEq for Animated<T>

AnimationKey
    struct AnimationKey: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        pub fn of(cause: impl Hash) -> Self

AnimationRequest
    struct AnimationRequest<T: Animatable>
        pub id: WidgetId
        pub slot: AnimationSlot
        pub target: T
        pub key: Option<AnimationKey>
        pub motion: MotionRole
        pub fn new(id: WidgetId, slot: impl Into<AnimationSlot>, target: T) -> Self
        pub const fn with_key(self, key: AnimationKey) -> Self
        pub const fn with_motion(self, motion: MotionRole) -> Self
        impl<T: Clone + Animatable> Clone for AnimationRequest<T>
        impl<T: Debug + Animatable> Debug for AnimationRequest<T>

AnimationSlot
    struct AnimationSlot: Clone, Copy, Debug, DockTab (blanket), Eq, From<&'static str>, Hash, PartialEq
        pub const fn new(name: &'static str) -> Self

AnimationSpec
    struct AnimationSpec: Clone, Copy, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub const FAST: Self
        pub const MEDIUM: Self
        pub const THEME: Self
        pub const SNAP: Self
        pub const SPRING: Self
        pub const fn duration(length: Duration, ease: Easing) -> Self
        pub fn spring(stiffness: f32, damping: f32) -> Self
        pub const fn is_instant(&self) -> bool

App
    trait App
        fn record(&mut self, window: WindowToken, ui: &mut Ui)
        fn is_busy(&self) -> bool { .. }
        impl<F: FnMut(&mut Ui)> App for F

ArcShape
    struct ArcShape: Clone, Copy, Debug, PaintedBy<Stroke>
        pub const fn at(self, rect: Rect) -> Self

Axis
    enum Axis: Clone, Copy, Debug, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        X
        Y

BackdropEffect
    struct BackdropEffect: Clone, Copy, Debug, DockTab (blanket), EffectKind, Eq, Hash, PartialEq

BackdropFallback
    enum BackdropFallback: Clone, Debug, Default, Deserialize<'de>, Lerp, PartialEq, Serialize
        ThemeSurface
        Brush(Brush)

BackdropReach
    enum BackdropReach: Clone, Copy, Debug, PartialEq
        Px(f32)
        Lens(Lens)
        pub const NONE: Self

BarMode
    enum BarMode: Clone, Copy, Debug, Default, Eq, PartialEq
        Reserved
        Overlay
        Hidden

BatchKind
    enum BatchKind: Clone, Copy, Debug, Eq, PartialEq
        Setup = 0
        PreClear = 1
        Quads = 2
        Shadows = 3
        Text = 4
        Mesh = 5
        Image = 6
        Curve = 7
        Icon = 8
        Effect = 9
        pub const ALL: [Self; 10]
        pub const fn label(self) -> &'static str

Block
    struct Block: Configure, ConfigureDefaults (blanket), Debug
        pub fn new() -> Self
        pub fn show(self, ui: &mut Ui) -> Response
        pub const fn chrome(self, chrome: Chrome) -> Self

Border
    struct Border: Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub color: RgbaF32
        pub width: f32
        pub const NONE: Self
        pub const fn is_noop(&self) -> bool
        pub const fn new(color: RgbaF32, width: f32) -> Self
        impl From<Border> for Chrome
        impl From<Border> for Paint

BoundNum
    struct BoundNum<'a>: Debug, From<&'a mut f32>, From<&'a mut f64>, From<&'a mut i32>, From<&'a mut i64>

Brush
    enum Brush: Animatable, Clone, Debug, Default, Deserialize<'de>, From<Gradient>, From<RgbaF32>, From<SrgbaU8>, Lerp, PartialEq, Serialize
        Solid(RgbaF32)
        Gradient(Gradient)
        pub const TRANSPARENT: Self
        pub const fn is_noop(&self) -> bool
        pub const fn as_solid(&self) -> Option<RgbaF32>
        impl From<Brush> for Chrome
        impl From<Brush> for Paint

Button
    struct Button<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a ButtonTheme>>) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub const fn text_wrap(self, wrap: TextWrap) -> Self
        pub fn text_align(self, a: impl Into<Align>) -> Self
        pub const fn selected(self, selected: bool) -> Self
        pub fn show(self, ui: &mut Ui) -> Response
        pub fn show_with<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &TextStyle) -> R) -> InnerResponse<R>

ButtonTheme
    struct ButtonTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize, ThemeSlot
        pub unselected: StatefulLook
        pub selected: StatefulLook
        pub defaults: SlotDefaults
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self
        pub fn accent(p: &Palette, m: &Metrics) -> Self
        pub fn subtle(p: &Palette, m: &Metrics) -> Self

Caret
    struct Caret: Clone, Copy, Debug, PartialEq
        pub x: f32
        pub y_top: f32
        pub line_height: f32

Checkbox
    struct Checkbox<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(value: &'a mut bool) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ToggleTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse

Chrome
    struct Chrome: Animatable, Clone, Debug, Default, Deserialize<'de>, From<Border>, From<Brush>, From<Gradient>, From<Paint>, From<RgbaF32>, From<Shadow>, From<SrgbaU8>, Lerp, PartialEq, Serialize
        pub corners: Corners
        pub paint: Paint
        pub const NONE: Self
        pub const fn is_noop(&self) -> bool
        pub fn new(paint: impl Into<Paint>) -> Self
        pub fn rounded(paint: impl Into<Paint>, corners: impl Into<Corners>) -> Self
        pub fn with_corners(self, corners: impl Into<Corners>) -> Self
        pub fn with_paint(self, paint: impl Into<Paint>) -> Self
        pub fn with_fill(self, brush: impl Into<Brush>) -> Self
        pub const fn with_border(self, border: Border) -> Self
        pub const fn with_shadow(self, shadow: Shadow) -> Self
        impl From<Chrome> for WidgetLook

CircleShape
    struct CircleShape: Clone, Copy, Debug, PaintedBy<ImagePaint>, PaintedBy<Material>, PaintedBy<MaterialPaint>, PaintedBy<Stroke>
        pub const fn at(self, rect: Rect) -> Self
        impl<P: Into<Paint>> PaintedBy<P> for CircleShape
        impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for CircleShape

ClickOutside
    enum ClickOutside: Clone, Copy, Debug, Eq, PartialEq
        Block
        Dismiss

ClipMode
    enum ClipMode: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, Ord, PartialEq, PartialOrd, Serialize
        None = 0
        Rect = 1
        Rounded = 2
        pub const fn is_clip(self) -> bool

Clipboard
    struct Clipboard: Clone, Debug
        pub fn text(&self) -> Result<Option<String>, ClipboardError>
        pub fn set_text(&self, text: &str)
        pub fn image(&self) -> Result<Option<Image>, ClipboardError>
        pub fn set_image(&self, image: &Image)

ClipboardError
    struct ClipboardError: Clone, Copy, Debug, Display, Eq, Error, PartialEq

CloseShown
    enum CloseShown: Clone, Copy, Debug, Default, Deserialize<'de>, Eq, PartialEq, Serialize
        Always
        SelectedOrHovered

ColorButton
    struct ColorButton<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(picker: ColorPicker<'a>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse
        pub fn open(ui: &mut Ui, id: WidgetId)
        pub fn close(ui: &mut Ui, id: WidgetId)
        pub fn is_open(ui: &Ui, id: WidgetId) -> bool

ColorCoords
    struct ColorCoords: Clone, Copy, Debug, Default, PartialEq
        pub const fn new(model: ColorModel, h: f32, s: f32, v: f32) -> Self
        pub fn from_color(color: RgbaF32, model: ColorModel, fallback_hue: f32) -> Self
        pub const fn model(self) -> ColorModel
        pub fn to_color(self) -> RgbaF32
        pub fn with_model(self, model: ColorModel) -> Self
        pub const fn hue(self) -> f32
        pub const fn saturation(self) -> f32
        pub const fn value(self) -> f32
        pub const fn with_hue(self, h: f32) -> Self
        pub const fn with_saturation(self, s: f32) -> Self
        pub const fn with_value(self, v: f32) -> Self

ColorEffect
    struct ColorEffect: Clone, Copy, Debug, DockTab (blanket), EffectKind, Eq, Hash, PartialEq

ColorField
    struct ColorField<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(coords: &'a mut ColorCoords) -> Self
        pub const fn texel_size(self, n: u32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse

ColorModel
    enum ColorModel: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Okhsv
        Hsv
        pub const ALL: [Self; 2]

ColorPicker
    struct ColorPicker<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(color: &'a mut RgbaF32) -> Self
        pub const fn alpha(self, alpha: bool) -> Self
        pub const fn model(self, model: ColorModel) -> Self
        pub const fn swatches(self, row: SwatchRow<'a>) -> Self
        pub const fn texel_size(self, n: u32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse

ColorPickerTheme
    struct ColorPickerTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
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
        pub chip: ButtonTheme
        pub disabled_veil: RgbaF32
        pub gap: f32
        pub popup: PopupTheme
        pub value: DragValueTheme
        pub hex: TextEditTheme
        pub label: TextStyleOverrides
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

ColorRamp
    struct ColorRamp: Clone, Copy, Debug, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        pub stops: GradientStops
        pub interpolation: Interpolation
        pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self
        pub fn evenly(colors: impl IntoIterator<Item = RgbaF32>) -> Self
        pub fn color_at(&self, t: f32) -> RgbaF32
        pub const fn with_interpolation(self, interpolation: Interpolation) -> Self
        pub const fn is_noop(&self) -> bool
        impl<const N: usize> From<[RgbaF32; N]> for ColorRamp
        impl<const N: usize> From<[Stop; N]> for ColorRamp

ColorScheme
    enum ColorScheme: Clone, Copy, Debug, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Light
        Dark
        pub const FALLBACK: Self
        pub const ALL: [Self; 2]

ColorStrip
    struct ColorStrip<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn hue(coords: &'a mut ColorCoords) -> Self
        pub fn alpha(color: &'a mut RgbaF32) -> Self
        pub const fn texel_size(self, n: u32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse

ColorSwatch
    struct ColorSwatch<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(color: RgbaF32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response

ComboBox
    struct ComboBox<'a, T, I, L>: Configure, ConfigureDefaults (blanket)
        impl<'a, T, I> ComboBox<'a, T, I, fn(&T) -> &str> where T: PartialEq + AsRef<str>, I: IntoIterator<Item = T> + Clone
            pub fn new(selected: &'a mut T, options: I) -> Self
        impl<'a, T, I, L> ComboBox<'a, T, I, L> where T: PartialEq, I: IntoIterator<Item = T> + Clone, L: Fn(&T) -> &str
            pub fn labeled(selected: &'a mut T, options: I, label: L) -> Self
            pub fn style(self, s: impl Into<Option<&'a ComboBoxTheme>>) -> Self
            pub fn button_style(self, s: impl Into<Option<&'a ButtonTheme>>) -> Self
            pub fn menu_style(self, s: impl Into<Option<&'a MenuTheme>>) -> Self
            pub fn show(self, ui: &mut Ui) -> ValueResponse
        impl<'a, T: Debug, I: Debug, L: Debug> Debug for ComboBox<'a, T, I, L>

ComboBoxList
    struct ComboBoxList: Debug
        pub fn menu_id(combo: WidgetId) -> WidgetId
        pub fn open(ui: &mut Ui, id: WidgetId)
        pub fn close(ui: &mut Ui, id: WidgetId)
        pub fn is_open(ui: &Ui, id: WidgetId) -> bool

ComboBoxTheme
    struct ComboBoxTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub gap: f32
        pub arrow_size: Size
        pub arrow_width: f32
        pub const fn from_parts(_p: &Palette, m: &Metrics) -> Self

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
        fn at(self, p: impl Into<Vec2>) -> Self { .. }
        fn grid_cell(self, cell: impl Into<GridCell>) -> Self { .. }
        fn adopt_placement(self, from: &Widget) -> Self { .. }
        fn gap(self, g: f32) -> Self { .. }
        fn line_gap(self, g: f32) -> Self { .. }
        fn justify(self, j: Justify) -> Self { .. }
        fn align(self, a: impl Into<Align>) -> Self { .. }
        fn child_align(self, a: impl Into<Align>) -> Self { .. }
        fn sense(self, s: Sense) -> Self { .. }
        fn add_sense(self, s: Sense) -> Self { .. }
        fn disabled(self, disabled: bool) -> Self { .. }
        fn focusable(self, focusable: bool) -> Self { .. }
        fn tab_stop(self, tab_stop: bool) -> Self { .. }
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
    struct ConicGeometry: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub center: Vec2
        pub start_angle: f32

ContentHash
    struct ContentHash: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, NoUninit, Ord, PartialEq, PartialOrd

ContentType
    enum ContentType: Clone, Copy, Debug, Eq, PartialEq
        Mask = 0
        Color = 1

Corners
    struct Corners: Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, Lerp, PartialEq, Pod, Serialize, Zeroable
        pub const ZERO: Self
        pub fn to_array(self) -> [f32; 4]
        pub fn top_left(self) -> f32
        pub fn top_right(self) -> f32
        pub fn bottom_right(self) -> f32
        pub fn bottom_left(self) -> f32
        pub fn all(r: f32) -> Self
        pub fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self
        pub fn top(r: f32) -> Self
        pub fn bottom(r: f32) -> Self
        pub fn left(r: f32) -> Self
        pub fn right(r: f32) -> Self
        pub fn scaled_by(self, factor: f32) -> Self
        pub const fn is_approx_zero(&self) -> bool
        impl<T: Number> From<T> for Corners
        impl<TL: Number, TR: Number, BR: Number, BL: Number> From<(TL, TR, BR, BL)> for Corners

CubicBezier
    struct CubicBezier: Clone, Copy, Debug, From<QuadraticBezier>, PartialEq
        pub p0: Vec2
        pub p1: Vec2
        pub p2: Vec2
        pub p3: Vec2

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

CurveShape
    struct CurveShape: Clone, Copy, Debug, PaintedBy<Stroke>
        pub const fn at(self, rect: Rect) -> Self
        pub const fn fit(self, fit: Fit) -> Self

DebugOverlayConfig
    struct DebugOverlayConfig: Clone, Copy, Debug, Default, Eq, PartialEq
        pub damage_rect: bool
        pub dim_undamaged: bool
        pub frame_stats: bool

DeviceRequirements
    struct DeviceRequirements: Clone, Debug
        pub features: Features
        pub limits: Limits
        pub const REQUIRED_FEATURES: Features
        pub const REQUIRED_DOWNLEVEL: DownlevelFlags
        pub const GPU_STATS_FEATURES: Features
        pub fn negotiate(adapter: &Adapter, optional: Features) -> Result<Self, DeviceRequirementsError>

DeviceRequirementsError
    #[non_exhaustive]
    enum DeviceRequirementsError: Clone, Debug, Display, Eq, Error, PartialEq
        Features { missing: Features }
        Downlevel { missing: DownlevelFlags }
        Limit { name: &'static str, required: u64, available: u64 }
        impl From<DeviceRequirementsError> for GpuRequestError

Display
    struct Display: Clone, Copy, Debug, Default, PartialEq
        pub const fn physical_size(&self) -> UVec2
        pub const fn system_scale(&self) -> SystemScale
        pub const fn user_scale(&self) -> UserScale
        pub const fn pixel_snap(&self) -> bool
        pub const fn refresh_millihertz(&self) -> Option<NonZeroU32>
        pub fn land_translation(&self, translation: Vec2) -> Vec2
        pub const fn scale_factor(&self) -> f32
        pub fn logical_size(&self) -> Size
        pub fn system_logical_size(&self) -> Size
        pub fn logical_rect(&self) -> Rect

DockDrop
    enum DockDrop: Clone, Copy, Debug, Deserialize<'de>, Eq, PartialEq, Serialize
        Join { group: TabGroupId, idx: usize }
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

DockLayout
    struct DockLayout<T>
        impl<'de, T> Deserialize<'de> where T: Deserialize<'de>
        impl<T: Clone> Clone for DockLayout<T>
        impl<T: Debug> Debug for DockLayout<T>
        impl<T: PartialEq> PartialEq for DockLayout<T>
        impl<T> Serialize where T: Serialize

DockOperation
    enum DockOperation<T>
        ActivateTab { tab: T }
        OpenTab { tab: T }
        CloseTab { tab: T }
        MoveTab { tab: T, to: DockDrop }
        SetRatio { split: DockPath, ratio: f32 }
        FocusGroup { group: TabGroupId }
        impl<'de, T> Deserialize<'de> where T: Deserialize<'de>
        impl<T: Clone> Clone for DockOperation<T>
        impl<T: Copy> Copy for DockOperation<T>
        impl<T: Debug> Debug for DockOperation<T>
        impl<T: PartialEq> PartialEq for DockOperation<T>
        impl<T> Serialize where T: Serialize

DockPath
    struct DockPath: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        pub const ROOT: Self
        pub fn splitter_id(self, dock: WidgetId) -> WidgetId
        pub fn first(self) -> Self
        pub fn second(self) -> Self

DockState
    struct DockState<T>
        impl<T: DockTab> DockState<T>
            pub fn new(pinned: T) -> Self
            pub fn load(layout: DockLayout<T>, pinned: T) -> Result<Self, DockError<T>>
            pub const fn layout(&self) -> &DockLayout<T>
            pub fn with_max_depth(self, depth: u32) -> Self
            pub const fn pinned(&self) -> T
            pub const fn focused(&self) -> TabGroupId
            pub fn groups(&self) -> impl Iterator<Item = &TabGroup<T>>
            pub fn all_tabs(&self) -> impl Iterator<Item = T> + '_
            pub fn active_tabs(&self) -> impl Iterator<Item = T> + '_
            pub fn primary(&self) -> &TabGroup<T>
            pub fn find_tab(&self, tab: T) -> Option<TabAddress>
            pub fn group(&self, id: TabGroupId) -> Option<&TabGroup<T>>
            pub fn apply(&mut self, operation: DockOperation<T>)
            pub fn retain_tabs(&mut self, keep: impl FnMut(T) -> bool)
            pub fn can_split(&self, group: TabGroupId) -> bool
        impl<T: Clone> Clone for DockState<T>
        impl<T: Debug> Debug for DockState<T>
        impl<T: PartialEq> PartialEq for DockState<T>

DockTab
    trait DockTab: Copy + Eq + Hash + Debug + 'static
        impl<T: Copy + Eq + Hash + Debug + 'static> DockTab for T

DockTabMenu
    struct DockTabMenu<'a, T>
        pub tab: T
        pub group: TabGroupId
        pub operations: &'a mut Vec<DockOperation<T>>
        impl<'a, T: Debug> Debug for DockTabMenu<'a, T>

DockTabs
    trait DockTabs
        type Tab: DockTab
        fn label(&mut self, ui: &mut Ui, tab: Self::Tab) -> InternedStr
        fn body(&mut self, ui: &mut Ui, tab: Self::Tab, size: Option<Size>)
        fn closable(&mut self, _tab: Self::Tab) -> bool { .. }
        fn draggable(&mut self, _tab: Self::Tab) -> bool { .. }
        fn chip(&mut self, ui: &mut Ui, _tab: Self::Tab, chip: &TabChip) { .. }
        fn has_tab_menu(&mut self, _tab: Self::Tab) -> bool { .. }
        fn tab_menu(&mut self, _ui: &mut Ui, _menu: DockTabMenu<'_, Self::Tab>) { .. }

DockTheme
    struct DockTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub preview: Chrome
        pub ghost: WidgetLook
        pub ghost_padding: Spacing
        pub ghost_offset: Vec2
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

DockView
    struct DockView<'a, T, M = Applying<'a, T>>: ConfigureDefaults (blanket)
        impl<'a, T: DockTab> DockView<'a, T, Applying<'a, T>>
            pub fn new(dock: WidgetId, state: &'a mut DockState<T>) -> Self
            pub fn show<D: DockTabs<Tab = T>>(self, ui: &mut Ui, tabs: &mut D) -> Response
        impl<'a, T: DockTab> DockView<'a, T, Emitting<'a, T>>
            pub fn emitting(dock: WidgetId, state: &'a DockState<T>, operations: &'a mut Vec<DockOperation<T>>) -> Self
            pub fn show<D: DockTabs<Tab = T>>(self, ui: &mut Ui, tabs: &mut D) -> Response
            pub fn scan(ui: &mut Ui, dock: WidgetId, state: &DockState<T>, operations: &mut Vec<DockOperation<T>>)
        impl<'a, T, M> DockView<'a, T, M>
            pub const fn min_pane(self, px: f32) -> Self
            pub const fn overflow(self, overflow: TabOverflow) -> Self
            pub const fn allowed_splits(self, allowed: AllowedSplits) -> Self
            pub const fn edge_fraction(self, fraction: f32) -> Self
            pub fn style(self, s: impl Into<Option<&'a DockTheme>>) -> Self
            pub fn tabs_style(self, s: impl Into<Option<&'a TabsTheme>>) -> Self
            pub fn menu_style(self, s: impl Into<Option<&'a MenuTheme>>) -> Self
        impl<'a, T: Debug, M: Debug> Debug for DockView<'a, T, M>
        impl<T, M> Configure for DockView<'_, T, M>

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

DragValue
    struct DragValue<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(value: impl Into<BoundNum<'a>>) -> Self
        pub fn text_align(self, a: impl Into<Align>) -> Self
        pub const fn speed(self, speed: f64) -> Self
        pub const fn range(self, range: RangeInclusive<f64>) -> Self
        pub const fn step(self, step: f64) -> Self
        pub const fn decimals(self, n: usize) -> Self
        pub fn suffix(self, text: impl Into<TextInput<'a>>) -> Self
        pub const fn editable(self, editable: bool) -> Self
        pub fn style(self, s: impl Into<Option<&'a DragValueTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse
        pub fn start_editing(ui: &mut Ui, id: WidgetId)
        pub fn stop_editing(ui: &mut Ui, id: WidgetId, end: EditEnd)
        pub fn is_editing(ui: &Ui, id: WidgetId) -> bool

DragValueTheme
    struct DragValueTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize, ThemeSlot
        pub looks: StatefulLook
        pub defaults: SlotDefaults
        pub caret: RgbaF32
        pub caret_width: f32
        pub selection: RgbaF32
        pub placeholder: RgbaF32
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self
        pub fn editor(&self) -> TextEditTheme

Easing
    enum Easing: Clone, Copy, Debug, Deserialize<'de>, Eq, PartialEq, Serialize
        Linear
        OutCubic
        InOutCubic
        OutQuart
        OutBack
        pub const fn apply(self, t: f32) -> f32
        impl From<Easing> for PaintCurve

EditEnd
    enum EditEnd: Clone, Copy, Debug, Eq, PartialEq
        Submitted
        Canceled

EditableLabel
    struct EditableLabel<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(text: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a TextEditTheme>>) -> Self
        pub const fn color(self, color: RgbaF32) -> Self
        pub const fn font_size(self, px: f32) -> Self
        pub const fn line_height_factor(self, factor: f32) -> Self
        pub const fn family(self, family: FontFamily) -> Self
        pub const fn weight(self, weight: FontWeight) -> Self
        pub const fn slant(self, slant: FontSlant) -> Self
        pub const fn bold(self) -> Self
        pub const fn italic(self) -> Self
        pub const fn editable(self, editable: bool) -> Self
        pub const fn max_chars(self, n: usize) -> Self
        pub fn show(self, ui: &mut Ui) -> EditableLabelResponse
        pub fn start_editing(ui: &mut Ui, id: WidgetId)
        pub fn stop_editing(ui: &mut Ui, id: WidgetId, end: EditEnd)
        pub fn is_editing(ui: &Ui, id: WidgetId) -> bool

EditableLabelResponse
    struct EditableLabelResponse: Clone, Debug
        pub response: Response
        pub editing: bool
        pub committed: bool
        pub committed_text: Option<String>
        pub ended: Option<EditEnd>

EffectError
    enum EffectError: Debug, Display, Error
        Parse(WgslError)
        Validation(WgslError)
        MissingEntry
        Entry(EntryMismatch)
        ParamLayout(ParamMismatch)
        ParamsTooLarge { size: usize }
        TooManyImageSlots { slots: u8 }
        Binding { name: String }
        ReservedName

EffectKind
    trait EffectKind: Kind

EffectName
    struct EffectName<U: EffectParams, K: EffectKind = ColorEffect>: DockTab (blanket)
        impl EffectName<PressParams>
            pub const RIPPLE: Self
            pub const SPARKLE: Self
        impl EffectName<FrostedParams, BackdropEffect>
            pub const FROSTED: Self
        impl EffectName<GlassParams, BackdropEffect>
            pub const GLASS: Self
        impl<U: EffectParams, K: EffectKind> EffectName<U, K>
            pub const MAX_NAME_LEN: usize
            pub const fn named(name: &str) -> Option<Self>
            pub fn name(&self) -> &str
        impl<'de, U: EffectParams, K: EffectKind> Deserialize<'de> for EffectName<U, K>
        impl<U: EffectParams, K: EffectKind> Clone for EffectName<U, K>
        impl<U: EffectParams, K: EffectKind> Copy for EffectName<U, K>
        impl<U: EffectParams, K: EffectKind> Debug for EffectName<U, K>
        impl<U: EffectParams, K: EffectKind> Eq for EffectName<U, K>
        impl<U: EffectParams, K: EffectKind> Hash for EffectName<U, K>
        impl<U: EffectParams, K: EffectKind> PartialEq for EffectName<U, K>
        impl<U: EffectParams, K: EffectKind> Serialize for EffectName<U, K>

EffectPaint
    struct EffectPaint<U: EffectParams, K: EffectKind = ColorEffect>
        impl<U: EffectParams, K: EffectKind> EffectPaint<U, K>
            pub const fn new(name: EffectName<U, K>, params: U) -> Self
            pub fn images<const N: usize>(self, images: [ImageHandle; N]) -> Self
        impl<U: EffectParams> EffectPaint<U, ColorEffect>
            pub const fn outside(self, reach: f32) -> Self
        impl<U: EffectParams> EffectPaint<U, BackdropEffect>
            pub const fn backdrop_blur(self, sigma: f32) -> Self
            pub fn fallback(self, fallback: impl Into<Brush>) -> Self
        impl<U: EffectParams, K: EffectKind> Debug for EffectPaint<U, K>
        impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for CircleShape
        impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for RectShape
        impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for TriangleShape

EffectParam
    struct EffectParam: Clone, Copy, Debug, Eq, PartialEq
        pub name: &'static str
        pub offset: usize
        pub ty: ParamType

EffectParams
    pub use palantir_derive::EffectParams as EffectParams

EffectSource
    struct EffectSource<U: EffectParams, K: EffectKind = ColorEffect>
        impl<U: EffectParams, K: EffectKind> EffectSource<U, K>
            pub const MAX_IMAGE_SLOTS: u8
            pub const MAX_PARAMS_SIZE: usize
            pub fn new(wgsl: impl Into<Cow<'static, str>>) -> Self
            pub const fn reach(self, reach: fn(&U) -> BackdropReach) -> Self
            pub const fn prebuild(self, prebuild: bool) -> Self
            pub const fn image_slots(self, slots: u8) -> Self
            pub const fn reduced_motion(self, paint: ReducedEffect<U, K>) -> Self
            pub const fn power_saver(self, paint: ReducedEffect<U, K>) -> Self
        impl<U: EffectParams, K: EffectKind> Debug for EffectSource<U, K>

Engagement
    enum Engagement: Clone, Copy, Debug, Eq, PartialEq
        Pressed
        Focused

EntryMismatch
    enum EntryMismatch: Clone, Copy, Debug, Display, Eq, PartialEq
        ArgumentCount { want: usize, got: usize }
        Stage
        Input
        Params
        Output

ErrorChain
    struct ErrorChain<'a>: Clone, Copy, Debug, Display
        pub 0: &'a dyn Error
        pub const MAX_DEPTH: usize

Expander
    struct Expander<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(label: impl Into<TextInput<'a>>) -> Self
        pub const fn start_open(self, start_open: bool) -> Self
        pub const fn keep_body(self, keep_body: bool) -> Self
        pub fn style(self, s: impl Into<Option<&'a ExpanderTheme>>) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> ExpanderResponse<R>
        pub fn open(ui: &mut Ui, id: WidgetId)
        pub fn close(ui: &mut Ui, id: WidgetId)
        pub fn is_open(ui: &Ui, id: WidgetId) -> bool

ExpanderResponse
    struct ExpanderResponse<R>
        pub response: Response
        pub inner: Option<R>
        pub changed: bool
        pub openness: f32
        impl<R: Clone> Clone for ExpanderResponse<R>
        impl<R: Copy> Copy for ExpanderResponse<R>
        impl<R: Debug> Debug for ExpanderResponse<R>

ExpanderTheme
    struct ExpanderTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize, ThemeSlot
        pub looks: StatefulLook
        pub arrow_size: Size
        pub arrow_radius: f32
        pub arrow_closed_angle: f32
        pub arrow_open_angle: f32
        pub gap: f32
        pub indent: f32
        pub body_padding: Spacing
        pub defaults: SlotDefaults
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

Fit
    enum Fit: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        Natural = 0
        Fill = 1
        Contain = 2
        Cover = 3

FloatMaterial
    struct FloatMaterial: Clone, Debug, Deserialize<'de>, PartialEq, Serialize
        pub material: Material
        pub fill_alpha: f32
        pub fn dress(&self, panel: &mut Chrome, material: &mut Option<Material>, ink: RgbaF32)

FocusPolicy
    enum FocusPolicy: Clone, Copy, Debug, Default, Eq, PartialEq
        PreserveOnMiss
        ClearOnMiss

FocusRingTheme
    struct FocusRingTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
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
    enum FontLoadError: Clone, Copy, Debug, Display, Eq, Error, PartialEq
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
    struct FontSource: Clone, Debug, From<&'static [u8]>, From<Cow<'static, [u8]>>, From<Vec<u8>>
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
        pub const fn center_of(rect: Rect) -> Self

FramePaint
    enum FramePaint: Clone, Copy, Debug, Eq, PartialEq
        Skip
        Full
        Partial

FrameReport
    struct FrameReport: Clone, Debug, PartialEq
        pub repaint_requested: bool
        pub repaint_at: Option<Duration>
        pub earliest_repaint_at: Option<Duration>
        pub const fn paint(&self) -> FramePaint

FrostedParams
    struct FrostedParams: Clone, Copy, Debug, EffectParams, PartialEq, Pod, Zeroable
        pub saturation: f32
        pub noise: f32
        pub const DEFAULT: Self

GlassParams
    struct GlassParams: Clone, Copy, Debug, EffectParams, PartialEq, Pod, Zeroable
        pub saturation: f32
        pub noise: f32
        pub lens: Lens
        pub const fn new(lens: Lens) -> Self

GlyphFont
    struct GlyphFont: Clone, Copy, Debug, PartialEq
        pub font_size: f32
        pub line_height: f32
        pub family: FontFamily
        pub weight: FontWeight
        pub slant: FontSlant
        pub const fn new(font_size: f32) -> Self
        pub const fn with_line_height(self, line_height: f32) -> Self
        pub const fn with_family(self, family: FontFamily) -> Self
        pub const fn with_weight(self, weight: FontWeight) -> Self
        pub const fn with_slant(self, slant: FontSlant) -> Self

GlyphLine
    struct GlyphLine<'a>: Clone, Copy, Debug
        pub text: &'a str
        pub font: GlyphFont
        pub raster_factor: f32

GlyphRasterKey
    struct GlyphRasterKey: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq

Gpu
    struct Gpu: Clone, Debug
        pub fn new(device: Device, queue: Queue) -> Self
        pub const fn device(&self) -> &Device
        pub const fn queue(&self) -> &Queue
        pub fn lose(&self)
        pub fn max_texture_dimension(&self) -> NonZeroU32
        pub fn is_lost(&self) -> bool

GpuFrameCtx
    #[non_exhaustive]
    struct GpuFrameCtx<'a>: Debug
        pub gpu: &'a Gpu
        pub encoder: &'a mut CommandEncoder
        pub target: &'a TextureView
        pub physical_size: UVec2
        pub physical_full_size: UVec2
        pub physical_offset: UVec2
        pub scale_factor: f32
        pub raster_factor: f32
        pub dt: Duration

GpuInitCtx
    #[non_exhaustive]
    struct GpuInitCtx<'a>: Debug
        pub gpu: &'a Gpu
        pub target_format: TextureFormat
        pub text: &'a TextShaper

GpuPaint
    trait GpuPaint: 'static
        fn init(&mut self, ctx: &GpuInitCtx<'_>) { .. }
        fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>)

GpuPassStats
    struct GpuPassStats: Clone, Copy, Debug, Default
        pub const fn last_pass(&self) -> Option<Duration>
        pub const fn last_kind(&self, kind: BatchKind) -> Option<Duration>
        pub const fn last_pipeline_stats(&self) -> Option<PipelineStats>
        pub const fn last_copy_out(&self) -> Option<Duration>
        pub const fn last_main_pass_cpu(&self) -> Option<Duration>

GpuRequestError
    #[non_exhaustive]
    enum GpuRequestError: Debug, Display, Error, From<DeviceRequirementsError>, From<RequestAdapterError>, From<RequestDeviceError>
        NoBackend
        RequestAdapter { source: RequestAdapterError }
        Requirements { source: DeviceRequirementsError }
        RequestDevice { source: RequestDeviceError }

GpuView
    struct GpuView: Configure, ConfigureDefaults (blanket), Debug
        pub fn new<T: GpuPaint + 'static>(paint: &Rc<RefCell<T>>) -> Self
        pub const fn repaint(self, repaint: bool) -> Self
        pub fn show(self, ui: &mut Ui) -> Response

GpuWait
    enum GpuWait: Clone, Copy, Debug, Eq, PartialEq
        Done
        DeviceLost
        TimedOut

Gradient
    struct Gradient: Clone, Debug, Deserialize<'de>, Hash, PartialEq, Serialize
        pub geometry: GradientGeometry
        pub ramp: ColorRamp
        pub spread: Spread
        pub fn new(geometry: GradientGeometry, ramp: impl Into<ColorRamp>) -> Self
        pub fn linear(angle: f32, ramp: impl Into<ColorRamp>) -> Self
        pub fn horizontal(ramp: impl Into<ColorRamp>) -> Self
        pub fn vertical(ramp: impl Into<ColorRamp>) -> Self
        pub fn radial(center: Vec2, radius: Vec2, ramp: impl Into<ColorRamp>) -> Self
        pub fn centered_radial(ramp: impl Into<ColorRamp>) -> Self
        pub fn conic(center: Vec2, start_angle: f32, ramp: impl Into<ColorRamp>) -> Self
        pub fn centered_conic(ramp: impl Into<ColorRamp>) -> Self
        pub const fn with_spread(self, spread: Spread) -> Self
        pub const fn with_interpolation(self, interpolation: Interpolation) -> Self
        pub const fn is_noop(&self) -> bool
        impl From<Gradient> for Brush
        impl From<Gradient> for Chrome
        impl From<Gradient> for Paint

GradientGeometry
    enum GradientGeometry: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        Linear(LinearGeometry)
        Radial(RadialGeometry)
        Conic(ConicGeometry)

GradientStops
    struct GradientStops: Clone, Copy, Debug, Deref, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self
        impl<'a> IntoIterator for &'a GradientStops

Grid
    struct Grid<Rows = [Track; 0], Cols = [Track; 0]>: ConfigureDefaults (blanket)
        impl Grid
            pub fn new() -> Self
        impl<Rows, Cols> Grid<Rows, Cols>
            pub fn rows<NewRows: AsRef<[Track]>>(self, rows: NewRows) -> Grid<NewRows, Cols>
            pub fn cols<NewCols: AsRef<[Track]>>(self, cols: NewCols) -> Grid<Rows, NewCols>
            pub const fn chrome(self, chrome: Chrome) -> Self
            pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> where Rows: AsRef<[Track]>, Cols: AsRef<[Track]>
        impl<Rows, Cols> Configure for Grid<Rows, Cols>
        impl<Rows: Debug, Cols: Debug> Debug for Grid<Rows, Cols>

GridCell
    struct GridCell: Clone, Copy, Debug, Default, DockTab (blanket), Eq, From<(u16, u16)>, Hash, PartialEq
        pub const fn at(row: u16, col: u16) -> Self
        pub const fn with_span(self, row_span: NonZeroU16, col_span: NonZeroU16) -> Self
        pub const fn row(self) -> u16
        pub const fn col(self) -> u16
        pub const fn row_span(self) -> NonZeroU16
        pub const fn col_span(self) -> NonZeroU16
        pub const fn along(axis: Axis, main: u16) -> Self

HAlign
    enum HAlign: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
        Auto = 0
        Left = 1
        Center = 2
        Right = 3
        Stretch = 4
        impl From<HAlign> for Align

Harness
    struct Harness: Debug
        pub const WINDOW: WindowToken
        pub fn new(size: UVec2) -> Self
        pub const fn builder(size: UVec2) -> HarnessBuilder
        pub fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport
        pub fn frame_app(&mut self, app: &mut impl App) -> FrameReport
        pub fn step(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport
        pub fn frame_value<R>(&mut self, record: impl FnMut(&mut Ui) -> R) -> R
        pub fn response_in(&mut self, id: WidgetId, record: impl FnMut(&mut Ui)) -> Response
        pub fn prime(&mut self, frames: u32, record: impl FnMut(&mut Ui))
        pub fn advance(&mut self, dt: Duration) -> &mut Self
        pub const fn at(&mut self, time: Duration) -> &mut Self
        pub fn advance_frames(&mut self, frames: u32, dt: Duration, record: impl FnMut(&mut Ui))
        pub fn advance_past_double_click(&mut self) -> &mut Self
        pub fn resize(&mut self, size: UVec2) -> &mut Self
        pub fn set_system_scale(&mut self, scale: SystemScale) -> &mut Self
        pub fn set_refresh_millihertz(&mut self, millihertz: Option<NonZeroU32>) -> &mut Self
        pub fn set_system_color_scheme(&mut self, scheme: Option<ColorScheme>) -> &mut Self
        pub fn set_system_motion_reduced(&mut self, reduced: bool) -> &mut Self
        pub fn set_system_transparency_reduced(&mut self, reduced: bool) -> &mut Self
        pub fn set_system_power_reduced(&mut self, reduced: bool) -> &mut Self
        pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta
        pub fn hover(&mut self, at: impl Into<Locator>) -> InputDelta
        pub fn press(&mut self, at: impl Into<Locator>, button: PointerButton) -> InputDelta
        pub fn release(&mut self, button: PointerButton) -> InputDelta
        pub fn click(&mut self, at: impl Into<Locator>) -> InputDelta
        pub fn double_click(&mut self, at: impl Into<Locator>) -> InputDelta
        pub fn right_click(&mut self, at: impl Into<Locator>) -> InputDelta
        pub fn drag_to(&mut self, at: impl Into<Locator>) -> InputDelta
        pub fn scroll_pixels(&mut self, at: impl Into<Locator>, delta: Vec2) -> InputDelta
        pub fn scroll_lines(&mut self, at: impl Into<Locator>, delta: Vec2) -> InputDelta
        pub fn pinch(&mut self, at: impl Into<Locator>, factor: ZoomFactor) -> InputDelta
        pub fn key(&mut self, key: impl Into<Shortcut>) -> InputDelta
        pub fn key_down(&mut self, key: impl Into<Shortcut>) -> InputDelta
        pub fn key_up(&mut self, key: impl Into<Shortcut>) -> InputDelta
        pub fn hold_modifiers(&mut self, mods: Modifiers) -> InputDelta
        pub fn type_text(&mut self, text: &str) -> InputDelta
        pub fn insert_text(&mut self, text: &str) -> InputDelta
        pub const fn session<'h, A>(&'h mut self, app: &'h mut A) -> HarnessSession<'h, A>
        pub fn widgets(&self, filter: Option<&str>) -> Vec<InspectedWidget>
        pub fn rect(&self, id: WidgetId) -> Option<Rect>
        pub fn layout_rect(&self, id: WidgetId) -> Option<Rect>
        pub fn point_in(&self, id: WidgetId, local: Vec2) -> Vec2
        pub fn hit_at(&self, pos: Vec2) -> Option<WidgetId>
        pub fn state<S: 'static>(&self, id: WidgetId) -> Option<&S>
        pub fn window_commands(&self) -> &[WindowCommand]
        pub fn clipboard_text(&self) -> Option<String>
        pub fn set_clipboard_text(&mut self, text: &str) -> &mut Self
        pub fn wait_for_gpu(&self) -> GpuWait
        pub fn wait_for_wake(&self, timeout: Duration) -> bool
        pub fn has_pending_wake(&self) -> bool
        pub fn is_device_lost(&self) -> bool
        pub fn recover(&mut self, gpu: Gpu)
        pub fn capture(&mut self, app: &mut impl App, pixels: &mut Vec<u8>) -> Result<(), PlayError>
        pub const fn ui(&self) -> &Ui
        pub const fn ui_mut(&mut self) -> &mut Ui

HarnessBuilder
    struct HarnessBuilder: Debug
        pub const fn system_scale(self, scale: SystemScale) -> Self
        pub const fn refresh_millihertz(self, millihertz: NonZeroU32) -> Self
        pub const fn system_color_scheme(self, scheme: ColorScheme) -> Self
        pub const fn system_motion_reduced(self, reduced: bool) -> Self
        pub const fn system_transparency_reduced(self, reduced: bool) -> Self
        pub const fn system_power_reduced(self, reduced: bool) -> Self
        pub const fn collect_gpu_stats(self, collect_gpu_stats: bool) -> Self
        pub const fn pixel_snap(self, pixel_snap: bool) -> Self
        pub const fn fonts(self, scope: FontScope) -> Self
        pub fn shaper(self, shaper: TextShaper) -> Self
        pub fn depth(self, depth: HarnessDepth) -> Self
        pub const fn frame_dt(self, dt: Duration) -> Self
        pub const fn settle_frames(self, frames: NonZeroU32) -> Self
        pub const fn background_wait(self, wait: Duration) -> Self
        pub fn build(self) -> Harness

HarnessDepth
    enum HarnessDepth: Debug
        Record
        Compose
        Gpu { gpu: Gpu, format: TargetFormat }
        pub const fn gpu(gpu: Gpu) -> Self
        pub const fn gpu_float(gpu: Gpu) -> Self

HarnessSession
    struct HarnessSession<'h, A>
        impl<'h, A: Debug> Debug for HarnessSession<'h, A>
        impl<A: App> AgentWindow<A> for HarnessSession<'_, A>

HostHandle  [features: winit]
    struct HostHandle<T: 'static>: Clone, Debug
        pub fn is_open(&self, window: WindowToken) -> bool
        pub fn is_hidden(&self, window: WindowToken) -> bool
        pub fn run_on_main(&self, f: impl FnOnce(&mut T) + Send + 'static) -> Result<(), HostHandleError>
        pub fn quit(&self)
        pub fn remote(&self, window: WindowToken) -> Remote<T>

HostHandleError  [features: winit]
    struct HostHandleError: Clone, Copy, Debug, Display, Eq, Error, PartialEq

IconDefinition
    struct IconDefinition: Clone, Debug
        pub name: Cow<'static, str>
        pub view_box: Size
        pub svg: Span
        pub tintable: bool

IconHandle
    struct IconHandle: Clone, Copy, Debug, PartialEq
        pub const fn view_box(self) -> Size

IconPaint
    struct IconPaint: Clone, Copy, Debug
        pub const fn new(handle: IconHandle) -> Self
        pub const fn fit(self, fit: Fit) -> Self
        pub const fn tint(self, tint: RgbaF32) -> Self
        pub const fn desaturate(self, desaturate: bool) -> Self
        impl PaintedBy<IconPaint> for RectShape

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
        pub fn texel(&self, at: UVec2) -> SrgbaU8
        pub fn magnified(&self, factor: NonZeroU32) -> Self
        pub fn as_srgba8(&self) -> &[u8]
        pub fn texels_mut(&mut self) -> &mut [SrgbaU8]
        pub fn from_png(bytes: &[u8]) -> Result<Self>  [features: golden or png]
        pub fn write_png(&self, out: impl Write) -> Result<()>  [features: golden or png]
        pub fn fill_with(&mut self, texel: impl FnMut(UVec2) -> SrgbaU8)
        pub fn row_mut(&mut self, row: u32) -> &mut [SrgbaU8]
        pub fn repeat_row(&mut self, row: u32)
        pub fn fitted(&self, max_side: NonZeroU32) -> Self

ImageDataError
    enum ImageDataError: Clone, Copy, Debug, Display, Eq, Error, PartialEq
        ZeroSize { size: UVec2 }
        LengthOverflow { size: UVec2 }
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

ImageHandle
    struct ImageHandle: Clone, Debug
        pub fn size(&self) -> UVec2
        pub fn update(&self, image: &Image) -> Result<(), ImageSizeError>
        pub fn is_resident(&self) -> bool

ImagePaint
    struct ImagePaint: Clone, Debug
        pub fn new(handle: ImageHandle) -> Self
        pub const fn fit(self, fit: Fit) -> Self
        pub const fn tile(self, offset: Vec2, scale: Vec2) -> Self
        pub const fn min_filter(self, min_filter: ImageFilter) -> Self
        pub const fn mag_filter(self, mag_filter: ImageFilter) -> Self
        pub const fn downsample(self, downsample: ImageDownsample) -> Self
        pub const fn tint(self, tint: RgbaF32) -> Self
        impl PaintedBy<ImagePaint> for CircleShape
        impl PaintedBy<ImagePaint> for RectShape
        impl PaintedBy<ImagePaint> for TriangleShape

ImageSizeError
    struct ImageSizeError: Clone, Copy, Debug, Display, Eq, Error, PartialEq
        pub size: UVec2
        pub max_dimension: u32

ImePreedit
    struct ImePreedit<'a>: Clone, Copy, Debug, Eq, PartialEq
        pub text: &'a str
        pub cursor: Option<Span>

InnerResponse
    struct InnerResponse<R>
        pub response: Response
        pub inner: R
        impl<R: Clone> Clone for InnerResponse<R>
        impl<R: Copy> Copy for InnerResponse<R>
        impl<R: Debug> Debug for InnerResponse<R>

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
        ScrollLines { notches: Vec2, lines: Vec2, pages: Vec2 }
        Pinch(ZoomFactor)
        KeyDown { key: Key, repeat: bool, physical: Key, text: KeyText }
        KeyUp { key: Key }
        ModifiersChanged(Modifiers)
        ImePreedit(ImePreedit<'a>)
        ImeCommit(&'a str)
        SurfaceFocusLost
        pub const fn scroll_lines(notches: Vec2) -> Self

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
        pub const fn is_empty(self) -> bool
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
        pub const fn from_char(c: char) -> Option<Self>
        impl From<Key> for Shortcut

KeyClass
    enum KeyClass: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        Text
        Space
        Enter
        Edit
        Caret
        Page
        Focus
        Cycle
        Escape
        Accel

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
        pub const SPACE: Self
        pub const ENTER: Self
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

KeyPress
    struct KeyPress: Clone, Copy, Debug, Eq, PartialEq
        pub key: Key
        pub mods: Modifiers
        pub repeat: bool
        pub physical: Key
        pub text: KeyText
        pub fn class(self) -> KeyClass

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
        Pointer = 6

LayerScope
    struct LayerScope<'a>: Debug
        pub fn at(self, p: impl Into<Vec2>) -> Self
        pub const fn anchor(self, anchor: Anchor) -> Self
        pub fn max_size(self, s: impl Into<Size>) -> Self
        pub fn show<R>(self, body: impl FnOnce(&mut Ui) -> R) -> R

Lens
    struct Lens: Clone, Copy, Debug, PartialEq, Pod, Zeroable
        pub bezel: f32
        pub thickness: f32
        pub refractive_index: f32
        pub dispersion: f32
        pub highlight: f32
        pub const NONE: Self
        pub const WINDOW_GLASS: Self

LineCap
    enum LineCap: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Butt = 0
        Square = 1
        Round = 2

LineJoin
    enum LineJoin: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Miter = 0
        Bevel = 1
        Round = 2

LinearGeometry
    struct LinearGeometry: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub angle: f32

Locate
    trait Locate<A>: Clone + Debug + Send + 'static
        fn find_all(&self, app: &A, ui: &Ui, out: &mut Vec<Found>) -> Result<(), LocateError>

LocateError
    #[non_exhaustive]
    enum LocateError: Clone, Debug, Display, Error, PartialEq
        NotFound { locator: String, nearest: Vec<String> }
        Ambiguous { locator: String, matches: Vec<Found> }
        OffScreen { locator: String }
        Covered { locator: String, point: Vec2 }
        Disabled { locator: String }
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
            pub fn offset(self, by: Vec2) -> Self
            pub fn nth(self, n: usize) -> Self
            pub fn find<A>(&self, app: &A, ui: &Ui) -> Result<Found, LocateError> where X: Locate<A>
        impl<X: Clone> Clone for Locator<X>
        impl<X: Debug> Debug for Locator<X>
        impl<X: PartialEq> PartialEq for Locator<X>
        impl<X> From<&str> for Locator<X>
        impl<X> From<String> for Locator<X>
        impl<X> From<Vec2> for Locator<X>
        impl<X> From<WidgetId> for Locator<X>

Material
    struct Material: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub effect: MaterialEffect
        pub backdrop_blur: f32
        pub max_luminance: Option<f32>
        pub min_luminance: Option<f32>
        pub fallback: BackdropFallback
        pub const fn new<U: EffectParams>(effect: EffectName<U, BackdropEffect>, params: U) -> Self
        pub const fn blur(sigma: f32) -> Self
        pub const fn frosted(sigma: f32) -> Self
        pub const fn glass(lens: Lens) -> Self
        pub const fn with_backdrop_blur(self, sigma: f32) -> Self
        pub fn with_fallback(self, fallback: impl Into<Brush>) -> Self
        pub const fn with_max_luminance(self, luminance: f32) -> Self
        pub fn with_max_luminance_for(self, ink: RgbaF32, fill: RgbaF32) -> Self
        pub const fn with_min_luminance(self, luminance: f32) -> Self
        pub fn with_min_luminance_for(self, ink: RgbaF32, fill: RgbaF32) -> Self
        pub const fn at_rest(self) -> Self
        impl From<Material> for MaterialPaint
        impl PaintedBy<Material> for CircleShape
        impl PaintedBy<Material> for RectShape
        impl PaintedBy<Material> for TriangleShape

MaterialEffect
    struct MaterialEffect: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub const MAX_LANES: usize
        pub const fn new<U: EffectParams>(effect: EffectName<U, BackdropEffect>, params: U) -> Self
        pub fn name(&self) -> &str
        pub fn params(&self) -> &[f32]

MaterialPaint
    struct MaterialPaint: Clone, Debug, From<Material>, PartialEq
        pub paint: Paint
        pub material: Material
        impl PaintedBy<MaterialPaint> for CircleShape
        impl PaintedBy<MaterialPaint> for RectShape
        impl PaintedBy<MaterialPaint> for TriangleShape

Menu
    struct Menu<'a, O = TriggerOwned>: ConfigureDefaults (blanket)
        impl Menu<'_, TriggerOwned>
            pub fn for_id(trigger: WidgetId) -> Self
            pub fn context(trigger: &Response) -> Self
            pub fn below(trigger: &Response) -> Self
            pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> Option<OverlayResponse<R>>
            pub fn open(ui: &mut Ui, trigger: WidgetId)
            pub fn open_at(ui: &mut Ui, trigger: WidgetId, point: Vec2)
            pub fn menu_id(trigger: WidgetId) -> WidgetId
            pub fn close(ui: &mut Ui, trigger: WidgetId)
            pub fn is_open(ui: &Ui, trigger: WidgetId) -> bool
        impl Menu<'_, CallerOwned>
            pub fn at(point: Vec2) -> Self
            pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayResponse<R>
        impl<'a, O> Menu<'a, O>
            pub fn style(self, s: impl Into<Option<&'a MenuTheme>>) -> Self
            pub const fn landing(self, landing: MenuLanding) -> Self
            pub const fn chrome(self, chrome: Chrome) -> Self
        impl<'a, O: Debug> Debug for Menu<'a, O>
        impl<O> Configure for Menu<'_, O>

MenuBar
    struct MenuBar<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a MenuBarTheme>>) -> Self
        pub fn menu_style(self, s: impl Into<Option<&'a MenuTheme>>) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut MenuBarMenus<'_>) -> R) -> InnerResponse<R>
        pub fn open(ui: &mut Ui, bar: WidgetId, label: &str)
        pub fn close(ui: &mut Ui, bar: WidgetId)
        pub fn is_open(ui: &Ui, bar: WidgetId) -> bool
        pub fn title_id(bar: WidgetId, label: &str) -> WidgetId

MenuBarMenus
    struct MenuBarMenus<'u>: Debug
        pub fn menu<'l, R>(&mut self, label: impl Into<TextInput<'l>>, body: impl FnOnce(&mut Ui) -> R) -> Option<OverlayResponse<R>>

MenuBarTheme
    struct MenuBarTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub title: ButtonTheme
        pub bar: Chrome
        pub bar_padding: Spacing
        pub gap: f32
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

MenuItem
    struct MenuItem<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a MenuItemTheme>>) -> Self
        pub const fn selected(self, selected: bool) -> Self
        pub const fn shortcut(self, shortcut: Shortcut) -> Self
        pub const fn shortcut_hint(self, shortcut: Shortcut) -> Self
        pub fn show(self, ui: &mut Ui) -> Response

MenuItemTheme
    struct MenuItemTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize, ThemeSlot
        pub unselected: StatefulLook
        pub selected: StatefulLook
        pub shortcut: RgbaF32
        pub gap: f32
        pub defaults: SlotDefaults
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

MenuLanding
    enum MenuLanding: Clone, Copy, Debug, Default, Eq, PartialEq
        First
        Last
        Row(WidgetId)

MenuSeparator
    struct MenuSeparator<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a SeparatorTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response

MenuTheme
    struct MenuTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub panel: Chrome
        pub material: Option<Material>
        pub padding: Spacing
        pub min_width: f32
        pub gap: f32
        pub item: MenuItemTheme
        pub separator: SeparatorTheme
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

Mesh
    struct Mesh: Clone, Debug, Default
        pub const fn new() -> Self
        pub fn with_capacity(vertices: usize, idxs: usize) -> Self
        pub fn clear(&mut self)
        pub const fn is_noop(&self) -> bool
        pub fn content_hash(&self) -> ContentHash
        pub fn add_vertex(&mut self, pos: Vec2, color: impl Into<RgbaF32>) -> u32
        pub fn add_triangle(&mut self, a: u32, b: u32, c: u32)
        pub fn append(&mut self, other: &Self)
        pub const fn bbox(&self) -> Rect
        pub fn filled_polygon(points: &[Vec2], color: impl Into<RgbaF32>) -> Self

MeshShape
    struct MeshShape<'a>: Clone, Debug, PaintedBy<RgbaF32>
        pub const fn at(self, rect: Rect) -> Self
        pub const fn fit(self, fit: Fit) -> Self

Metrics
    struct Metrics: Clone, Debug, Default, Deserialize<'de>, PartialEq, Serialize
        pub text: TextSteps
        pub space: SpaceSteps
        pub radius: RadiusSteps
        pub stroke: StrokeSteps
        pub float_shadow: ShadowGeometry
        pub float_material: Option<FloatMaterial>
        pub transition: AnimationSpec
        pub const DEFAULT: Self

Modal
    struct Modal<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a ModalTheme>>) -> Self
        pub const fn scrim(self, scrim: Chrome) -> Self
        pub const fn click_outside(self, click_outside: ClickOutside) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayResponse<R>
        pub const fn chrome(self, chrome: Chrome) -> Self

ModalTheme
    struct ModalTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub panel: Chrome
        pub panel_material: Option<Material>
        pub scrim: Chrome
        pub scrim_material: Option<Material>
        pub padding: Spacing
        pub min_width: f32
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

Modifiers
    struct Modifiers: BitOr, Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
        pub const CTRL: Self
        pub const SHIFT: Self
        pub const ALT: Self
        pub const MAC_CTRL: Self
        pub const META: Self
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
        pub const fn has_command(self) -> bool

MotionRole
    enum MotionRole: Clone, Copy, Debug, Default, Eq, PartialEq
        Decorative
        Essential

Number
    trait Number: Scalar
        impl<T: Scalar> Number for T

OffscreenHost
    struct OffscreenHost: Debug
        pub const WINDOW: WindowToken
        pub const fn builder(gpu: Gpu) -> OffscreenHostBuilder
        pub const fn ui(&self) -> &Ui
        pub const fn ui_mut(&mut self) -> &mut Ui
        pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta
        pub fn advance(&mut self, dt: Duration) -> &mut Self
        pub fn at(&mut self, time: Duration) -> &mut Self
        pub fn set_system_color_scheme(&mut self, scheme: Option<ColorScheme>) -> &mut Self
        pub fn set_system_motion_reduced(&mut self, reduced: bool) -> &mut Self
        pub fn set_system_transparency_reduced(&mut self, reduced: bool) -> &mut Self
        pub fn set_system_power_reduced(&mut self, reduced: bool) -> &mut Self
        pub fn wait_for_gpu(&self) -> GpuWait
        pub fn wait_for_wake(&self, timeout: Duration) -> bool
        pub fn has_pending_wake(&self) -> bool
        pub fn window_commands(&self) -> &[WindowCommand]
        pub fn is_device_lost(&self) -> bool
        pub fn recover(&mut self, gpu: Gpu)
        pub const fn set_system_scale(&mut self, scale: SystemScale) -> &mut Self
        pub const fn set_refresh_millihertz(&mut self, millihertz: Option<NonZeroU32>) -> &mut Self
        pub fn frame(&mut self, target: RenderTarget<'_>, app: &mut impl App) -> FrameReport

OffscreenHostBuilder
    struct OffscreenHostBuilder: Debug
        pub const fn fonts(self, scope: FontScope) -> Self
        pub fn shaper(self, shaper: TextShaper) -> Self
        pub const fn collect_gpu_stats(self, collect_gpu_stats: bool) -> Self
        pub const fn time(self, time: OffscreenTime) -> Self
        pub const fn retained_target(self, retained_target: bool) -> Self
        pub const fn system_scale(self, scale: SystemScale) -> Self
        pub const fn refresh_millihertz(self, millihertz: NonZeroU32) -> Self
        pub const fn system_color_scheme(self, scheme: ColorScheme) -> Self
        pub const fn system_motion_reduced(self, reduced: bool) -> Self
        pub const fn system_transparency_reduced(self, reduced: bool) -> Self
        pub const fn system_power_reduced(self, reduced: bool) -> Self
        pub const fn pixel_snap(self, pixel_snap: bool) -> Self
        pub const fn system_clipboard(self, system_clipboard: bool) -> Self  [features: system-clipboard or winit]
        pub fn build(self) -> OffscreenHost

OffscreenTime
    enum OffscreenTime: Clone, Copy, Debug, Eq, PartialEq
        Realtime
        Manual

OverlayClose
    enum OverlayClose: Clone, Copy, Debug, Eq, PartialEq
        Dismissed
        Requested

OverlayResponse
    struct OverlayResponse<R>
        pub id: WidgetId
        pub opened: bool
        pub close: Option<OverlayClose>
        pub inner: R
        pub const fn closed(&self) -> bool
        impl<R: Clone> Clone for OverlayResponse<R>
        impl<R: Copy> Copy for OverlayResponse<R>
        impl<R: Debug> Debug for OverlayResponse<R>

PLATFORM
    const PLATFORM: Platform

Paint
    struct Paint: Animatable, Clone, Debug, Default, Deserialize<'de>, From<Border>, From<Brush>, From<Gradient>, From<RgbaF32>, From<Shadow>, From<SrgbaU8>, Lerp, PartialEq, Serialize
        pub fill: Brush
        pub border: Border
        pub shadow: Shadow
        pub const NONE: Self
        pub fn with_fill(self, brush: impl Into<Brush>) -> Self
        pub const fn with_border(self, border: Border) -> Self
        pub const fn with_shadow(self, shadow: Shadow) -> Self
        pub const fn with_material(self, material: Material) -> MaterialPaint
        pub const fn is_noop(&self) -> bool
        impl From<Paint> for Chrome

PaintAnimation
    struct PaintAnimation: Clone, Copy, Debug
        pub channels: PaintChannels
        pub timing: PaintTiming
        pub curve: PaintCurve
        pub motion: MotionRole
        pub const fn alpha(from: f32, to: f32) -> Self
        pub const fn turn(from: f32, to: f32) -> Self
        pub const fn clock() -> Self
        pub const fn with_alpha(self, from: f32, to: f32) -> Self
        pub const fn with_turn(self, from: f32, to: f32) -> Self
        pub const fn with_period(self, period: Duration) -> Self
        pub const fn with_started_at(self, at: Duration) -> Self
        pub const fn with_repeat(self, repeat: PaintRepeat) -> Self
        pub const fn with_steps(self, n: NonZeroU32) -> Self
        pub fn with_curve(self, curve: impl Into<PaintCurve>) -> Self
        pub const fn with_motion(self, motion: MotionRole) -> Self

PaintChannels
    struct PaintChannels: Clone, Copy, Debug, PartialEq
        pub alpha: Option<PaintRange>
        pub turn: Option<PaintRange>

PaintCurve
    struct PaintCurve: Clone, Copy, Debug, From<Easing>, Hash
        pub const LINEAR: Self
        pub const SINE: Self
        pub const SQUARE: Self
        pub const fn new(curve: fn(f32) -> f32) -> Self
        pub fn apply(self, phase: f32) -> f32

PaintRange
    struct PaintRange: Clone, Copy, Debug, PartialEq
        pub from: f32
        pub to: f32

PaintRepeat
    enum PaintRepeat: Clone, Copy, Debug, Eq, PartialEq
        Once
        Forever
        Settle(Duration)
        Held

PaintSteps
    enum PaintSteps: Clone, Copy, Debug, Eq, PartialEq
        Continuous
        Steps(NonZeroU32)

PaintTiming
    struct PaintTiming: Clone, Copy, Debug, Eq, PartialEq
        pub started_at: Duration
        pub period: Duration
        pub repeat: PaintRepeat
        pub steps: PaintSteps

PaintedBy
    trait PaintedBy<P>: DrawWith<P>

Palette
    struct Palette: Clone, Debug, Deserialize<'de>, PartialEq, Serialize
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
        pub on_accent: RgbaF32
        pub shadow: RgbaF32
        pub const DARK: Self
        pub const LIGHT: Self
        pub const fn for_scheme(scheme: ColorScheme) -> &'static Self
        pub const fn border_soft(&self) -> RgbaF32
        pub const fn border_mid(&self) -> RgbaF32
        pub const fn border_strong(&self) -> RgbaF32
        pub const fn float_shadow(&self, m: &Metrics) -> Shadow
        pub fn popup_panel(&self, m: &Metrics) -> Chrome

Panel
    struct Panel: Configure, ConfigureDefaults (blanket), Debug
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R>
        pub fn hstack() -> Self
        pub fn vstack() -> Self
        pub fn stack(axis: Axis) -> Self
        pub fn wrap_hstack() -> Self
        pub fn wrap_vstack() -> Self
        pub fn wrap_stack(axis: Axis) -> Self
        pub fn zstack() -> Self
        pub fn canvas() -> Self
        pub const fn chrome(self, chrome: Chrome) -> Self

ParamMismatch
    enum ParamMismatch: Clone, Debug, Display, Eq, PartialEq
        FieldCount { wgsl: usize, rust: usize }
        Name { idx: usize, wgsl: String, rust: &'static str }
        Offset { name: &'static str, wgsl: u32, rust: usize }
        Type { name: &'static str, rust: ParamType }
        Size { wgsl: u32, rust: usize }

ParamType
    enum ParamType: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        F32
        I32
        U32
        Vec2F32
        Vec3F32
        Vec4F32
        Vec2I32
        Vec3I32
        Vec4I32
        Vec2U32
        Vec3U32
        Vec4U32
        pub const fn wgsl(self) -> &'static str

ParamValue
    trait ParamValue
        const TYPE: ParamType

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

PlacedGlyph
    struct PlacedGlyph: Clone, Copy, Debug
        pub raster_key: GlyphRasterKey
        pub physical_pos: IVec2

Platform
    enum Platform: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        Mac
        Windows
        Linux

PlayError
    #[non_exhaustive]
    enum PlayError: Clone, Debug, Display, Error, From<LocateError>, PartialEq
        Locate(LocateError)
        Unsettled { frames_recorded: u32 }
        Untypable { found: char }
        ShortDrag { travel: f32 }
        EmptyRegion
        NoPixels
        NoDevice
        DeviceLost
        GpuTimedOut
        Disconnected
        Blocked
        Hidden
        NoFrame { waited: Duration }
        Busy { waited: Duration }
        StillBusy { waited: Duration }

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

PointerButtonPhase
    enum PointerButtonPhase: Clone, Copy, Debug, Default, Eq, PartialEq
        Idle
        Down { count: u8 }
        Held
        Up { click: Option<u8> }

PointerButtonState
    struct PointerButtonState: Clone, Copy, Debug, Default, PartialEq
        pub phase: PointerButtonPhase
        pub drag: Drag
        pub const IDLE: Self
        pub const fn held(self) -> bool
        pub const fn pressed(self) -> bool
        pub const fn clicked(self) -> bool
        pub const fn released(self) -> bool
        pub const fn press_count(self) -> u8
        pub const fn click_count(self) -> u8
        pub const fn double_clicked(self) -> bool

PointerEdge
    enum PointerEdge: Clone, Copy, Debug, Eq, PartialEq
        Pressed { count: u8 }
        Clicked { count: u8 }
        DragStarted
        DragStopped

PointerEvent
    enum PointerEvent: Clone, Copy, Debug, PartialEq
        Moved(Vec2)
        Pressed { pos: Vec2, button: PointerButton }
        Released { pos: Vec2, button: PointerButton }
        Scroll { pos: Vec2, pixels: Vec2, lines: Vec2, notches: Vec2, pages: Vec2 }
        Pinch { pos: Vec2, factor: ZoomFactor }
        Left

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

PolylineShape
    struct PolylineShape<'a>: Clone, Debug, PaintedBy<Stroke>
        pub const fn at(self, rect: Rect) -> Self
        pub const fn fit(self, fit: Fit) -> Self
        pub const fn point_colors(self, colors: &'a [RgbaF32]) -> Self
        pub const fn segment_colors(self, colors: &'a [RgbaF32]) -> Self

Popup
    struct Popup<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a PopupTheme>>) -> Self
        pub const fn layer(self, layer: Layer) -> Self
        pub fn click_outside(self, click_outside: impl Into<Option<ClickOutside>>) -> Self
        pub const fn anchor(self, anchor: Anchor) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayResponse<R>
        pub const fn chrome(self, chrome: Chrome) -> Self

PopupTheme
    struct PopupTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub panel: Chrome
        pub material: Option<Material>
        pub padding: Spacing
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

PopupTrigger
    struct PopupTrigger<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn on(trigger: &Response) -> Self
        pub fn style(self, s: impl Into<Option<&'a PopupTheme>>) -> Self
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> Option<OverlayResponse<R>>
        pub fn popup_id(trigger: WidgetId) -> WidgetId
        pub fn open(ui: &mut Ui, trigger: WidgetId)
        pub fn close(ui: &mut Ui, trigger: WidgetId)
        pub fn is_open(ui: &Ui, trigger: WidgetId) -> bool

PowerPreference
    enum PowerPreference: Clone, Copy, Debug, Default, DockTab (blanket), Eq, From<PowerPreference>, Hash, PartialEq
        LowPower
        HighPerformance

Preference
    enum Preference<T>: Default, DockTab (blanket)
        System
        Fixed(T)
        impl<'de, T> Deserialize<'de> where T: Deserialize<'de>
        impl<T: Clone> Clone for Preference<T>
        impl<T: Copy> Copy for Preference<T>
        impl<T: Debug> Debug for Preference<T>
        impl<T: Eq> Eq for Preference<T>
        impl<T: Hash> Hash for Preference<T>
        impl<T: PartialEq> PartialEq for Preference<T>
        impl<T> Serialize where T: Serialize

PressEffect
    struct PressEffect: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub effect: EffectName<PressParams>
        pub color: RgbaF32
        pub highlight: RgbaF32
        pub enter: Duration
        pub exit: Duration
        pub const MAX_PRESSES: usize

PressParams
    struct PressParams: Clone, Copy, Debug, EffectParams, PartialEq, Pod, Zeroable
        pub color: RgbaF32
        pub highlight: RgbaF32
        pub point: Vec2
        pub released_seconds: f32
        pub enter_seconds: f32
        pub exit_seconds: f32
        pub seed: f32

ProgressBar
    struct ProgressBar<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(fraction: f32) -> Self
        pub fn style(self, s: impl Into<Option<&'a ProgressBarTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response

ProgressBarTheme
    struct ProgressBarTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub track: RgbaF32
        pub fill: RgbaF32
        pub thickness: f32
        pub const fn from_parts(p: &Palette, _m: &Metrics) -> Self

QuadraticBezier
    struct QuadraticBezier: Clone, Copy, Debug, PartialEq
        pub p0: Vec2
        pub p1: Vec2
        pub p2: Vec2
        impl From<QuadraticBezier> for CubicBezier

RadialGeometry
    struct RadialGeometry: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub center: Vec2
        pub radius: Vec2

RadioGroup
    struct RadioGroup<'a, T>: Configure, ConfigureDefaults (blanket)
        pub fn new(selected: &'a mut T) -> Self
        pub const fn axis(self, axis: Axis) -> Self
        pub const fn look(self, look: RadioLook<'a>) -> Self
        impl<T: PartialEq> RadioGroup<'_, T>
            pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &mut RadioOptions<'_, T>)) -> ValueResponse
        impl<'a, T: Debug> Debug for RadioGroup<'a, T>

RadioLook
    enum RadioLook<'a>: Clone, Copy, Debug
        Pip(Option<&'a ToggleTheme>)
        Segment(Option<&'a SegmentedTheme>)

RadioOption
    struct RadioOption<'o, 'g, T>: Configure, ConfigureDefaults (blanket)
        impl<'o, T: PartialEq> RadioOption<'o, '_, T>
            pub fn label(self, label: impl Into<TextInput<'o>>) -> Self
            pub fn show(self, ui: &mut Ui) -> ValueResponse
        impl<'o, 'g, T: Debug> Debug for RadioOption<'o, 'g, T>

RadioOptions
    struct RadioOptions<'g, T>
        impl<'g, T: PartialEq> RadioOptions<'g, T>
            pub fn option(&mut self, value: T) -> RadioOption<'_, 'g, T>
        impl<'g, T: Debug> Debug for RadioOptions<'g, T>

RadiusSteps
    struct RadiusSteps: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub mark: f32
        pub control: f32
        pub card: f32
        pub float: f32

RasterImage
    struct RasterImage<'a>: Clone, Copy, Debug
        pub content: ContentType
        pub size: UVec2
        pub bearing: IVec2
        pub data: &'a [u8]

RecordPass
    enum RecordPass: Clone, Copy, Debug, Default, Eq, PartialEq
        Warmup
        First
        Second

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

RectShape
    struct RectShape: Clone, Copy, Debug, PaintedBy<IconPaint>, PaintedBy<ImagePaint>, PaintedBy<Material>, PaintedBy<MaterialPaint>
        pub const fn at(self, rect: Rect) -> Self
        pub fn corners(self, corners: impl Into<Corners>) -> Self
        impl<P: Into<Paint>> PaintedBy<P> for RectShape
        impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for RectShape

ReducedEffect
    enum ReducedEffect<U: EffectParams, K: EffectKind = ColorEffect>
        Same
        Other(EffectName<U, K>)
        Nothing
        impl<U: EffectParams, K: EffectKind> Clone for ReducedEffect<U, K>
        impl<U: EffectParams, K: EffectKind> Copy for ReducedEffect<U, K>
        impl<U: EffectParams, K: EffectKind> Debug for ReducedEffect<U, K>
        impl<U: EffectParams, K: EffectKind> Eq for ReducedEffect<U, K>
        impl<U: EffectParams, K: EffectKind> PartialEq for ReducedEffect<U, K>

Reduction
    enum Reduction: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Unreduced
        Reduced
        pub const fn is_reduced(self) -> bool

Reductions
    struct Reductions: Clone, Copy, Debug, Default, Eq, PartialEq
        pub transparency: Reduction
        pub motion: Reduction
        pub power: Reduction
        pub const NONE: Self

Remote  [features: winit]
    struct Remote<T: 'static>: AgentWindow<T>, Debug
        pub fn with_blocked(self, blocked: impl Fn() -> bool + Send + 'static) -> Self
        pub const fn with_wait(self, wait: Duration) -> Self

RenderTarget
    struct RenderTarget<'a>: Clone, Copy, Debug
        pub fn new(texture: &'a Texture) -> Self

RequestedGpu
    struct RequestedGpu: Debug
        pub adapter: Adapter
        pub gpu: Gpu
        pub fn headless(power_preference: PowerPreference) -> Result<Self, GpuRequestError>

Response
    struct Response: Clone, Copy, Debug, PartialEq
        pub id: WidgetId
        pub rect: Option<Rect>
        pub layout_rect: Option<Rect>
        pub transform: TranslateScale
        pub pointer_local: Option<Vec2>
        pub pointer_over: bool
        pub disabled: bool
        pub focused: bool
        pub left: PointerButtonState
        pub right: PointerButtonState
        pub middle: PointerButtonState
        pub scroll: ScrollDelta
        pub const fn idle(id: WidgetId) -> Self
        pub const fn hovered(&self) -> bool
        pub const fn clicked(&self) -> bool
        pub const fn double_clicked(&self) -> bool
        pub const fn any_clicked(&self) -> bool
        pub const fn button(&self, button: PointerButton) -> &PointerButtonState
        pub const fn is_pressed(&self) -> bool
        pub fn press_fraction(&self, band: f32) -> Option<Vec2>

RgbaF32
    struct RgbaF32: Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, Display, From<SrgbaU8>, FromStr, Hash, Lerp, ParamValue, PartialEq, Pod, Serialize, Zeroable
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
        pub const fn luminance(self) -> f32
        pub const fn contrast_ratio(self, other: Self) -> f32
        pub const fn with_alpha(self, a: f32) -> Self
        pub const fn from_srgba8(bytes: SrgbaU8) -> Self
        pub const fn hex(rgb: u32) -> Self
        pub const fn hexa(rgba: u32) -> Self
        pub const fn to_srgba8(self) -> SrgbaU8
        impl From<RgbaF32> for Brush
        impl From<RgbaF32> for Chrome
        impl From<RgbaF32> for Paint
        impl PaintedBy<RgbaF32> for MeshShape<'_>
        impl PaintedBy<RgbaF32> for TextShape

Screenshot
    struct Screenshot: Clone, Debug, PartialEq
        pub const fn image(&self) -> &Image
        pub const fn scale_factor(&self) -> f32
        pub const fn region(&self) -> Rect
        pub fn place(&self, rect: Rect, picture: UVec2) -> Option<Rect>

ScreenshotId
    struct ScreenshotId: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq

Scroll
    struct Scroll<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn vertical() -> Self
        pub fn horizontal() -> Self
        pub fn both() -> Self
        pub fn pan_by(self, delta: Vec2) -> Self
        pub const fn reveal(self, child: WidgetId) -> Self
        pub fn zoom_by(self, factor: ZoomFactor) -> Self
        pub fn style(self, s: impl Into<Option<&'a ScrollbarTheme>>) -> Self
        pub const fn bar_mode(self, mode: BarMode) -> Self
        pub const fn overlay_bars(self) -> Self
        pub const fn hidden_bars(self) -> Self
        pub fn content_margin(self, m: impl Into<Spacing>) -> Self
        pub fn zoomable(self) -> Self
        pub fn zoom_config(self, config: ZoomConfig) -> Self
        pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R>
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub fn offset(ui: &Ui, id: WidgetId) -> Vec2
        pub fn set_offset(ui: &mut Ui, id: WidgetId, offset: Vec2)
        pub fn zoom(ui: &Ui, id: WidgetId) -> f32
        pub fn set_zoom(ui: &mut Ui, id: WidgetId, zoom: f32)

ScrollDelta
    struct ScrollDelta: Clone, Copy, Debug, Default, PartialEq
        pub pixels: Vec2
        pub lines: Vec2
        pub notches: Vec2
        pub pages: Vec2
        pub zoom: ZoomFactor
        pub const NONE: Self
        pub const fn pan(self, line_height: f32, viewport: Size) -> Vec2

ScrollbarTheme
    struct ScrollbarTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub thickness: f32
        pub gap: f32
        pub min_thumb: f32
        pub track: RgbaF32
        pub thumb: RgbaF32
        pub thumb_hovered: RgbaF32
        pub thumb_active: RgbaF32
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

SegmentedTheme
    struct SegmentedTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub track: Chrome
        pub track_padding: Spacing
        pub segment: ButtonTheme
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

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
    struct Separator<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn horizontal() -> Self
        pub fn vertical() -> Self
        pub fn along(axis: Axis) -> Self
        pub fn style(self, s: impl Into<Option<&'a SeparatorTheme>>) -> Self
        pub const fn color(self, color: RgbaF32) -> Self
        pub fn show(self, ui: &mut Ui) -> Response

SeparatorTheme
    struct SeparatorTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub color: RgbaF32
        pub thickness: f32
        pub margin: Spacing
        pub const fn from_parts(p: &Palette, m: &Metrics) -> Self
        pub fn menu(p: &Palette, m: &Metrics) -> Self

Shadow
    struct Shadow: Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub color: RgbaF32
        pub offset: Vec2
        pub blur: f32
        pub spread: f32
        pub kind: ShadowKind
        pub const NONE: Self
        pub const fn drop(color: RgbaF32, offset: Vec2, blur: f32) -> Self
        pub const fn inset(color: RgbaF32, offset: Vec2, blur: f32) -> Self
        pub const fn with_spread(self, spread: f32) -> Self
        pub const fn is_noop(&self) -> bool
        impl From<Shadow> for Chrome
        impl From<Shadow> for Paint

ShadowGeometry
    struct ShadowGeometry: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub offset: Vec2
        pub blur: f32

ShadowKind
    enum ShadowKind: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        Drop
        Inset

Shape
    struct Shape: Clone, Copy, Debug
        pub const fn rect() -> RectShape
        pub const fn circle() -> CircleShape
        pub const fn arc(start_angle: f32, sweep: f32) -> ArcShape
        pub const fn triangle(a: Vec2, b: Vec2, c: Vec2) -> TriangleShape
        pub const fn line(a: Vec2, b: Vec2) -> CurveShape
        pub fn polyline(points: &[Vec2]) -> PolylineShape<'_>
        pub const fn cubic_bezier(bezier: CubicBezier) -> CurveShape
        pub const fn quadratic_bezier(bezier: QuadraticBezier) -> CurveShape
        pub const fn text(text: InternedStr, font: GlyphFont) -> TextShape
        pub const fn mesh(mesh: &Mesh) -> MeshShape<'_>

Shortcut
    struct Shortcut: Clone, Copy, Debug, Display, DockTab (blanket), Eq, From<Key>, FromStr, Hash, PartialEq
        pub mods: ShortcutMods
        pub key: Key
        pub const fn new(mods: ShortcutMods, key: Key) -> Self
        pub const fn key(key: Key) -> Self
        pub const fn ctrl(c: char) -> Self
        pub fn matches(self, press: KeyPress) -> bool

ShortcutMods
    struct ShortcutMods: BitOr, Clone, Copy, Debug, Default, DockTab (blanket), Eq, FromStr, Hash, PartialEq
        pub const CTRL: Self
        pub const SHIFT: Self
        pub const ALT: Self
        pub const META: Self
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
        pub const fn has_command(self) -> bool

Size
    struct Size: Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, Hash, Lerp, PartialEq, Pod, Serialize, Zeroable
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
        pub const FILL: Self
        pub const HUG: Self
        pub const FILL_W: Self
        pub const FILL_H: Self
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
        impl<T: Number> From<T> for Sizing

Slider
    struct Slider<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(value: impl Into<BoundNum<'a>>, range: RangeInclusive<f64>) -> Self
        pub const fn step(self, step: f64) -> Self
        pub const fn decimals(self, n: usize) -> Self
        pub fn style(self, s: impl Into<Option<&'a SliderTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse

SliderColors
    struct SliderColors: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub track: RgbaF32
        pub fill: RgbaF32
        pub knob: RgbaF32

SliderTheme
    struct SliderTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub enabled: SliderColors
        pub disabled: SliderColors
        pub knob_diameter: f32
        pub track_thickness: f32
        pub const fn from_parts(p: &Palette, _m: &Metrics) -> Self

SlotDefaults
    struct SlotDefaults: Clone, Copy, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub padding: Spacing
        pub margin: Spacing
        pub transition: AnimationSpec
        pub press: Option<PressEffect>

SpaceSteps
    struct SpaceSteps: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub xxs: f32
        pub xs: f32
        pub sm: f32
        pub md: f32
        pub lg: f32
        pub xl: f32

Spacing
    struct Spacing: Add, Animatable, Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, Lerp, PartialEq, Pod, Serialize, Zeroable
        pub const ZERO: Self
        pub fn to_array(self) -> [f32; 4]
        pub fn left(self) -> f32
        pub fn top(self) -> f32
        pub fn right(self) -> f32
        pub fn bottom(self) -> f32
        pub fn all(v: f32) -> Self
        pub fn xy(x: f32, y: f32) -> Self
        pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self
        pub fn sums(self) -> Size
        impl<L: Number, T: Number, R: Number, B: Number> From<(L, T, R, B)> for Spacing
        impl<T: Number> From<T> for Spacing
        impl<X: Number, Y: Number> From<(X, Y)> for Spacing

Span
    struct Span: Clone, Copy, Debug, Default, Eq, PartialEq, Pod, TryFrom<Range<u32>>, TryFrom<Range<usize>>, Zeroable
        pub start: u32
        pub len: u32
        pub const fn new(start: u32, len: u32) -> Self
        pub const fn range(self) -> Range<usize>

SpanRangeError
    enum SpanRangeError: Clone, Copy, Debug, Display, Eq, Error, PartialEq
        Backward { start: usize, end: usize }
        PastU32 { end: usize }

Spinner
    struct Spinner<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new() -> Self
        pub fn style(self, s: impl Into<Option<&'a SpinnerTheme>>) -> Self
        pub const fn color(self, color: RgbaF32) -> Self
        pub const fn stroke_width(self, px: f32) -> Self
        pub fn show(self, ui: &mut Ui) -> Response

SpinnerTheme
    struct SpinnerTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub color: RgbaF32
        pub diameter: f32
        pub sweep: f32
        pub period: Duration
        pub width_ratio: f32
        pub min_stroke_width: f32
        pub const fn from_parts(p: &Palette, m: &Metrics) -> Self

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
        pub const fn axis(self) -> Axis
        pub const fn half(self) -> SplitHalf

Splitter
    struct Splitter<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn row(ratio: &'a mut f32) -> Self
        pub fn col(ratio: &'a mut f32) -> Self
        pub fn along(axis: Axis, ratio: &'a mut f32) -> Self
        pub const fn min_pane(self, px: f32) -> Self
        pub const fn min_ratio(self, ratio: f32) -> Self
        pub fn style(self, s: impl Into<Option<&'a SplitterTheme>>) -> Self
        pub fn show(self, ui: &mut Ui, body: impl FnMut(&mut Ui, SplitHalf)) -> ValueResponse

SplitterTheme
    struct SplitterTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub grab_thickness: f32
        pub rule: RgbaF32
        pub rule_thickness: f32
        pub rule_hovered: RgbaF32
        pub rule_active: RgbaF32
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
        impl From<SrgbaU8> for Chrome
        impl From<SrgbaU8> for Paint
        impl From<SrgbaU8> for RgbaF32

StatefulLook
    struct StatefulLook: Clone, Debug, Default, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub normal: WidgetLook
        pub hovered: WidgetLook
        pub active: WidgetLook
        pub disabled: WidgetLook
        pub const fn pick(&self, response: &Response, engagement: Engagement) -> &WidgetLook
        pub const fn each_mut(&mut self) -> [&mut WidgetLook; 4]

StepError
    struct StepError: Clone, Debug, Display, Error, PartialEq
        pub step: usize
        pub source: PlayError

Stop
    struct Stop: Clone, Copy, Debug, Default, Deserialize<'de>, Eq, PartialEq, Serialize
        pub fn new(offset: f32, color: impl Into<RgbaF32>) -> Self
        pub const fn offset(self) -> f32
        pub const fn color(self) -> RgbaF32

Stroke
    struct Stroke: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub color: RgbaF32
        pub width: f32
        pub ramp: Option<ColorRamp>
        pub cap: LineCap
        pub join: LineJoin
        pub const fn new(color: RgbaF32, width: f32) -> Self
        pub const fn with_ramp(self, ramp: ColorRamp) -> Self
        pub const fn with_cap(self, cap: LineCap) -> Self
        pub const fn with_join(self, join: LineJoin) -> Self
        pub const fn is_noop(&self) -> bool
        impl PaintedBy<Stroke> for ArcShape
        impl PaintedBy<Stroke> for CircleShape
        impl PaintedBy<Stroke> for CurveShape
        impl PaintedBy<Stroke> for PolylineShape<'_>

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
        MissingUsages { missing: TextureUsages }

SvgParseError
    struct SvgParseError: Debug, Display, Error

SwatchRow
    enum SwatchRow<'a>: Clone, Copy, Debug, Default, PartialEq
        None
        History
        Fixed(&'a [RgbaF32])

Switch
    struct Switch<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(value: &'a mut bool) -> Self
        pub fn label(self, label: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a ToggleTheme>>) -> Self
        pub fn show(self, ui: &mut Ui) -> ValueResponse

SystemScale
    struct SystemScale: Clone, Copy, Debug, Default, PartialEq, PartialOrd
        pub const ONE: Self
        pub const MIN: f32
        pub const MAX: f32
        pub const fn new(factor: f32) -> Option<Self>
        pub const fn get(self) -> f32

TabAddress
    struct TabAddress: Clone, Copy, Debug, Eq, PartialEq
        pub group: TabGroupId
        pub idx: usize

TabChip
    struct TabChip: Clone, Copy, Debug
        pub idx: usize
        pub key: TabKey
        pub label: InternedStr
        pub selected: bool
        pub text: TextStyle
        pub id: WidgetId
        pub fn show_label(&self, ui: &mut Ui)

TabEnd
    enum TabEnd: Clone, Copy, Debug, Default, Eq, PartialEq
        First
        Last

TabGroup
    struct TabGroup<T>
        pub const fn id(&self) -> TabGroupId
        pub fn tabs(&self) -> &[T]
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
        pub fn pane_id(self, dock: WidgetId) -> WidgetId
        pub fn content_id(self, dock: WidgetId) -> WidgetId
        pub fn strip_id(self, dock: WidgetId) -> WidgetId

TabItem
    struct TabItem: Clone, Copy, Debug
        pub key: TabKey
        pub label: InternedStr
        pub closable: bool
        pub draggable: bool
        pub const fn new(key: TabKey, label: InternedStr) -> Self

TabKey
    struct TabKey: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        pub fn of(value: impl Hash) -> Self

TabMove
    struct TabMove: Clone, Copy, Debug, Eq, PartialEq
        pub from: usize
        pub to: usize
        pub const fn landing(self) -> usize
        pub fn apply<T>(self, items: &mut [T])

TabOverflow
    enum TabOverflow: Clone, Copy, Debug, Default, Eq, PartialEq
        Scroll
        Menu

TabStrip
    struct TabStrip<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn new(items: &'a [TabItem]) -> Self
        pub fn selected_idx(self, idx: impl Into<Option<usize>>) -> Self
        pub const fn focused(self, focused: bool) -> Self
        pub const fn overflow(self, overflow: TabOverflow) -> Self
        pub fn style(self, s: impl Into<Option<&'a TabsTheme>>) -> Self
        pub fn menu_style(self, s: impl Into<Option<&'a MenuTheme>>) -> Self
        pub fn chip_id(strip: WidgetId, key: TabKey) -> WidgetId
        pub fn close_id(strip: WidgetId, key: TabKey) -> WidgetId
        pub fn show(self, ui: &mut Ui, content: impl FnMut(&mut Ui, &TabChip)) -> TabStripResponse

TabStripResponse
    struct TabStripResponse: Clone, Copy, Debug
        pub response: Response
        pub activated: Option<usize>
        pub closed: Option<usize>
        pub reordered: Option<TabMove>
        pub dragging: Option<usize>

TabbedView
    struct TabbedView<'a, S, L, K = fn(usize, &S) -> TabKey>: ConfigureDefaults (blanket)
        impl<'a, S: AsRef<str>> TabbedView<'a, S, fn(&S) -> &str>
            pub fn new(selected: &'a mut usize, options: &'a [S]) -> Self
        impl<'a, S, L: Fn(&S) -> &str> TabbedView<'a, S, L>
            pub fn labeled(selected: &'a mut usize, options: &'a [S], label: L) -> Self
            pub fn keyed<H: Hash>(self, key: impl Fn(&S) -> H) -> TabbedView<'a, S, L, impl Fn(usize, &S) -> TabKey>
        impl<'a, S, L: Fn(&S) -> &str, K: Fn(usize, &S) -> TabKey> TabbedView<'a, S, L, K>
            pub const fn closable(self, closable: bool) -> Self
            pub const fn draggable(self, draggable: bool) -> Self
            pub const fn overflow(self, overflow: TabOverflow) -> Self
            pub fn style(self, s: impl Into<Option<&'a TabsTheme>>) -> Self
            pub fn menu_style(self, s: impl Into<Option<&'a MenuTheme>>) -> Self
            pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, usize) -> R) -> TabbedViewResponse<R>
        impl<'a, S: Debug, L: Debug, K: Debug> Debug for TabbedView<'a, S, L, K>
        impl<S, L, K> Configure for TabbedView<'_, S, L, K>

TabbedViewResponse
    struct TabbedViewResponse<R>
        pub response: Response
        pub inner: Option<R>
        pub changed: bool
        pub activated: Option<usize>
        pub closed: Option<usize>
        pub reordered: Option<TabMove>
        pub dragging: Option<usize>
        impl<R: Clone> Clone for TabbedViewResponse<R>
        impl<R: Copy> Copy for TabbedViewResponse<R>
        impl<R: Debug> Debug for TabbedViewResponse<R>

TabsTheme
    struct TabsTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize, ThemeSlot
        pub unselected: StatefulLook
        pub selected: StatefulLook
        pub cap_focused: RgbaF32
        pub cap_unfocused: RgbaF32
        pub cap_thickness: f32
        pub strip: Chrome
        pub strip_padding: Spacing
        pub gap: f32
        pub rule: RgbaF32
        pub rule_thickness: f32
        pub chip_padding: Spacing
        pub trailing_inset: f32
        pub min_width: f32
        pub max_width: f32
        pub close: StatefulLook
        pub close_size: f32
        pub close_glyph_span: f32
        pub close_glyph_width: f32
        pub close_shown: CloseShown
        pub drop_mark: RgbaF32
        pub drop_mark_width: f32
        pub label_gap: f32
        pub defaults: SlotDefaults
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

TargetFormat
    struct TargetFormat: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, PartialEq
        pub const RGBA8_SRGB: Self
        pub const RGBA16_FLOAT: Self
        pub fn new(format: TextureFormat) -> Self
        pub const fn get(self) -> TextureFormat

Text
    struct Text<'a>: Configure, ConfigureDefaults (blanket), Debug
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
        pub fn text_align(self, a: impl Into<Align>) -> Self
        pub fn show(self, ui: &mut Ui) -> Response

TextEdit
    struct TextEdit<'a>: Configure, ConfigureDefaults (blanket), Debug
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
        pub const fn select_all_on_focus(self, select_all_on_focus: bool) -> Self
        pub const fn escape_falls_through(self, escape_falls_through: bool) -> Self
        pub const fn max_chars(self, n: usize) -> Self
        pub fn text_align(self, a: impl Into<Align>) -> Self
        pub const fn multiline(self, multiline: bool) -> Self
        pub fn placeholder(self, text: impl Into<TextInput<'a>>) -> Self
        pub fn show(self, ui: &mut Ui) -> TextEditResponse

TextEditResponse
    struct TextEditResponse: Clone, Debug
        pub response: Response
        pub changed: bool
        pub committed: bool
        pub committed_text: Option<String>
        pub ended: Option<EditEnd>
        pub focus_gained: bool
        pub focus_lost: bool

TextEditTheme
    struct TextEditTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize, ThemeSlot
        pub looks: StatefulLook
        pub placeholder: RgbaF32
        pub caret: RgbaF32
        pub caret_width: f32
        pub selection: RgbaF32
        pub defaults: SlotDefaults
        pub fn corner_centering(&self, text: Size, at: Vec2) -> Vec2
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self
        pub fn flat(&self) -> Self

TextGlyphs
    struct TextGlyphs<'a>: Debug
        pub fn line(&mut self, line: GlyphLine<'_>, out: &mut Vec<PlacedGlyph>)
        pub fn measure(&mut self, text: &str, font: GlyphFont) -> Size
        pub fn rasterize(&mut self, glyph: GlyphRasterKey) -> Option<RasterImage<'_>>

TextInput
    enum TextInput<'a>: Debug, Default, From<Cow<'a, str>>, From<InternedStr>, From<String>
        Borrowed(&'a str)
        Owned(String)
        Interned(InternedStr)
        impl<'a, T: AsRef<str> + ?Sized> From<&'a T> for TextInput<'a>

TextProbe
    struct TextProbe<'a>: Debug
        pub const fn size(&self) -> Size
        pub fn caret_at(&self, byte_offset: usize) -> Caret
        pub fn byte_at(&self, pos: Vec2) -> usize
        pub fn selection_rects(&self, range: Range<usize>, out: &mut Vec<Rect>)

TextRun
    struct TextRun<'a>: Clone, Copy, Debug
        pub text: &'a str
        pub font: GlyphFont
        pub wrap: TextWrap
        pub align: Align
        pub max_width: Option<f32>

TextShape
    struct TextShape: Clone, Debug, PaintedBy<RgbaF32>
        pub const fn at_origin(self, origin: Vec2) -> Self
        pub const fn wrap(self, wrap: TextWrap) -> Self
        pub fn align(self, a: impl Into<Align>) -> Self

TextShaper
    struct TextShaper: Clone, Debug, Default
        pub fn new() -> Self
        pub fn with_fonts(scope: FontScope) -> Self
        pub fn load_font(&self, source: impl Into<FontSource>) -> Result<FontFamily, FontLoadError>
        pub fn has_font(&self, family: FontFamily) -> bool
        pub fn font_families(&self) -> Vec<FontFamily>
        pub fn glyphs(&self) -> TextGlyphs<'_>

TextSteps
    struct TextSteps: Clone, Copy, Debug, Deserialize<'de>, PartialEq, Serialize
        pub caption: f32
        pub body: f32
        pub title: f32

TextStyle
    struct TextStyle: Animatable, Clone, Copy, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub font_size: f32
        pub color: RgbaF32
        pub line_height_factor: f32
        pub family: FontFamily
        pub weight: FontWeight
        pub slant: FontSlant
        pub const fn from_parts(p: &Palette, m: &Metrics) -> Self
        pub const fn stock(scheme: ColorScheme) -> Self
        pub fn font(&self) -> GlyphFont
        pub fn line_height(&self) -> f32
        pub fn block_height(&self, line_count: usize) -> f32
        pub const fn with_font_size(self, px: f32) -> Self
        pub const fn with_color(self, color: RgbaF32) -> Self
        pub const fn with_line_height_factor(self, factor: f32) -> Self
        pub const fn with_family(self, family: FontFamily) -> Self
        pub const fn with_weight(self, weight: FontWeight) -> Self
        pub const fn with_slant(self, slant: FontSlant) -> Self
        pub const fn bold(self) -> Self
        pub const fn italic(self) -> Self

TextStyleOverrides
    struct TextStyleOverrides: Clone, Copy, Debug, Default, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub color: Option<RgbaF32>
        pub font_size: Option<f32>
        pub line_height_factor: Option<f32>
        pub family: Option<FontFamily>
        pub weight: Option<FontWeight>
        pub slant: Option<FontSlant>
        pub const NONE: Self
        pub fn apply(self, base: &TextStyle) -> TextStyle
        pub const fn with_font_size(self, px: f32) -> Self
        pub const fn with_color(self, color: RgbaF32) -> Self
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
    struct Theme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub button: ButtonTheme
        pub checkbox: ToggleTheme
        pub radio: ToggleTheme
        pub segmented: SegmentedTheme
        pub switch: ToggleTheme
        pub scrollbar: ScrollbarTheme
        pub text_edit: TextEditTheme
        pub drag_value: DragValueTheme
        pub menu: MenuTheme
        pub menu_bar: MenuBarTheme
        pub combo_box: ComboBoxTheme
        pub modal: ModalTheme
        pub popup: PopupTheme
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
        pub window_bg: RgbaF32
        pub material_fallback: RgbaF32
        pub transition: AnimationSpec
        pub panel_chrome: Option<Chrome>
        pub panel_clip: ClipMode
        pub fn scale_text(&mut self, factor: f32)
        pub fn dress_floats(&mut self, material: &FloatMaterial)
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self
        pub fn stock(scheme: ColorScheme) -> Self

ThemeEffectError
    enum ThemeEffectError: Clone, Copy, Debug, Display, Error, PartialEq
        UnloadedPress(EffectName<PressParams>)
        NotPress(EffectName<PressParams>)
        UnloadedMaterial(MaterialEffect)
        NotMaterial(MaterialEffect)
        MaterialParams { effect: MaterialEffect, want: u8, got: u8 }

ToggleTheme
    struct ToggleTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize, ThemeSlot
        pub unchecked: StatefulLook
        pub checked: StatefulLook
        pub indicator: RgbaF32
        pub indicator_unchecked: RgbaF32
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
    struct Tooltip<'a>: Configure, ConfigureDefaults (blanket), Debug
        pub fn on(trigger: &Response, text: impl Into<TextInput<'a>>) -> Self
        pub fn style(self, s: impl Into<Option<&'a TooltipTheme>>) -> Self
        pub const fn delay(self, delay: Duration) -> Self
        pub const fn when_disabled(self, when_disabled: bool) -> Self
        pub fn show(self, ui: &mut Ui) -> TooltipResponse
        pub const fn chrome(self, chrome: Chrome) -> Self
        pub fn bubble_id(trigger: WidgetId) -> WidgetId

TooltipResponse
    struct TooltipResponse: Clone, Copy, Debug
        pub visible: bool

TooltipTheme
    struct TooltipTheme: Clone, Debug, Deserialize<'de>, Lerp, PartialEq, Serialize
        pub panel: Chrome
        pub material: Option<Material>
        pub text: TextStyleOverrides
        pub padding: Spacing
        pub max_size: Size
        pub delay: Duration
        pub warmup: Duration
        pub gap: f32
        pub fn from_parts(p: &Palette, m: &Metrics) -> Self

Track
    struct Track: Clone, Copy, Debug, Hash, PartialEq
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
        pub const fn apply_vector(self, v: Vec2) -> Vec2
        pub const fn inverse_vector(self, v: Vec2) -> Vec2
        pub const fn apply_rect(self, r: Rect) -> Rect

TriangleShape
    struct TriangleShape: Clone, Debug, PaintedBy<ImagePaint>, PaintedBy<Material>, PaintedBy<MaterialPaint>
        pub const fn at(self, rect: Rect) -> Self
        pub const fn fit(self, fit: Fit) -> Self
        pub const fn radius(self, radius: f32) -> Self
        impl<P: Into<Paint>> PaintedBy<P> for TriangleShape
        impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for TriangleShape

Ui
    struct Ui: Debug
        pub const fn record_pass(&self) -> RecordPass
        pub fn waker(&self) -> Waker
        pub const fn theme(&self) -> &Rc<Theme>
        pub const fn scheme_theme(&self, scheme: ColorScheme) -> &Rc<Theme>
        pub fn set_scheme_theme(&mut self, scheme: ColorScheme, theme: impl Into<Rc<Theme>>) -> Result<(), ThemeEffectError>
        pub fn preferences(&self) -> UiPreferences
        pub fn set_preferences(&mut self, preferences: UiPreferences)
        pub const fn color_scheme(&self) -> ColorScheme
        pub fn system_color_scheme(&self) -> Option<ColorScheme>
        pub const fn reductions(&self) -> Reductions
        pub fn system_reductions(&self) -> Reductions
        pub const fn is_frame_rate_reduced(&self) -> bool
        pub fn pointer_events(&mut self, wake: PointerWake) -> &[PointerEvent]
        pub fn keyboard_events(&mut self, wake: KeyboardWake) -> &[KeyPress]
        pub fn key_pressed(&mut self, key: Key) -> bool
        pub fn key_held(&mut self, key: Key) -> bool
        pub fn key_released(&mut self, key: Key) -> bool
        pub fn shortcut_pressed(&mut self, shortcut: impl Into<Shortcut>) -> bool
        pub fn request_relayout(&mut self)
        pub const fn now(&self) -> Duration
        pub const fn request_cursor(&mut self, cursor: CursorIcon)
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
        pub fn gpu_pass_stats(&self) -> &GpuPassStats
        pub fn is_window_open(&self, token: WindowToken) -> bool
        pub fn add_shape<S: PaintedBy<P>, P>(&mut self, shape: S, paint: P) -> Option<AddedShape<'_>>
        pub fn load_icons(&self, table: IconTable) -> IconSet
        pub fn load_image(&self, image: &Image) -> Result<ImageHandle, ImageSizeError>
        pub fn load_font(&self, source: impl Into<FontSource>) -> Result<FontFamily, FontLoadError>
        pub fn load_effect<U: EffectParams, K: EffectKind>(&self, name: EffectName<U, K>, source: EffectSource<U, K>) -> Result<(), EffectError>
        pub fn has_effect<U: EffectParams, K: EffectKind>(&self, name: EffectName<U, K>) -> bool
        pub fn has_font(&self, family: FontFamily) -> bool
        pub fn font_families(&self) -> Vec<FontFamily>
        pub fn max_image_dimension(&self) -> Option<NonZeroU32>
        pub fn clipboard(&self) -> Clipboard
        pub fn fmt(&mut self, args: Arguments<'_>) -> InternedStr
        pub fn intern<'a>(&mut self, text: impl Into<TextInput<'a>>) -> InternedStr
        pub fn text(&self, text: InternedStr) -> &str
        pub fn layer(&mut self, layer: Layer) -> LayerScope<'_>
        pub fn close_overlay(&mut self)
        pub fn withdraw_input_scope(&mut self, id: WidgetId)
        pub fn response_for(&self, id: WidgetId) -> Response
        pub fn state<S: 'static>(&self, id: WidgetId) -> Option<&S>
        pub fn with_state<S: Default + 'static, R>(&mut self, id: WidgetId, body: impl FnOnce(&mut Self, &mut S) -> R) -> R
        pub fn with_state_or_insert_with<S: 'static, R>(&mut self, id: WidgetId, init: impl FnOnce(&mut Self) -> S, body: impl FnOnce(&mut Self, &mut S) -> R) -> R
        pub fn set_state<S: 'static>(&mut self, id: WidgetId, value: S)
        pub fn remove_state<S: 'static>(&mut self, id: WidgetId) -> Option<S>
        pub fn singleton<S: 'static>(&self) -> Option<&S>
        pub fn with_singleton<S: Default + 'static, R>(&mut self, body: impl FnOnce(&mut Self, &mut S) -> R) -> R
        pub fn set_singleton<S: 'static>(&mut self, value: S)
        pub fn remove_singleton<S: 'static>(&mut self) -> Option<S>
        pub fn animate<V: Animatable>(&mut self, request: AnimationRequest<V>, spec: AnimationSpec) -> Animated<V>
        pub const fn focus(&self) -> Option<WidgetId>
        pub fn is_focus_within(&self, ancestor: WidgetId) -> bool
        pub fn is_hover_within(&self, ancestor: WidgetId) -> bool
        pub const fn display(&self) -> Display
        pub fn user_scale(&self) -> UserScale
        pub fn set_user_scale(&mut self, scale: UserScale)
        pub const fn record_frame_idx(&self) -> u64
        pub fn inspect(&self, out: &mut Vec<InspectedWidget>)
        pub fn double_click_interval(&self) -> Duration
        pub const fn render_frame_idx(&self) -> u64
        pub fn probe_text<'a>(&'a mut self, run: TextRun<'a>) -> TextProbe<'a>
        pub fn pointer_actions(&self) -> impl Iterator<Item = PointerAction> + '_
        pub const fn set_focus(&mut self, id: WidgetId)
        pub const fn clear_focus(&mut self)
        pub const fn request_ime_area(&mut self, area: Rect)
        pub const fn ime_area(&self) -> Option<Rect>
        pub fn ime_preedit(&self) -> Option<ImePreedit<'_>>
        pub const fn is_focus_visible(&self) -> bool
        pub const fn focus_first_within(&mut self, ancestor: WidgetId)
        pub const fn set_focus_within(&mut self, ancestor: WidgetId, end: TabEnd)
        pub const fn pointer_pos(&mut self) -> Option<Vec2>
        pub const fn modifiers(&mut self) -> Modifiers
        pub const fn peek_pointer_pos(&self) -> Option<Vec2>
        pub const fn peek_modifiers(&self) -> Modifiers
        pub const fn focus_policy(&self) -> FocusPolicy
        pub const fn set_focus_policy(&mut self, p: FocusPolicy)
        pub const fn input_policy(&self) -> InputPolicy
        pub const fn set_input_policy(&mut self, p: InputPolicy)

UiPreferences
    struct UiPreferences: Clone, Copy, Debug, Default, Deserialize<'de>, DockTab (blanket), Eq, Hash, PartialEq, Serialize
        pub scheme: Preference<ColorScheme>
        pub motion: Preference<Reduction>
        pub transparency: Preference<Reduction>
        pub power: Preference<Reduction>
        pub frame_rate: Preference<Reduction>

UserScale
    struct UserScale: Clone, Copy, Debug, Default, PartialEq, PartialOrd
        pub const ONE: Self
        pub const LADDER: [f32; 13]
        pub const MIN: f32
        pub const MAX: f32
        pub const fn new(factor: f32) -> Option<Self>
        pub const fn get(self) -> f32
        pub fn stepped_up(self) -> Self
        pub fn stepped_down(self) -> Self
        pub const fn percent(self) -> u32

VAlign
    enum VAlign: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, PartialEq
        Auto = 0
        Top = 1
        Center = 2
        Bottom = 3
        Stretch = 4
        impl From<VAlign> for Align

ValueResponse
    struct ValueResponse: Clone, Copy, Debug
        pub response: Response
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

Waker
    struct Waker: Clone, Debug
        pub fn wake(&self)

WgslError
    struct WgslError: Debug, Display, Error
        pub fn diagnostic(&self) -> String
        pub fn span(&self) -> Option<Range<usize>>

WidgetId
    struct WidgetId: Clone, Copy, Debug, DockTab (blanket), Eq, Hash, NoUninit, PartialEq
        pub fn from_hash(h: impl Hash) -> Self
        pub fn with(self, h: impl Hash) -> Self
        pub fn auto() -> Self
        impl<X> From<WidgetId> for Locator<X>

WidgetLook
    struct WidgetLook: Clone, Debug, Default, Deserialize<'de>, From<Chrome>, Lerp, PartialEq, Serialize
        pub chrome: Chrome
        pub text: TextStyleOverrides
        pub const fn new(chrome: Chrome) -> Self
        pub const fn with_text(self, text: TextStyleOverrides) -> Self
        pub fn to_animated(&self, ambient_text: TextStyle) -> AnimatedLook

WindowCommand
    enum WindowCommand: Debug
        Open { token: WindowToken, config: WindowConfig }
        Close(WindowToken)

WindowConfig
    struct WindowConfig: Clone, Debug, Default
        pub title: String
        pub inner_size: Option<Size>
        pub min_inner_size: Option<Size>
        pub placement: WindowPlacement
        pub icon: Option<Image>
        pub app_id: Option<String>
        pub fn new(title: impl Into<String>) -> Self
        pub const fn with_inner_size(self, size: Size) -> Self
        pub const fn with_min_inner_size(self, size: Size) -> Self
        pub const fn with_physical_pos(self, pos: IVec2) -> Self
        pub const fn with_geometry(self, geometry: WindowGeometry) -> Self
        pub const fn with_maximized(self, maximized: bool) -> Self
        pub fn with_icon(self, icon: Image) -> Self
        pub fn with_app_id(self, app_id: impl Into<String>) -> Self

WindowGeometry
    struct WindowGeometry: Clone, Copy, Debug, PartialEq
        pub inner_size: Size
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
    struct WinitHostBuilder<T>: Debug
        impl<T> WinitHostBuilder<T> where T: App + 'static
            pub fn window(self, window: WindowConfig) -> Self
            pub fn title(self, title: impl Into<String>) -> Self
            pub const fn fonts(self, scope: FontScope) -> Self
            pub fn shaper(self, shaper: TextShaper) -> Self
            pub const fn vsync(self, vsync: Vsync) -> Self
            pub const fn power_preference(self, preference: PowerPreference) -> Self
            pub const fn collect_gpu_stats(self, collect_gpu_stats: bool) -> Self
            pub const fn pixel_snap(self, pixel_snap: bool) -> Self
            pub const fn system_clipboard(self, system_clipboard: bool) -> Self
            pub fn build(self, create_app: impl FnOnce(&mut Ui, HostHandle<T>) -> T + 'static) -> Result<WinitHost<T>, WinitHostError>

WinitHostError  [features: winit]
    #[non_exhaustive]
    enum WinitHostError: Debug, Display, Error
        CreateEventLoop { source: EventLoopError }
        RunEventLoop { source: EventLoopError }
        CreateWindow { token: WindowToken, source: OsError }
        Gpu { source: GpuRequestError }
        DeviceLost { source: GpuRequestError }
        DeviceKeptLost
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

bytemuck
    pub use bytemuck as bytemuck

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
    trait Animatable: Lerp + Clone + PartialEq + 'static
        fn sub(&self, other: &Self) -> Self
        fn add(&self, other: &Self) -> Self
        fn scale(&self, k: f32) -> Self
        fn magnitude_squared(&self) -> f32
        fn settle_distance_squared(&self) -> f32 { .. }
        fn zero() -> Self
        fn to_motion(&self) -> Self { .. }
        fn from_motion(motion: Self) -> Self { .. }
        fn normalize_for_spring(&mut self, _target: &Self, _velocity: &mut Self) { .. }

widget::AnimatedLook
    struct AnimatedLook: Animatable, Clone, Debug, Lerp, PartialEq
        pub chrome: Chrome
        pub text: TextStyle

widget::Applying
    struct Applying<'a, T>
        impl<'a, T: Debug> Debug for Applying<'a, T>

widget::Authored
    struct Authored<'a>: Clone, Copy, Debug
        pub const fn size(&self) -> Option<SizeSpec>
        pub const fn min_size(&self) -> Option<Size>
        pub const fn max_size(&self) -> Option<Size>
        pub const fn padding(&self) -> Option<Spacing>
        pub const fn margin(&self) -> Option<Spacing>
        pub const fn transform(&self) -> TranslateScale
        pub const fn pos(&self) -> Vec2
        pub const fn grid_cell(&self) -> GridCell
        pub const fn gap(&self) -> Option<f32>
        pub const fn line_gap(&self) -> Option<f32>
        pub const fn justify(&self) -> Option<Justify>
        pub const fn align(&self) -> Align
        pub const fn child_align(&self) -> Align
        pub const fn sense(&self) -> Sense
        pub const fn disabled(&self) -> bool
        pub const fn focusable(&self) -> bool
        pub const fn tab_stop(&self) -> bool
        pub const fn arrow_focus(&self) -> Option<Axis>
        pub const fn tab_idx(&self) -> i16
        pub const fn input_scope(&self) -> KeyFilter
        pub const fn visibility(&self) -> Visibility
        pub const fn clip(&self) -> Option<ClipMode>

widget::Barrier
    enum Barrier: Clone, Copy, Debug
        None
        Root(ClickOutside)
        Surface(WidgetId, ClickOutside)

widget::CallerOwned
    struct CallerOwned: Clone, Copy, Debug

widget::CheckState
    enum CheckState: Clone, Copy, Debug, Default, DockTab (blanket), Eq, From<bool>, Hash, PartialEq
        Unchecked
        Checked

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
        pub fn at(&mut self, p: impl Into<Vec2>) -> &mut Self
        pub fn grid_cell(&mut self, cell: impl Into<GridCell>) -> &mut Self
        pub fn adopt_placement(&mut self, from: &Widget) -> &mut Self
        pub fn gap(&mut self, g: f32) -> &mut Self
        pub fn line_gap(&mut self, g: f32) -> &mut Self
        pub const fn justify(&mut self, j: Justify) -> &mut Self
        pub fn align(&mut self, a: impl Into<Align>) -> &mut Self
        pub fn child_align(&mut self, a: impl Into<Align>) -> &mut Self
        pub const fn sense(&mut self, s: Sense) -> &mut Self
        pub fn add_sense(&mut self, s: Sense) -> &mut Self
        pub const fn disabled(&mut self, disabled: bool) -> &mut Self
        pub const fn focusable(&mut self, focusable: bool) -> &mut Self
        pub const fn tab_stop(&mut self, tab_stop: bool) -> &mut Self
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
        pub fn default_align(&mut self, a: impl Into<Align>) -> &mut Self
        pub fn default_child_align(&mut self, a: impl Into<Align>) -> &mut Self
        pub const fn default_justify(&mut self, j: Justify) -> &mut Self
        pub fn default_gap(&mut self, g: f32) -> &mut Self
        pub fn default_line_gap(&mut self, g: f32) -> &mut Self
        pub fn default_min_size(&mut self, s: impl Into<Size>) -> &mut Self
        pub fn default_max_size(&mut self, s: impl Into<Size>) -> &mut Self
        pub fn default_clip(&mut self, mode: ClipMode) -> &mut Self

widget::ConfigureDefaults
    trait ConfigureDefaults: Configure
        fn default_id(self, id: WidgetId) -> Self { .. }
        fn default_size(self, s: impl Into<SizeSpec>) -> Self { .. }
        fn default_padding(self, p: impl Into<Spacing>) -> Self { .. }
        fn default_margin(self, m: impl Into<Spacing>) -> Self { .. }
        fn default_align(self, a: impl Into<Align>) -> Self { .. }
        fn default_child_align(self, a: impl Into<Align>) -> Self { .. }
        fn default_justify(self, j: Justify) -> Self { .. }
        fn default_gap(self, g: f32) -> Self { .. }
        fn default_line_gap(self, g: f32) -> Self { .. }
        fn default_min_size(self, s: impl Into<Size>) -> Self { .. }
        fn default_max_size(self, s: impl Into<Size>) -> Self { .. }
        fn default_clip(self, mode: ClipMode) -> Self { .. }
        impl<T: Configure> ConfigureDefaults for T

widget::DOUBLE_CLICK_INTERVAL
    const DOUBLE_CLICK_INTERVAL: Duration

widget::DOUBLE_CLICK_RADIUS
    const DOUBLE_CLICK_RADIUS: f32

widget::Emitting
    struct Emitting<'a, T>
        impl<'a, T: Debug> Debug for Emitting<'a, T>

widget::Lerp
    trait Lerp: Sized
        fn lerp(a: &Self, b: &Self, t: f32) -> Self
        fn lerp_into(a: &Self, b: &Self, t: f32, out: &mut Self) { .. }

widget::LookPlan
    struct LookPlan: Debug

widget::OverlayScope
    struct OverlayScope: Debug
        pub fn claim(ui: &mut Ui, spec: OverlaySpec, root: &mut Widget) -> Self
        pub fn record<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayResponse<R>

widget::OverlaySpec
    struct OverlaySpec: Clone, Copy, Debug
        pub layer: Layer
        pub anchor: Option<Anchor>
        pub barrier: Barrier

widget::Selection
    enum Selection: Clone, Copy, Debug, Default, DockTab (blanket), Eq, From<bool>, Hash, PartialEq
        Unselected
        Selected

widget::ThemeSlot
    trait ThemeSlot
        type Pick: Copy + Hash
        fn look(&self, response: &Response, pick: Self::Pick) -> &WidgetLook
        fn defaults(&self) -> SlotDefaults
        fn plan(&self, response: &Response, pick: Self::Pick, text: TextStyle) -> LookPlan { .. }

widget::TriggerOwned
    struct TriggerOwned: Clone, Copy, Debug

widget::Widget
    struct Widget: Configure, ConfigureDefaults (blanket), Debug
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
        pub fn response(&mut self, ui: &mut Ui) -> Response
        pub fn activation_response(&mut self, ui: &mut Ui, activation: Activation) -> Response
        pub fn wear(&mut self, ui: &mut Ui, plan: LookPlan) -> AnimatedLook
        pub fn shortcut_pressed(&mut self, ui: &mut Ui, shortcut: impl Into<Shortcut>) -> bool
        pub fn key_pressed(&mut self, ui: &mut Ui, key: Key) -> bool
        pub fn record<R>(self, ui: &mut Ui, chrome: Option<&Chrome>, body: impl FnOnce(&mut Ui) -> R) -> R
        pub fn show<R>(self, ui: &mut Ui, chrome: Option<&Chrome>, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R>
        pub const fn authored(&self) -> Authored<'_>
        pub fn grid_tracks(&mut self, ui: &mut Ui, rows: &[Track], cols: &[Track])

widget::domain
    mod

widget::domain::EPS
    const EPS: f32

widget::domain::band_fraction
    const fn band_fraction(pos: f32, extent: f32, band: f32) -> f32

widget::domain::is_angle
    const fn is_angle(v: f32) -> bool

widget::domain::is_color
    const fn is_color(c: RgbaF32) -> bool

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

widget::domain::is_refractive_index
    const fn is_refractive_index(n: f32) -> bool

```

## Public but not exported

Items declared `pub` that no path from the crate root reaches. A caller cannot name
them; each one is either dead surface or a type that leaks through a signature.

```text
(none)
```
