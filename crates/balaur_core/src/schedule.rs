//! The order systems run in within a stage (PLAN-ecs §2, Bevy's schedules).
//!
//! A system may carry a name, the sets it belongs to, what it runs before
//! and after (a system's name or a set's) and conditions it runs on. A stage
//! sorts its systems once, the first time it runs after one was added, and
//! keeps the order: the sort takes the earliest-added system free to run each
//! time, so a stage nobody orders runs as it always did, and systems with no
//! order between them keep the order they were added wherever what was asked
//! allows. A cycle is an
//! error naming the systems in it; an order naming nothing is a warning. The
//! eight stages stay the frame's spine.

use std::collections::{BTreeSet, HashMap};

use crate::app::SystemFn;
use crate::engine::Engine;

/// A condition a system or a set runs on, asked each time the stage runs.
pub type ConditionFn = Box<dyn FnMut(&Engine) -> bool>;

/// One system, and what orders it.
pub struct System {
    name: String,
    run: SystemFn,
    sets: Vec<String>,
    before: Vec<String>,
    after: Vec<String>,
    conditions: Vec<ConditionFn>,
}

impl System {
    /// A system named `name`, which other systems order themselves by.
    pub fn new(name: &str, run: impl FnMut(&Engine, f32) + 'static) -> Self {
        Self::boxed(name.to_owned(), Box::new(run))
    }

    /// A typed system named `name`: a function of
    /// [`SystemParam`](crate::system_param::SystemParam)s.
    pub fn typed<M>(name: &str, run: impl crate::system_param::TypedSystem<M>) -> Self {
        Self::boxed(name.to_owned(), run.into_run(name.to_owned()))
    }

    pub(crate) fn boxed(name: String, run: SystemFn) -> Self {
        Self {
            name,
            run,
            sets: Vec::new(),
            before: Vec::new(),
            after: Vec::new(),
            conditions: Vec::new(),
        }
    }

    /// One of the systems of set `set`.
    #[must_use]
    pub fn in_set(mut self, set: &str) -> Self {
        self.sets.push(set.to_owned());
        self
    }

    /// Runs before the system or set named `label`.
    #[must_use]
    pub fn before(mut self, label: &str) -> Self {
        self.before.push(label.to_owned());
        self
    }

    /// Runs after the system or set named `label`.
    #[must_use]
    pub fn after(mut self, label: &str) -> Self {
        self.after.push(label.to_owned());
        self
    }

    /// Runs only on a stage run where `condition` says yes.
    #[must_use]
    pub fn run_if(mut self, condition: impl FnMut(&Engine) -> bool + 'static) -> Self {
        self.conditions.push(Box::new(condition));
        self
    }
}

/// What orders every system of a set.
#[derive(Default)]
pub struct Set {
    before: Vec<String>,
    after: Vec<String>,
    conditions: Vec<ConditionFn>,
}

impl Set {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every system of the set runs before the system or set `label`.
    #[must_use]
    pub fn before(mut self, label: &str) -> Self {
        self.before.push(label.to_owned());
        self
    }

    /// Every system of the set runs after the system or set `label`.
    #[must_use]
    pub fn after(mut self, label: &str) -> Self {
        self.after.push(label.to_owned());
        self
    }

    /// No system of the set runs on a stage run where `condition` says no;
    /// asked once a stage run.
    #[must_use]
    pub fn run_if(mut self, condition: impl FnMut(&Engine) -> bool + 'static) -> Self {
        self.conditions.push(Box::new(condition));
        self
    }
}

/// One stage's systems, its sets, and the order they sorted to.
#[derive(Default)]
pub(crate) struct Schedule {
    systems: Vec<System>,
    sets: HashMap<String, Set>,
    order: Option<Vec<usize>>,
}

impl Schedule {
    pub(crate) fn add(&mut self, system: System) {
        self.systems.push(system);
        self.order = None;
    }

    /// Set `name`'s order and conditions, added to what it had.
    pub(crate) fn configure_set(&mut self, name: &str, set: Set) {
        let held = self.sets.entry(name.to_owned()).or_default();
        held.before.extend(set.before);
        held.after.extend(set.after);
        held.conditions.extend(set.conditions);
        self.order = None;
    }

    pub(crate) fn len(&self) -> usize {
        self.systems.len()
    }

    /// The order the systems run in, sorted now if a system was added since.
    ///
    /// # Errors
    /// The systems of a cycle, named in order.
    pub(crate) fn order(&mut self) -> Result<&[usize], String> {
        if self.order.is_none() {
            self.order = Some(self.sort()?);
        }
        Ok(self.order.as_deref().unwrap_or_default())
    }

    /// Run every system in order, each between two steps of the change tick
    /// and only where its own conditions and its sets' say yes.
    ///
    /// # Panics
    /// On a cycle: the app was put together wrong.
    pub(crate) fn run(&mut self, stage: &str, eng: &Engine, dt: f32) {
        if let Err(why) = self.order() {
            panic!("the {stage} stage cannot be ordered: {why}");
        }
        // Taken for the run and put back, so a frame copies nothing.
        let order = self.order.take().unwrap_or_default();
        let mut sets_ran: HashMap<String, bool> = HashMap::new();
        for &at in &order {
            let system = &mut self.systems[at];
            let mut runs = true;
            for set in &system.sets {
                let on = *sets_ran.entry(set.clone()).or_insert_with(|| {
                    self.sets.get_mut(set).is_none_or(|held| {
                        held.conditions.iter_mut().all(|condition| condition(eng))
                    })
                });
                runs &= on;
            }
            if !runs || !system.conditions.iter_mut().all(|condition| condition(eng)) {
                continue;
            }
            eng.advance_change_tick();
            (system.run)(eng, dt);
            eng.advance_change_tick();
        }
        self.order = Some(order);
    }

    /// The systems a label names: the one with that name, or every system in
    /// the set of that name.
    fn named(&self, label: &str) -> Vec<usize> {
        self.systems
            .iter()
            .enumerate()
            .filter(|(_, system)| {
                system.name == label || system.sets.iter().any(|set| set == label)
            })
            .map(|(at, _)| at)
            .collect()
    }

    /// Kahn's sort, always taking the earliest-added system that is free, so
    /// an order nobody gave stays the order of adding. A label naming no
    /// system or set orders nothing, and is said: it may be an optional
    /// plugin's, or a typo.
    fn sort(&self) -> Result<Vec<usize>, String> {
        let count = self.systems.len();
        let mut after: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); count];
        let mut link = |sources: &[usize], label: &str, before: bool| {
            let targets = self.named(label);
            if targets.is_empty() && !self.sets.contains_key(label) {
                tracing::warn!(
                    label,
                    "a system is ordered against a name no system or set has"
                );
            }
            for &source in sources {
                for &target in &targets {
                    let (from, to) = if before {
                        (source, target)
                    } else {
                        (target, source)
                    };
                    if from != to {
                        after[from].insert(to);
                    }
                }
            }
        };
        for (at, system) in self.systems.iter().enumerate() {
            for label in &system.before {
                link(&[at], label, true);
            }
            for label in &system.after {
                link(&[at], label, false);
            }
        }
        for (name, set) in &self.sets {
            let members = self.named(name);
            for label in &set.before {
                link(&members, label, true);
            }
            for label in &set.after {
                link(&members, label, false);
            }
        }
        let mut waiting = vec![0usize; count];
        for next in &after {
            for &to in next {
                waiting[to] += 1;
            }
        }
        let mut free: BTreeSet<usize> = (0..count).filter(|&at| waiting[at] == 0).collect();
        let mut order = Vec::with_capacity(count);
        while let Some(at) = free.pop_first() {
            order.push(at);
            for &to in &after[at] {
                waiting[to] -= 1;
                if waiting[to] == 0 {
                    free.insert(to);
                }
            }
        }
        if order.len() == count {
            return Ok(order);
        }
        Err(self.cycle(&after, &order))
    }

    /// The names round one cycle among the systems the sort left out. Each of
    /// those waits on another left out, so walking back from any of them
    /// comes round to one already passed, which closes a cycle.
    fn cycle(&self, after: &[BTreeSet<usize>], sorted: &[usize]) -> String {
        let left: BTreeSet<usize> = (0..after.len()).filter(|at| !sorted.contains(at)).collect();
        let waits_on = |at: usize| left.iter().copied().find(|&from| after[from].contains(&at));
        let Some(&start) = left.first() else {
            return "a cycle".to_owned();
        };
        let mut path = vec![start];
        let mut at = start;
        while let Some(back) = waits_on(at) {
            if let Some(from) = path.iter().position(|&seen| seen == back) {
                // Each step back ran before the one it was found from, so
                // forwards it is `back`, the walk reversed, then `back` again.
                let round: Vec<&str> = std::iter::once(back)
                    .chain(path[from + 1..].iter().rev().copied())
                    .chain(std::iter::once(back))
                    .map(|at| self.label(at))
                    .collect();
                return format!("a cycle: {}", round.join(" -> "));
            }
            path.push(back);
            at = back;
        }
        "a cycle".to_owned()
    }

    fn label(&self, at: usize) -> &str {
        &self.systems[at].name
    }
}
