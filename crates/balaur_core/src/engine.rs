use std::cell::{Cell, Ref, RefCell, RefMut};
use std::rc::Rc;

use crate::resources::Resources;

/// Deferred structural change, applied at the end of the frame so scripts can
/// freely request them mid-update without invalidating iteration.
pub enum Command {
    /// Recursively despawn a node and its subtree.
    Free(hecs::Entity),
}

/// Cheap-to-clone handle to the whole engine state. The engine is
/// single-threaded by design (the data plane can go parallel later behind
/// this same facade); interior mutability keeps borrows short and explicit.
///
/// `Engine` is what Rust systems receive every frame and what script binding
/// closures capture, so both sides of the FFI see the exact same state.
#[derive(Clone)]
pub struct Engine {
    inner: Rc<EngineInner>,
}

pub(crate) struct EngineInner {
    pub(crate) world: RefCell<hecs::World>,
    pub(crate) resources: RefCell<Resources>,
    // Option because the host is installed after the engine exists (it needs
    // an Engine clone for its binding closures). This is a deliberate Rc cycle:
    // the engine is a live-forever singleton.
    pub(crate) script_host: RefCell<Option<Rc<dyn balaur_script::ScriptHost<Engine>>>>,
    pub(crate) commands: RefCell<Vec<Command>>,
    pub(crate) root: Cell<hecs::Entity>,
    pub(crate) time: Cell<f64>,
    pub(crate) delta: Cell<f32>,
    pub(crate) tick: Cell<u64>,
    pub(crate) quit: Cell<bool>,
    /// What the process exits with once it quits; 0 unless a script said.
    pub(crate) exit_code: Cell<i32>,
    /// One engine-wide counter behind [`Engine::next_token`], so every
    /// subsystem's awaitable ids share a namespace and a wake can never
    /// resume the wrong task.
    pub(crate) tokens: Cell<u64>,
    /// Bumped each time the app drops its fixed-step remainder at a session
    /// boundary; see [`Engine::step_restarts`].
    pub(crate) step_restarts: Cell<u64>,
    /// The subtree a debugger treats as the game. `None` means the whole tree.
    pub(crate) debug_scope: Cell<Option<hecs::Entity>>,
    pub(crate) frozen: Cell<bool>,
    /// A paused replay, held apart from `frozen` so releasing one does not
    /// release the other: a breakpoint inside a replay is both at once.
    pub(crate) replay_hold: Cell<bool>,
    /// The game's own pause, which a script owns and the debugger's freeze
    /// knows nothing about.
    pub(crate) paused: Cell<bool>,
    /// A pause or a resume nobody has announced yet, for `on_paused_changed`.
    pub(crate) pause_change: Cell<Option<bool>>,
    /// What measured frame time is multiplied by before it is owed to the
    /// fixed step: slow motion, fast forward, and 0 for neither.
    pub(crate) time_scale: Cell<f32>,
    /// How far the frame being drawn sits between the last fixed step and
    /// the next, 0 to 1. Render-side alone; see [`crate::interpolate`].
    pub(crate) frame_alpha: Cell<f32>,
}

impl Engine {
    pub fn new() -> Self {
        let mut world = hecs::World::new();
        let root = crate::scene::spawn_root(&mut world);
        Self {
            inner: Rc::new(EngineInner {
                world: RefCell::new(world),
                resources: RefCell::new(Resources::default()),
                script_host: RefCell::new(None),
                commands: RefCell::new(Vec::new()),
                root: Cell::new(root),
                time: Cell::new(0.0),
                delta: Cell::new(0.0),
                tick: Cell::new(0),
                quit: Cell::new(false),
                exit_code: Cell::new(0),
                tokens: Cell::new(1),
                step_restarts: Cell::new(0),
                debug_scope: Cell::new(None),
                frozen: Cell::new(false),
                replay_hold: Cell::new(false),
                paused: Cell::new(false),
                pause_change: Cell::new(None),
                time_scale: Cell::new(1.0),
                frame_alpha: Cell::new(0.0),
            }),
        }
    }

    pub fn world(&self) -> Ref<'_, hecs::World> {
        self.inner.world.borrow()
    }

    pub fn world_mut(&self) -> RefMut<'_, hecs::World> {
        self.inner.world.borrow_mut()
    }

    pub fn insert_resource<T: 'static>(&self, value: T) {
        self.inner.resources.borrow_mut().insert(value);
    }

    /// Panics if the resource was never inserted; use `try_resource` when the
    /// plugin providing it is optional.
    pub fn resource<T: 'static>(&self) -> Rc<RefCell<T>> {
        self.try_resource::<T>()
            .unwrap_or_else(|| panic!("missing resource {}", std::any::type_name::<T>()))
    }

    pub fn try_resource<T: 'static>(&self) -> Option<Rc<RefCell<T>>> {
        self.inner.resources.borrow().get::<T>()
    }

    /// A fresh engine-wide id for anything a script can wait on — an http
    /// request, a connection, a backend call. One counter for every
    /// subsystem, so a `ScriptHost::wake` token names exactly one operation.
    pub fn next_token(&self) -> u64 {
        let id = self.inner.tokens.get();
        self.inner.tokens.set(id + 1);
        id
    }

    /// What [`Engine::next_token`] will hand out next.
    pub fn tokens(&self) -> u64 {
        self.inner.tokens.get()
    }

    /// Put the token counter back to where a recorded session started.
    ///
    /// For [`crate::replay`] and nothing else: a replay's http replies are
    /// keyed by the id the request took, so the ids have to come out the
    /// same. Anything else calling this hands two live operations one id.
    pub fn set_tokens(&self, next: u64) {
        self.inner.tokens.set(next);
    }

    /// How many times the fixed step has restarted from a frame boundary: a
    /// recording starting, and a replay starting. A plugin that keeps its own
    /// accumulator drops its remainder when this moves, as the app drops its
    /// own, or a session replayed in a long-lived process takes different steps.
    pub fn step_restarts(&self) -> u64 {
        self.inner.step_restarts.get()
    }

    pub(crate) fn restart_steps(&self) {
        self.inner
            .step_restarts
            .set(self.inner.step_restarts.get() + 1);
    }

    pub fn remove_resource<T: 'static>(&self) {
        self.inner.resources.borrow_mut().remove::<T>();
    }

    /// The script subsystem, as a trait object: the engine does not know
    /// which language is running.
    pub fn script_host(&self) -> Option<Rc<dyn balaur_script::ScriptHost<Engine>>> {
        self.inner.script_host.borrow().clone()
    }

    pub fn set_script_host(&self, host: Rc<dyn balaur_script::ScriptHost<Engine>>) {
        *self.inner.script_host.borrow_mut() = Some(host);
    }

    pub fn push_command(&self, cmd: Command) {
        self.inner.commands.borrow_mut().push(cmd);
    }

    pub fn take_commands(&self) -> Vec<Command> {
        std::mem::take(&mut self.inner.commands.borrow_mut())
    }

    pub fn root(&self) -> hecs::Entity {
        self.inner.root.get()
    }

    pub fn time(&self) -> f64 {
        self.inner.time.get()
    }

    pub fn delta(&self) -> f32 {
        self.inner.delta.get()
    }

    /// How many frames have run. `App::tick` runs one; this reports which.
    ///
    /// A replay, a digest trace and a networked peer all key off this rather
    /// than off [`Engine::time`], which accumulates float error.
    pub fn tick(&self) -> u64 {
        self.inner.tick.get()
    }

    pub fn advance_time(&self, dt: f32) {
        self.inner.delta.set(dt);
        self.inner.time.set(self.inner.time.get() + f64::from(dt));
        self.inner.tick.set(self.inner.tick.get() + 1);
    }

    /// The frame time of a frame that is not a tick: a paused replay still
    /// draws, and counting its frames would run every later replayed tick at
    /// a number the recording never had.
    pub fn hold_time(&self, dt: f32) {
        self.inner.delta.set(dt);
    }

    /// Put the clock back to where a recorded session started, so a replay
    /// reports the tick and time the recording did.
    ///
    /// The counters keep running across an editor's play sessions; a script
    /// that branches on `engine::tick()` would otherwise see different
    /// numbers on the replay than it saw when the session was recorded.
    pub fn set_clock(&self, tick: u64, time: f64) {
        self.inner.tick.set(tick);
        self.inner.time.set(time);
    }

    pub fn request_quit(&self) {
        self.inner.quit.set(true);
    }

    /// [`Engine::request_quit`], with the code the process exits with: what
    /// a test harness's failure reaches the shell as.
    pub fn request_quit_with(&self, code: i32) {
        self.inner.exit_code.set(code);
        self.request_quit();
    }

    pub fn exit_code(&self) -> i32 {
        self.inner.exit_code.get()
    }

    /// Name the subtree a debugger pause holds still: the editor's mirror
    /// during play. `None` is the whole tree, which is what `balaur run` wants.
    pub fn set_debug_scope(&self, root: Option<hecs::Entity>) {
        self.inner.debug_scope.set(root);
    }

    pub fn debug_scope(&self) -> Option<hecs::Entity> {
        self.inner.debug_scope.get()
    }

    /// Hold the simulation while a script is paused. The frame loop keeps
    /// running; [`Engine::frozen_root`] says what stops.
    pub fn set_frozen(&self, frozen: bool) {
        self.inner.frozen.set(frozen);
    }

    /// Whether a debugger pause is holding the simulation, apart from a
    /// paused replay's own hold.
    pub fn is_frozen(&self) -> bool {
        self.inner.frozen.get()
    }

    /// Hold the simulation for a paused replay. Independent of
    /// [`Engine::set_frozen`], which is the debugger's.
    pub fn set_replay_hold(&self, held: bool) {
        self.inner.replay_hold.set(held);
    }

    /// The subtree a debugger pause or a paused replay holds still: no fixed
    /// step runs, and the script hosts skip every instance under it. `None`
    /// while nothing holds it.
    pub fn frozen_root(&self) -> Option<hecs::Entity> {
        (self.inner.frozen.get() || self.inner.replay_hold.get())
            .then(|| self.inner.debug_scope.get().unwrap_or_else(|| self.root()))
    }

    /// Pause or resume the game: every node whose `process` mode is
    /// `pausable` stops ticking, physics holds both worlds, and the frame
    /// loop keeps drawing. `always` and `when_paused` subtrees are what runs
    /// through it.
    pub fn set_paused(&self, paused: bool) {
        if self.inner.paused.get() == paused {
            return;
        }
        self.inner.paused.set(paused);
        self.inner.pause_change.set(Some(paused));
    }

    #[must_use]
    pub fn paused(&self) -> bool {
        self.inner.paused.get()
    }

    /// The change `on_paused_changed` has yet to announce, taken so it is announced
    /// once. A pause and a resume inside one frame cancel to the last state,
    /// which is the one a script would have been told about anyway.
    pub fn take_pause_change(&self) -> Option<bool> {
        self.inner.pause_change.take()
    }

    /// Multiply wall-clock time by `scale` before the simulation is owed it:
    /// half takes half the fixed steps, double takes twice, each still a
    /// whole fixed step. Negative is refused; zero is a pause that
    /// keeps every node ticking with no time passing.
    ///
    /// A replay and a rollback session drive by tick and ignore it: a scale
    /// is a wall-clock matter, not simulation state.
    pub fn set_time_scale(&self, scale: f32) {
        self.inner.time_scale.set(scale.max(0.0));
    }

    #[must_use]
    pub fn time_scale(&self) -> f32 {
        self.inner.time_scale.get()
    }

    /// Where the frame being drawn sits between the last fixed step and the
    /// next, 0 to 1. Written by the app once per frame, read by the scene
    /// sync and by nothing a script can reach.
    pub fn set_frame_alpha(&self, alpha: f32) {
        self.inner.frame_alpha.set(alpha.clamp(0.0, 1.0));
    }

    #[must_use]
    pub fn frame_alpha(&self) -> f32 {
        self.inner.frame_alpha.get()
    }

    pub fn quit_requested(&self) -> bool {
        self.inner.quit.get()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// Lets a binding call back into script without knowing the backend.
impl balaur_script::CallbackHost for Engine {
    fn invoke(
        &self,
        callback: balaur_script::CallbackId,
        args: &[balaur_script::Value],
    ) -> anyhow::Result<balaur_script::Value> {
        self.script_host()
            .ok_or_else(|| anyhow::anyhow!("no script backend is running"))?
            .invoke(callback, args)
    }
}
