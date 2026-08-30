use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Type-indexed storage for engine-global singletons: the physics worlds
/// (`PhysicsState`, `PhysicsState2d`), the audio device (`AudioState`), the
/// camera the renderer applies (`CameraConfig`), the input published each
/// frame (`InputSnapshot`), the random stream (`RngState`), and the rest.
/// The render backend itself is *not* in here — kiss3d owns the OS event
/// loop outside the map and only inserts a `WindowedBackend` marker so the
/// headless fallbacks know to stand down. Each resource lives behind its own
/// `RefCell` so plugins can borrow different resources at the same time.
#[derive(Default)]
pub struct Resources {
    map: HashMap<TypeId, Rc<dyn Any>>,
}

impl Resources {
    pub fn insert<T: 'static>(&mut self, value: T) {
        self.map
            .insert(TypeId::of::<T>(), Rc::new(RefCell::new(value)));
    }

    pub fn get<T: 'static>(&self) -> Option<Rc<RefCell<T>>> {
        self.map
            .get(&TypeId::of::<T>())
            .cloned()
            .and_then(|rc| rc.downcast::<RefCell<T>>().ok())
    }

    pub fn remove<T: 'static>(&mut self) {
        self.map.remove(&TypeId::of::<T>());
    }
}
