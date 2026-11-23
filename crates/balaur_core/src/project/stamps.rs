//! Scene files' modified times held through a batch of builds.

use crate::collections::DetHashMap;
use crate::engine::Engine;

/// Scene files' modified times, remembered while a batch builds: a screen
/// that instances one prefab 150 times asks the file system about it once,
/// not 150 times. A file saved during the batch is read again by the next.
#[derive(Default)]
struct HeldStamps {
    depth: usize,
    stamps: DetHashMap<String, Option<f64>>,
}

/// While this lives, [`super::scene_doc`] checks each scene file's modified time
/// once: what a scene switch and a `repeat` hold while they build.
pub(crate) struct StampsHeld<'a>(&'a Engine);

/// Hold scene files' modified times until the guard drops; holds nest.
pub(crate) fn hold_stamps(eng: &Engine) -> StampsHeld<'_> {
    let held = eng.try_resource::<HeldStamps>().unwrap_or_else(|| {
        eng.insert_resource(HeldStamps::default());
        eng.resource::<HeldStamps>()
    });
    held.borrow_mut().depth += 1;
    StampsHeld(eng)
}

impl Drop for StampsHeld<'_> {
    fn drop(&mut self) {
        let Some(held) = self.0.try_resource::<HeldStamps>() else {
            return;
        };
        let mut held = held.borrow_mut();
        held.depth = held.depth.saturating_sub(1);
        if held.depth == 0 {
            held.stamps.clear();
        }
    }
}

/// [`super::stamp_of`], remembered while a batch holds stamps.
pub(super) fn held_stamp_of(eng: &Engine, path: &str) -> Option<f64> {
    let held = eng.try_resource::<HeldStamps>();
    let Some(held) = held.filter(|held| held.borrow().depth > 0) else {
        return super::stamp_of(eng, path);
    };
    if let Some(&stamp) = held.borrow().stamps.get(path) {
        return stamp;
    }
    let stamp = super::stamp_of(eng, path);
    held.borrow_mut().stamps.insert(path.to_owned(), stamp);
    stamp
}
