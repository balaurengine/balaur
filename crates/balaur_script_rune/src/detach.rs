//! Taking scripts off nodes that are being freed.

use hecs::Entity;
use rustc_hash::FxHashMap;

use crate::RuneHost;

impl RuneHost {
    /// Tasks the node's script left suspended die with it, a pause included.
    pub fn detach(&self, entity: Entity) {
        let (inst, paused) = {
            let mut state = self.state.borrow_mut();
            state.tasks.retain(|t| t.owner != entity);
            let paused = state.paused.take_if(|p| p.owner == entity);
            (state.instances.shift_remove(&entity), paused)
        };
        if let Some(paused) = paused {
            self.drop_pause(&paused);
        }
        if let Some(inst) = inst
            && let Some(on_free) = self.method(&inst.key, balaur_core::hooks::ON_FREE)
            && let Err(err) = on_free.call::<()>((inst.state,)).into_result()
        {
            self.report(&inst.key, balaur_core::hooks::ON_FREE, &err);
        }
    }

    /// [`Self::detach`] for many nodes: one pass over the instances, where a
    /// `shift_remove` per node moves every instance behind it.
    ///
    /// Every instance leaves before the first `on_free` runs, which then run
    /// in the order the nodes were given.
    pub fn detach_all(&self, entities: &[Entity]) {
        let doomed: rustc_hash::FxHashSet<Entity> = entities.iter().copied().collect();
        let (mut gone, paused) = {
            let mut state = self.state.borrow_mut();
            state.tasks.retain(|t| !doomed.contains(&t.owner));
            let paused = state.paused.take_if(|p| doomed.contains(&p.owner));
            let mut gone = FxHashMap::default();
            if entities.iter().any(|e| state.instances.contains_key(e)) {
                let held = std::mem::take(&mut state.instances);
                state.instances = held
                    .into_iter()
                    .filter_map(|(e, inst)| {
                        if doomed.contains(&e) {
                            gone.insert(e, inst);
                            None
                        } else {
                            Some((e, inst))
                        }
                    })
                    .collect();
            }
            (gone, paused)
        };
        if let Some(paused) = paused {
            self.drop_pause(&paused);
        }
        for entity in entities {
            if let Some(inst) = gone.remove(entity)
                && let Some(on_free) = self.method(&inst.key, balaur_core::hooks::ON_FREE)
                && let Err(err) = on_free.call::<()>((inst.state,)).into_result()
            {
                self.report(&inst.key, balaur_core::hooks::ON_FREE, &err);
            }
        }
    }
}
