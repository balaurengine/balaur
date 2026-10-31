//! App modes (PLAN-ecs §2, Bevy's `States`): a value the whole app is in, a
//! menu or a level or a pause, with systems run on entering and leaving each
//! value.
//!
//! A mode set now changes at the start of the next frame, before `First`, in
//! the order the modes were declared: leaving runs the old value's exit
//! systems, entering the new one's enter systems. A sub-mode exists only while
//! its parent mode holds one value, and starts at its first value each time
//! the parent comes to it. A computed mode is worked out from the others each
//! frame. Every mode is in the digest and the snapshot; a restore puts the
//! value back without entering or leaving anything.

use std::any::type_name;
use std::cell::RefCell;
use std::rc::Rc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::app::{App, SystemFn};
use crate::engine::Engine;

/// What a mode's value is: a small value compared, copied, and written into a
/// snapshot.
pub trait Mode: Copy + Eq + std::fmt::Debug + Serialize + DeserializeOwned + 'static {}
impl<T: Copy + Eq + std::fmt::Debug + Serialize + DeserializeOwned + 'static> Mode for T {}

/// How a mode decides its next value.
enum Rule<M> {
    /// Whatever was last set.
    Set,
    /// Set, while `parent` holds; `first` each time it comes to hold.
    Sub {
        parent: Rc<dyn Fn(&Engine) -> bool>,
        first: M,
    },
    /// Worked out from the other modes each frame.
    Computed(fn(&Engine) -> Option<M>),
}

/// One mode: its value, what was set for next, its enter and exit systems.
struct ModeState<M> {
    current: Option<M>,
    next: Option<M>,
    rule: Rule<M>,
    enter: Vec<(M, SystemFn)>,
    exit: Vec<(M, SystemFn)>,
}

/// Each declared mode's change, in the order declared.
#[derive(Default)]
struct ModeKinds(Vec<fn(&Engine)>);

fn state<M: Mode>(eng: &Engine) -> Rc<RefCell<ModeState<M>>> {
    eng.try_resource::<ModeState<M>>().unwrap_or_else(|| {
        panic!(
            "mode {} was never declared with App::add_mode",
            type_name::<M>()
        )
    })
}

impl Engine {
    /// The value mode `M` is in; `None` before the first frame, and for a
    /// sub-mode whose parent is elsewhere.
    #[must_use]
    pub fn mode<M: Mode>(&self) -> Option<M> {
        self.try_resource::<ModeState<M>>()
            .and_then(|held| held.borrow().current)
    }

    /// Put mode `M` in `value` at the start of the next frame. A computed
    /// mode cannot be set, and a sub-mode whose parent is elsewhere ignores it.
    ///
    /// # Panics
    /// If `M` was never declared.
    pub fn set_mode<M: Mode>(&self, value: M) {
        let held = state::<M>(self);
        let mut held = held.borrow_mut();
        if matches!(held.rule, Rule::Computed(_)) {
            tracing::warn!(mode = type_name::<M>(), "a computed mode cannot be set");
            return;
        }
        held.next = Some(value);
    }
}

/// A condition for `run_if`: mode `M` is in `value`.
pub fn in_mode<M: Mode>(value: M) -> impl FnMut(&Engine) -> bool + 'static {
    move |eng| eng.mode::<M>() == Some(value)
}

impl App {
    /// Declare mode `M`, entering `first` at the first frame.
    pub fn add_mode<M: Mode>(&mut self, first: M) -> &mut Self {
        self.declare_mode(Rule::Set, Some(first))
    }

    /// Declare mode `S`, held only while mode `P` is in `parent`, entering
    /// `first` each time `P` comes to it. `P` is declared first.
    pub fn add_sub_mode<S: Mode, P: Mode>(&mut self, parent: P, first: S) -> &mut Self {
        let parent: Rc<dyn Fn(&Engine) -> bool> =
            Rc::new(move |eng: &Engine| eng.mode::<P>() == Some(parent));
        self.declare_mode::<S>(Rule::Sub { parent, first }, None)
    }

    /// Declare mode `C`, worked out by `compute` from the others each frame,
    /// after every mode declared before it changed.
    pub fn add_computed_mode<C: Mode>(&mut self, compute: fn(&Engine) -> Option<C>) -> &mut Self {
        self.declare_mode(Rule::Computed(compute), None)
    }

    /// Run `system` each time mode `M` enters `value`.
    pub fn on_enter<M: Mode>(
        &mut self,
        value: M,
        system: impl FnMut(&Engine, f32) + 'static,
    ) -> &mut Self {
        state::<M>(&self.engine)
            .borrow_mut()
            .enter
            .push((value, Box::new(system)));
        self
    }

    /// Run `system` each time mode `M` leaves `value`.
    pub fn on_exit<M: Mode>(
        &mut self,
        value: M,
        system: impl FnMut(&Engine, f32) + 'static,
    ) -> &mut Self {
        state::<M>(&self.engine)
            .borrow_mut()
            .exit
            .push((value, Box::new(system)));
        self
    }

    fn declare_mode<M: Mode>(&mut self, rule: Rule<M>, next: Option<M>) -> &mut Self {
        assert!(
            self.engine.try_resource::<ModeState<M>>().is_none(),
            "mode {} is declared twice",
            type_name::<M>()
        );
        self.engine.insert_resource(ModeState::<M> {
            current: None,
            next,
            rule,
            enter: Vec::new(),
            exit: Vec::new(),
        });
        if self.engine.try_resource::<ModeKinds>().is_none() {
            self.engine.insert_resource(ModeKinds::default());
        }
        self.engine
            .resource::<ModeKinds>()
            .borrow_mut()
            .0
            .push(change::<M>);
        let label = format!("mode:{}", type_name::<M>());
        self.add_digest_source(&label, |eng, out| {
            let mut hasher = crate::digest::Hasher::new();
            hasher.write_str(&format!("{:?}", eng.mode::<M>()));
            out.push(crate::digest::Entry {
                label: String::from("value"),
                digest: hasher.finish(),
            });
        });
        self.add_snapshot_source(
            &label,
            |eng| {
                let held = state::<M>(eng);
                let held = held.borrow();
                serde_json::json!({ "current": held.current, "next": held.next })
            },
            |eng, value| {
                let read = |key: &str| {
                    value
                        .get(key)
                        .and_then(|held| serde_json::from_value::<Option<M>>(held.clone()).ok())
                        .flatten()
                };
                let held = state::<M>(eng);
                let mut held = held.borrow_mut();
                held.current = read("current");
                held.next = read("next");
            },
        );
        self
    }
}

/// Every declared mode's change, at the start of a frame.
pub(crate) fn change_all(eng: &Engine) {
    let Some(kinds) = eng.try_resource::<ModeKinds>() else {
        return;
    };
    let changes = kinds.borrow().0.clone();
    for change in changes {
        change(eng);
    }
}

/// Mode `M` to its next value, leaving and entering where it moved.
fn change<M: Mode>(eng: &Engine) {
    let held = state::<M>(eng);
    // The rule is asked with nothing borrowed: it reads other modes.
    let (current, asked, rule) = {
        let mut held = held.borrow_mut();
        let rule = match &held.rule {
            Rule::Set => None,
            Rule::Sub { parent, first } => Some(Ok((parent.clone(), *first))),
            Rule::Computed(compute) => Some(Err(*compute)),
        };
        (held.current, held.next.take(), rule)
    };
    let to = match rule {
        None => asked.or(current),
        Some(Ok((parent, first))) => parent(eng).then(|| asked.or(current).unwrap_or(first)),
        Some(Err(compute)) => compute(eng),
    };
    if to == current {
        return;
    }
    if let Some(from) = current {
        run(eng, &held, from, |held| &mut held.exit);
    }
    held.borrow_mut().current = to;
    if let Some(to) = to {
        run(eng, &held, to, |held| &mut held.enter);
    }
}

/// The systems `list` keeps for `value`, each between two steps of the change
/// tick, then their commands. Taken out to run, so one may set a mode.
fn run<M: Mode>(
    eng: &Engine,
    held: &Rc<RefCell<ModeState<M>>>,
    value: M,
    list: fn(&mut ModeState<M>) -> &mut Vec<(M, SystemFn)>,
) {
    let mut systems = std::mem::take(list(&mut held.borrow_mut()));
    for (at, system) in &mut systems {
        if *at == value {
            eng.advance_change_tick();
            system(eng, 0.0);
            eng.advance_change_tick();
        }
    }
    *list(&mut held.borrow_mut()) = systems;
    eng.apply_deferred();
}
