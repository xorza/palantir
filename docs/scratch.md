My plan, which I think is the cleanest:

1. Rename current pub struct Ui → pub struct UiCore. All its fields/methods stay as-is. Every &mut Ui in library widgets becomes &mut UiCore (~212 sites, mechanical via sd).
2. Introduce pub struct Ui<T = ()> { core: UiCore, \_t: PhantomData<T> } with Deref/DerefMut to UiCore, owning .app() -> &mut T (no turbofish).
3. Host<T = ()> owns Ui<T>. host.frame(stamp, scale, &mut state, |ui| { ... }) — closure gets &mut Ui<T>; widgets called inside deref-coerce to &mut UiCore.
4. ui.app() lives on Ui<T>, not UiCore. Library widgets can't call .app() — which is correct (only user code knows the app type).

That gives you ui.app() (no turbofish) at user code sites, keeps widgets non-generic, and the rename is purely mechanical. Showcase tabs that take &mut Ui and call .app() need to
become &mut Ui<AppState> (small number).
