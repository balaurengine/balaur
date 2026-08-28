use std::cell::{Cell, Ref, RefCell, RefMut};
use std::rc::Rc;

use crate::resources::Resources;
use crate::script::ScriptHost;

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
/// `Engine` is what Rust systems receive every frame and what Lua binding
/// closures capture, so both sides of the FFI see the exact same state.
#[derive(Clone)]
pub struct Engine {
    inner: Rc<EngineInner>,
}

pub struct EngineInner {
    pub world: RefCell<hecs::World>,
    pub resources: RefCell<Resources>,
    // Option because the host is installed after the engine exists (it needs
    // an Engine clone for its Lua closures). This is a deliberate Rc cycle:
    // the engine is a live-forever singleton.
    pub scripts: RefCell<Option<ScriptHost>>,
    pub commands: RefCell<Vec<Command>>,
    pub root: Cell<hecs::Entity>,
    pub time: Cell<f64>,
    pub delta: Cell<f32>,
    pub quit: Cell<bool>,
}

impl Engine {
    pub fn new() -> Self {
        let mut world = hecs::World::new();
        let root = crate::scene::spawn_root(&mut world);
        Engine {
            inner: Rc::new(EngineInner {
                world: RefCell::new(world),
                resources: RefCell::new(Resources::default()),
                scripts: RefCell::new(None),
                commands: RefCell::new(Vec::new()),
                root: Cell::new(root),
                time: Cell::new(0.0),
                delta: Cell::new(0.0),
                quit: Cell::new(false),
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

    pub fn remove_resource<T: 'static>(&self) {
        self.inner.resources.borrow_mut().remove::<T>();
    }

    pub fn scripts(&self) -> Option<ScriptHost> {
        self.inner.scripts.borrow().clone()
    }

    pub fn set_scripts(&self, host: ScriptHost) {
        *self.inner.scripts.borrow_mut() = Some(host);
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

    pub fn advance_time(&self, dt: f32) {
        self.inner.delta.set(dt);
        self.inner.time.set(self.inner.time.get() + dt as f64);
    }

    pub fn request_quit(&self) {
        self.inner.quit.set(true);
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
