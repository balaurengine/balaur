use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::rc::Rc;

use crate::collections::DetHashMap;

/// Type-indexed storage for engine-global singletons: the physics worlds
/// (`PhysicsState`, `PhysicsState2d`), the audio device (`AudioState`), the
/// camera the renderer applies (`CameraConfig`), the input published each
/// frame (`InputSnapshot`), the random stream (`RngState`), and the rest.
/// The render backend itself is *not* in here — kiss3d owns the OS event
/// loop outside the map and only inserts a `WindowedBackend` marker so the
/// headless fallbacks know to stand down. Each resource lives behind its own
/// `RefCell` so plugins can borrow different resources at the same time.
///
/// Keyed with the same fast hasher as every other engine map: the standard
/// one SipHashes a `TypeId` on every `resource()` call, and a binding makes
/// several of those per script call.
#[derive(Default)]
pub struct Resources {
    map: DetHashMap<TypeId, Rc<dyn Any>>,
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
        self.map.swap_remove(&TypeId::of::<T>());
    }
}
