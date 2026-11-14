//! Typed systems (PLAN-ecs §2): a function whose parameters say what it
//! reads and writes, turned into a system.
//!
//! ```ignore
//! fn steer(mut ships: Query<'_, (&mut Transform, &Ship)>, wind: Res<'_, Wind>, step: Step) {
//!     ships.for_each(|(transform, ship)| ..);
//! }
//! app.add_typed(Stage::FixedUpdate, steer);
//! ```
//!
//! Each parameter is borrowed through the engine's `RefCell`s for the run,
//! and a clash (a resource held mutably by another parameter, or by code
//! still running) panics naming the system and the parameter. What a
//! parameter keeps between runs (a [`Local`], a reader's cursor) lives with
//! the system. A plain `FnMut(&Engine, f32)` stays a system: `add_system`.

use std::cell::{Ref, RefCell, RefMut};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use hecs::Entity;

use crate::app::SystemFn;
use crate::components::derived::Component;
use crate::components::ticks::{self, ChangeCursor};
use crate::engine::Engine;
use crate::messages::MessageCursor;

/// Where a parameter is fetched for: the system's name and the parameter's
/// type, for what a clash says.
pub struct ParamAt<'a> {
    pub system: &'a str,
    pub param: &'static str,
}

/// A value a typed system takes, fetched for each run.
pub trait SystemParam {
    /// What the parameter keeps between runs.
    type State: 'static;
    /// What the system is handed for one run.
    type Item<'w>;
    fn init(eng: &Engine) -> Self::State;
    fn fetch<'w>(
        state: &'w mut Self::State,
        eng: &'w Engine,
        step: f32,
        at: &ParamAt<'_>,
    ) -> Self::Item<'w>;
}

fn resource<T: 'static>(eng: &Engine, at: &ParamAt<'_>) -> Rc<RefCell<T>> {
    eng.try_resource::<T>().unwrap_or_else(|| {
        panic!(
            "system {} asks for {}, and no {} resource was inserted",
            at.system,
            at.param,
            std::any::type_name::<T>()
        )
    })
}

/// A resource, read.
pub struct Res<'w, T: 'static>(Ref<'w, T>);

impl<T> Deref for Res<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: 'static> SystemParam for Res<'_, T> {
    type State = Option<Rc<RefCell<T>>>;
    type Item<'w> = Res<'w, T>;
    fn init(_: &Engine) -> Self::State {
        None
    }
    fn fetch<'w>(
        state: &'w mut Self::State,
        eng: &'w Engine,
        _: f32,
        at: &ParamAt<'_>,
    ) -> Res<'w, T> {
        let held = state.get_or_insert_with(|| resource::<T>(eng, at));
        Res(held.try_borrow().unwrap_or_else(|_| {
            panic!(
                "system {}: {} clashes with a write of it held elsewhere",
                at.system, at.param
            )
        }))
    }
}

/// A resource, written.
pub struct ResMut<'w, T: 'static>(RefMut<'w, T>);

impl<T> Deref for ResMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for ResMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: 'static> SystemParam for ResMut<'_, T> {
    type State = Option<Rc<RefCell<T>>>;
    type Item<'w> = ResMut<'w, T>;
    fn init(_: &Engine) -> Self::State {
        None
    }
    fn fetch<'w>(
        state: &'w mut Self::State,
        eng: &'w Engine,
        _: f32,
        at: &ParamAt<'_>,
    ) -> ResMut<'w, T> {
        let held = state.get_or_insert_with(|| resource::<T>(eng, at));
        ResMut(held.try_borrow_mut().unwrap_or_else(|_| {
            panic!(
                "system {}: {} clashes with a read or write of it held elsewhere",
                at.system, at.param
            )
        }))
    }
}

/// A resource that may not be there, read: `None` until it is inserted.
impl<T: 'static> SystemParam for Option<Res<'_, T>> {
    type State = Option<Rc<RefCell<T>>>;
    type Item<'w> = Option<Res<'w, T>>;
    fn init(_: &Engine) -> Self::State {
        None
    }
    fn fetch<'w>(
        state: &'w mut Self::State,
        eng: &'w Engine,
        _: f32,
        at: &ParamAt<'_>,
    ) -> Option<Res<'w, T>> {
        if state.is_none() {
            *state = eng.try_resource::<T>();
        }
        let held = state.as_ref()?;
        Some(Res(held.try_borrow().unwrap_or_else(|_| {
            panic!(
                "system {}: {} clashes with a write of it held elsewhere",
                at.system, at.param
            )
        })))
    }
}

/// A resource that may not be there, written: `None` until it is inserted.
impl<T: 'static> SystemParam for Option<ResMut<'_, T>> {
    type State = Option<Rc<RefCell<T>>>;
    type Item<'w> = Option<ResMut<'w, T>>;
    fn init(_: &Engine) -> Self::State {
        None
    }
    fn fetch<'w>(
        state: &'w mut Self::State,
        eng: &'w Engine,
        _: f32,
        at: &ParamAt<'_>,
    ) -> Option<ResMut<'w, T>> {
        if state.is_none() {
            *state = eng.try_resource::<T>();
        }
        let held = state.as_ref()?;
        Some(ResMut(held.try_borrow_mut().unwrap_or_else(|_| {
            panic!(
                "system {}: {} clashes with a read or write of it held elsewhere",
                at.system, at.param
            )
        })))
    }
}

/// The entities matching `Q`: [`Query::for_each`], or [`Query::borrow`] and
/// its `iter()`. Two queries writing one component clash when both are
/// borrowed at once, which hecs refuses naming the component.
pub struct Query<'w, Q: hecs::Query> {
    world: Ref<'w, hecs::World>,
    marker: std::marker::PhantomData<fn() -> Q>,
}

impl<Q: hecs::Query> Query<'_, Q> {
    /// Every match, borrowed for as long as the borrow lives.
    pub fn borrow(&mut self) -> hecs::QueryBorrow<'_, Q> {
        self.world.query::<Q>()
    }

    /// Hand `each` every match.
    pub fn for_each(&mut self, mut each: impl FnMut(Q::Item<'_>)) {
        for item in &mut self.world.query::<Q>() {
            each(item);
        }
    }

    /// Hand `each` every match, in batches of `batch` on the thread pool
    /// natively and in order on the web. Each match is worked alone, so `each`
    /// writes only what its own match holds.
    pub fn par_for_each(&mut self, batch: u32, each: impl Fn(Q::Item<'_>) + Send + Sync)
    where
        for<'a> Q::Item<'a>: Send,
    {
        self.par_fold(batch, || (), |(), item| each(item), |(), ()| ());
    }

    /// Fold every match into one value: each batch of `batch` from `start()`
    /// with `fold`, on the thread pool natively, then the batches' values
    /// combined in batch order, so the answer is the same on every run and
    /// every target whatever the threads did.
    pub fn par_fold<R: Send>(
        &mut self,
        batch: u32,
        start: impl Fn() -> R + Send + Sync,
        fold: impl Fn(R, Q::Item<'_>) -> R + Send + Sync,
        combine: impl Fn(R, R) -> R,
    ) -> R
    where
        for<'a> Q::Item<'a>: Send,
    {
        let mut borrowed = self.world.query::<Q>();
        let batches: Vec<hecs::Batch<'_, Q>> = borrowed.iter_batched(batch.max(1)).collect();
        let now = crate::components::ticks::now_here();
        let each_batch = |batch: hecs::Batch<'_, Q>| {
            // A pool thread stamps with the system's tick, as this one would.
            crate::components::ticks::set_now(now);
            let out = batch.fold(start(), &fold);
            // A tracked write noted on a pool thread reaches the drain.
            crate::components::ticks::hand_over();
            out
        };
        #[cfg(not(target_family = "wasm"))]
        let partials: Vec<R> = {
            use rayon::prelude::*;
            batches.into_par_iter().map(each_batch).collect()
        };
        #[cfg(target_family = "wasm")]
        let partials: Vec<R> = batches.into_iter().map(each_batch).collect();
        partials.into_iter().reduce(combine).unwrap_or_else(start)
    }

    /// One entity's match: its `get` answers it, or why there is none.
    pub fn one(&self, entity: Entity) -> hecs::QueryOne<'_, Q> {
        self.world.query_one::<Q>(entity)
    }
}

impl<Q: hecs::Query + 'static> SystemParam for Query<'_, Q> {
    type State = ();
    type Item<'w> = Query<'w, Q>;
    fn init(_: &Engine) {}
    fn fetch<'w>((): &'w mut (), eng: &'w Engine, _: f32, at: &ParamAt<'_>) -> Query<'w, Q> {
        let world = eng.inner.world.try_borrow().unwrap_or_else(|_| {
            panic!(
                "system {}: {} needs the world, which is being written",
                at.system, at.param
            )
        });
        Query {
            world,
            marker: std::marker::PhantomData,
        }
    }
}

/// A query term that writes a component and is seen doing it:
/// `Query<'_, (Tracked<Pos>, &Vel)>`. Its item, [`Mut`], notes the write the
/// first time it is written through, and the write is stamped with the
/// system's change tick when the system ends, so [`Changed`] hears it. A plain `&mut T` is free and unseen.
pub struct Tracked<T>(std::marker::PhantomData<fn() -> T>);

/// A tracked query's component, borrowed: read freely, stamped when written.
pub struct Mut<'a, T: 'static> {
    value: &'a mut T,
    column: Option<&'a mut crate::components::ticks::Ticked<T>>,
    node: Entity,
    written: bool,
}

impl<T> Mut<'_, T> {
    /// The node the component sits on.
    #[must_use]
    pub fn node(&self) -> Entity {
        self.node
    }
}

impl<T> Deref for Mut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}

impl<T> DerefMut for Mut<'_, T> {
    /// The first write stores the system's change tick in the column beside
    /// the component; a component put on around the registry has none, and
    /// its write is noted and stamped when the system ends.
    fn deref_mut(&mut self) -> &mut T {
        if !self.written {
            self.written = true;
            match &mut self.column {
                Some(column) => column.changed = crate::components::ticks::now_here(),
                None => {
                    crate::components::ticks::note_written(std::any::TypeId::of::<T>(), self.node);
                }
            }
        }
        self.value
    }
}

/// What [`Tracked`] fetches: the component, its tick column where it has one,
/// and the node, as hecs fetches the three.
type Parts<T> = (
    &'static mut T,
    Option<&'static mut crate::components::ticks::Ticked<T>>,
    Entity,
);

/// [`Tracked`]'s fetch: hecs's own for its three parts.
pub struct FetchTracked<T: hecs::Component>(<Parts<T> as hecs::Query>::Fetch);

impl<T: hecs::Component> Clone for FetchTracked<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

// SAFETY: every call is hecs's own fetch of the three parts, passed through
// unchanged, so its contracts hold as they do for that query.
unsafe impl<T: hecs::Component> hecs::Fetch for FetchTracked<T> {
    type State = <<Parts<T> as hecs::Query>::Fetch as hecs::Fetch>::State;

    fn dangling() -> Self {
        Self(hecs::Fetch::dangling())
    }
    fn access(archetype: &hecs::Archetype) -> Option<hecs::Access> {
        <<Parts<T> as hecs::Query>::Fetch as hecs::Fetch>::access(archetype)
    }
    fn borrow(archetype: &hecs::Archetype, state: Self::State) {
        <<Parts<T> as hecs::Query>::Fetch as hecs::Fetch>::borrow(archetype, state);
    }
    fn prepare(archetype: &hecs::Archetype) -> Option<Self::State> {
        <<Parts<T> as hecs::Query>::Fetch as hecs::Fetch>::prepare(archetype)
    }
    fn execute(archetype: &hecs::Archetype, state: Self::State) -> Self {
        Self(<<Parts<T> as hecs::Query>::Fetch as hecs::Fetch>::execute(
            archetype, state,
        ))
    }
    fn release(archetype: &hecs::Archetype, state: Self::State) {
        <<Parts<T> as hecs::Query>::Fetch as hecs::Fetch>::release(archetype, state);
    }
    fn for_each_borrow(f: impl FnMut(std::any::TypeId, bool)) {
        <<Parts<T> as hecs::Query>::Fetch as hecs::Fetch>::for_each_borrow(f);
    }
}

impl<T: hecs::Component> hecs::Query for Tracked<T> {
    type Item<'q> = Mut<'q, T>;
    type Fetch = FetchTracked<T>;

    unsafe fn get<'q>(meta: &[hecs::EntityMeta], fetch: &Self::Fetch, n: usize) -> Mut<'q, T> {
        // SAFETY: hecs's own get of the three parts, under the contract this
        // one was called on.
        let (value, column, node) = unsafe { <Parts<T> as hecs::Query>::get(meta, &fetch.0, n) };
        Mut {
            value,
            column,
            node,
            written: false,
        }
    }
}

/// Writes made at the end of the stage ([`Engine::defer`] and its kin).
pub struct Commands<'w>(&'w Engine);

impl Deref for Commands<'_> {
    type Target = Engine;
    fn deref(&self) -> &Engine {
        self.0
    }
}

impl SystemParam for Commands<'_> {
    type State = ();
    type Item<'w> = Commands<'w>;
    fn init(_: &Engine) {}
    fn fetch<'w>((): &'w mut (), eng: &'w Engine, _: f32, _: &ParamAt<'_>) -> Commands<'w> {
        Commands(eng)
    }
}

/// A value the system keeps between its runs, starting at its default.
pub struct Local<'w, T>(&'w mut T);

impl<T> Deref for Local<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0
    }
}

impl<T> DerefMut for Local<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.0
    }
}

impl<T: Default + 'static> SystemParam for Local<'_, T> {
    type State = T;
    type Item<'w> = Local<'w, T>;
    fn init(_: &Engine) -> T {
        T::default()
    }
    fn fetch<'w>(state: &'w mut T, _: &'w Engine, _: f32, _: &ParamAt<'_>) -> Local<'w, T> {
        Local(state)
    }
}

/// The seconds the stage handed this run: the fixed step in `FixedUpdate`.
#[derive(Clone, Copy, Debug)]
pub struct Step(pub f32);

impl SystemParam for Step {
    type State = ();
    type Item<'w> = Step;
    fn init(_: &Engine) {}
    fn fetch<'w>((): &'w mut (), _: &'w Engine, step: f32, _: &ParamAt<'_>) -> Step {
        Step(step)
    }
}

/// Messages of type `T` sent since this system last read them.
pub struct MessageReader<'w, T: 'static> {
    cursor: &'w mut MessageCursor<T>,
    eng: &'w Engine,
}

impl<T: 'static> MessageReader<'_, T> {
    /// Hand `each` every message this system has not read, oldest first.
    pub fn read(&mut self, each: impl FnMut(&T)) -> u64 {
        self.cursor.read(self.eng, each)
    }
}

impl<T: 'static> SystemParam for MessageReader<'_, T> {
    type State = MessageCursor<T>;
    type Item<'w> = MessageReader<'w, T>;
    fn init(_: &Engine) -> MessageCursor<T> {
        MessageCursor::default()
    }
    fn fetch<'w>(
        state: &'w mut MessageCursor<T>,
        eng: &'w Engine,
        _: f32,
        _: &ParamAt<'_>,
    ) -> MessageReader<'w, T> {
        MessageReader { cursor: state, eng }
    }
}

/// Sends messages of type `T`.
pub struct MessageWriter<'w, T: 'static> {
    eng: &'w Engine,
    marker: std::marker::PhantomData<fn(T)>,
}

impl<T: 'static> MessageWriter<'_, T> {
    pub fn send(&self, message: T) {
        self.eng.send(message);
    }
}

impl<T: 'static> SystemParam for MessageWriter<'_, T> {
    type State = ();
    type Item<'w> = MessageWriter<'w, T>;
    fn init(_: &Engine) {}
    fn fetch<'w>((): &'w mut (), eng: &'w Engine, _: f32, _: &ParamAt<'_>) -> MessageWriter<'w, T> {
        MessageWriter {
            eng,
            marker: std::marker::PhantomData,
        }
    }
}

/// The nodes a registered component `C` was written on, or put on, since this
/// system last ran, in node order.
pub struct Changed<C: Component>(pub Vec<Entity>, std::marker::PhantomData<fn() -> C>);

/// The nodes a registered component `C` was put on since this system last
/// ran, in node order.
pub struct Added<C: Component>(pub Vec<Entity>, std::marker::PhantomData<fn() -> C>);

impl<C: Component> Deref for Changed<C> {
    type Target = [Entity];
    fn deref(&self) -> &[Entity] {
        &self.0
    }
}

impl<C: Component> Deref for Added<C> {
    type Target = [Entity];
    fn deref(&self) -> &[Entity] {
        &self.0
    }
}

/// Component `C`'s place in the registry, found once, and the change tick
/// the system last read at.
pub struct SinceState {
    index: Option<usize>,
    cursor: ChangeCursor,
}

fn since<C: Component>(state: &mut SinceState, eng: &Engine, at: &ParamAt<'_>) -> (usize, u64) {
    let index = *state.index.get_or_insert_with(|| {
        crate::components::index_of(eng, C::NAME).unwrap_or_else(|| {
            panic!(
                "system {}: {} names {}, which nothing registered",
                at.system,
                at.param,
                C::NAME
            )
        })
    });
    (index, state.cursor.read(eng))
}

impl<C: Component> SystemParam for Changed<C> {
    type State = SinceState;
    type Item<'w> = Changed<C>;
    fn init(_: &Engine) -> SinceState {
        SinceState {
            index: None,
            cursor: ChangeCursor::default(),
        }
    }
    fn fetch<'w>(
        state: &'w mut SinceState,
        eng: &'w Engine,
        _: f32,
        at: &ParamAt<'_>,
    ) -> Changed<C> {
        let (index, from) = since::<C>(state, eng, at);
        Changed(
            ticks::changed_since(eng, index, from),
            std::marker::PhantomData,
        )
    }
}

impl<C: Component> SystemParam for Added<C> {
    type State = SinceState;
    type Item<'w> = Added<C>;
    fn init(_: &Engine) -> SinceState {
        SinceState {
            index: None,
            cursor: ChangeCursor::default(),
        }
    }
    fn fetch<'w>(state: &'w mut SinceState, eng: &'w Engine, _: f32, at: &ParamAt<'_>) -> Added<C> {
        let (index, from) = since::<C>(state, eng, at);
        Added(
            ticks::added_since(eng, index, from),
            std::marker::PhantomData,
        )
    }
}

/// A function of [`SystemParam`]s, turned into a system. `Marker` tells the
/// arities apart.
pub trait TypedSystem<Marker>: 'static {
    fn into_run(self, name: String) -> SystemFn;
}

macro_rules! typed_system {
    ($($param:ident),*) => {
        #[allow(
            non_snake_case,
            clippy::too_many_arguments,
            reason = "a parameter's type and its value share a name in the macro"
        )]
        impl<Func, $($param: SystemParam + 'static),*> TypedSystem<fn($($param,)*)> for Func
        where
            Func: 'static,
            for<'w> &'w mut Func: FnMut($($param),*) + FnMut($($param::Item<'w>),*),
        {
            fn into_run(mut self, name: String) -> SystemFn {
                let mut states: Option<($($param::State,)*)> = None;
                Box::new(move |eng: &Engine, step: f32| {
                    // Naming the call's parameters apart from the system's two
                    // bounds is what lets the compiler pick the fetched one.
                    fn call<$($param),*>(mut run: impl FnMut($($param),*), $($param: $param),*) {
                        run($($param),*);
                    }
                    let ($($param,)*) = states.get_or_insert_with(|| ($($param::init(eng),)*));
                    call(
                        &mut self,
                        $($param::fetch(
                            $param,
                            eng,
                            step,
                            &ParamAt { system: &name, param: std::any::type_name::<$param>() },
                        )),*
                    );
                })
            }
        }
    };
}

typed_system!(P1);
typed_system!(P1, P2);
typed_system!(P1, P2, P3);
typed_system!(P1, P2, P3, P4);
typed_system!(P1, P2, P3, P4, P5);
typed_system!(P1, P2, P3, P4, P5, P6);
typed_system!(P1, P2, P3, P4, P5, P6, P7);
typed_system!(P1, P2, P3, P4, P5, P6, P7, P8);
