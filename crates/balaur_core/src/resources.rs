use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Type-indexed storage for engine-global singletons (physics state, render
/// backend, audio device, ...). Each resource lives behind its own `RefCell`
/// so plugins can borrow different resources at the same time.
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
