//! A scene's `init` calls held until every script in it is attached.

use rune::alloc::clone::TryClone as _;

use crate::RuneHost;

impl RuneHost {
    /// Hold the `init` of every script attached from here until the matching
    /// [`Self::release_inits`].
    pub fn hold_inits(&self) {
        self.state.borrow_mut().init_hold += 1;
    }

    /// End a hold; the outermost one runs every held `init`, in attach order,
    /// for the instances still attached.
    pub fn release_inits(&self) {
        let held = {
            let mut state = self.state.borrow_mut();
            state.init_hold = state.init_hold.saturating_sub(1);
            if state.init_hold > 0 {
                return;
            }
            std::mem::take(&mut state.held_inits)
        };
        for (entity, key) in held {
            let live = self
                .state
                .borrow()
                .instances
                .get(&entity)
                .filter(|i| *i.key == *key)
                .and_then(|i| i.state.try_clone().ok());
            if let Some(state) = live {
                self.invoke(entity, &key, "init", (state,), true, None);
            }
        }
    }
}
