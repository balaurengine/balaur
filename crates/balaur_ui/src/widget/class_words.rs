//! Class words: the screen's seven, which the window or a viewport answers
//! to, and a project's own (`[ui] classes`), which a widget's `classes` or a
//! container's `fit` puts in force for itself and what it lays out. A theme's
//! role and a scene's widget answer a word with a table of its name, as they
//! answer `narrow`.
//!
//! The words in force at a widget are a set, interned to a number: a style
//! resolved under one is kept by it, and a look knows when it moved.
//!
//! A `fit` container takes its list's words in order until what it holds
//! fits on one line along its direction. The step it settled on is kept here
//! by its node, with what each step needed, so a still screen measures once
//! and a wider one steps back at once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::RwLock;

use rustc_hash::FxHashMap;

/// The screen's words, in the order their tables override: the input class,
/// then the height, then the width. A phone held upright answers `touch`,
/// `tall` and `narrow`, in that order.
pub(crate) const SCREEN: [&str; 7] = [
    balaur_core::tags::TOUCH,
    balaur_core::tags::POINTER,
    balaur_core::facts::SHORT,
    balaur_core::facts::TALL,
    balaur_core::facts::NARROW,
    balaur_core::facts::MEDIUM,
    balaur_core::facts::WIDE,
];

/// The words projects declared, in the order they were declared. Process-wide
/// because a scene's keys are checked by a plain function, and kept for the
/// process: a word is a few bytes, and a project declares a handful.
static DECLARED: RwLock<Vec<&'static str>> = RwLock::new(Vec::new());

/// Take a project's own words, after the screen's and any declared before.
pub(crate) fn declare(words: &[String]) {
    let Ok(mut held) = DECLARED.write() else {
        return;
    };
    for word in words {
        if word.is_empty() || SCREEN.contains(&word.as_str()) || held.iter().any(|w| *w == word) {
            continue;
        }
        held.push(Box::leak(word.clone().into_boxed_str()));
    }
}

/// Take the words this run's `[ui] classes` declares, once a change of
/// settings: read where a scene is applied and where a pass begins, since a
/// scene is applied before the first pass reads the rest of `[ui]`.
pub(crate) fn declare_from(eng: &balaur_core::Engine) {
    thread_local! {
        static TAKEN: RefCell<Option<Rc<balaur_core::project::UiSettings>>> = const { RefCell::new(None) };
    }
    let settings = balaur_core::project::UiSettings::of(eng);
    let fresh = TAKEN.with(|taken| {
        let mut taken = taken.borrow_mut();
        let fresh = !taken.as_ref().is_some_and(|had| Rc::ptr_eq(had, &settings));
        *taken = Some(Rc::clone(&settings));
        fresh
    });
    if fresh {
        declare(&settings.class_words);
    }
}

/// The word spelled `text`, where the screen or a project has one.
pub(crate) fn word(text: &str) -> Option<&'static str> {
    if let Some(screen) = SCREEN.iter().find(|w| **w == text) {
        return Some(screen);
    }
    DECLARED.read().ok()?.iter().find(|w| **w == text).copied()
}

/// Every word, the screen's first, in the order their tables override.
pub(crate) fn all() -> Vec<&'static str> {
    let mut out = SCREEN.to_vec();
    if let Ok(held) = DECLARED.read() {
        out.extend(held.iter().copied());
    }
    out
}

/// Where `word` comes in the override order.
fn rank(word: &str) -> usize {
    if let Some(at) = SCREEN.iter().position(|w| *w == word) {
        return at;
    }
    DECLARED
        .read()
        .ok()
        .and_then(|held| held.iter().position(|w| *w == word))
        .map_or(usize::MAX, |at| SCREEN.len() + at)
}

/// A set of words in force, by the number it was interned under. The default
/// is the empty set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Words(u32);

type Interned = (FxHashMap<Rc<[&'static str]>, u32>, Vec<Rc<[&'static str]>>);

thread_local! {
    /// Each set by its words, and each set's words by its number. Number 0 is
    /// the empty set.
    static INTERNED: RefCell<Interned> = RefCell::new({
        let empty: Rc<[&'static str]> = Rc::from([]);
        let mut by_words = FxHashMap::default();
        by_words.insert(Rc::clone(&empty), 0);
        (by_words, vec![empty])
    });
}

impl Words {
    /// The empty set.
    pub(crate) const EMPTY: Self = Self(0);

    /// The set of `words`, in override order, each once.
    pub(crate) fn of(words: impl IntoIterator<Item = &'static str>) -> Self {
        let mut list: Vec<&'static str> = words.into_iter().collect();
        if list.is_empty() {
            return Self::default();
        }
        list.sort_by_key(|w| rank(w));
        list.dedup();
        INTERNED.with(|held| {
            let mut held = held.borrow_mut();
            if let Some(number) = held.0.get(list.as_slice()) {
                return Self(*number);
            }
            let words: Rc<[&'static str]> = Rc::from(list);
            let number = u32::try_from(held.1.len()).unwrap_or(u32::MAX);
            held.1.push(Rc::clone(&words));
            held.0.insert(words, number);
            Self(number)
        })
    }

    /// Its words, in override order.
    pub(crate) fn list(self) -> Rc<[&'static str]> {
        INTERNED.with(|held| {
            held.borrow()
                .1
                .get(self.0 as usize)
                .cloned()
                .unwrap_or_else(|| Rc::from([]))
        })
    }

    /// This set and `more` together.
    pub(crate) fn with(self, more: impl IntoIterator<Item = &'static str>) -> Self {
        let mut more = more.into_iter().peekable();
        if more.peek().is_none() {
            return self;
        }
        Self::of(self.list().iter().copied().chain(more))
    }
}

/// The words `named` spells, in its order; a word no project declared is
/// said once and left out.
pub(crate) fn words_of(named: &[smol_str::SmolStr]) -> Vec<&'static str> {
    named
        .iter()
        .filter_map(|text| {
            let found = word(text);
            if found.is_none() {
                unknown(text);
            }
            found
        })
        .collect()
}

fn unknown(text: &str) {
    thread_local! {
        static SAID: RefCell<rustc_hash::FxHashSet<String>> = RefCell::default();
    }
    SAID.with(|said| {
        if said.borrow_mut().insert(text.to_owned()) {
            tracing::warn!(
                "widget: `{text}` is no class word: the screen's, or one `[ui] classes` declares"
            );
        }
    });
}

/// What a line may run past its room and still fit: a layout's sums leave a
/// box a hair short of what it was measured for.
const SLACK: f32 = 0.5;

/// One fit container's step: how many of its words are in force, and what
/// each step needed where it was measured.
struct Fit {
    list: Vec<&'static str>,
    step: usize,
    needs: Vec<Option<f32>>,
}

thread_local! {
    static FITS: RefCell<FxHashMap<u64, Fit>> = RefCell::default();
    /// Moves whenever a step does, so the arena is built again under it.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// The words a fit container has in force, by its node's bits.
pub(crate) fn fit_words(bits: u64) -> Vec<&'static str> {
    FITS.with(|fits| {
        fits.borrow()
            .get(&bits)
            // copy: a word is a pointer to text kept for the process.
            .map(|fit| fit.list[..fit.step].to_vec())
            .unwrap_or_default()
    })
}

/// The number that moves when any container's step does.
pub(crate) fn generation() -> u64 {
    GENERATION.get()
}

/// Settle the container at `bits` on the step its `room` holds, now that it
/// needs `need` at the step in force. `true` when the step moved, which owes
/// the pass another go.
///
/// A step is the first `step` words of `list`. It climbs while what is in
/// force does not fit, and goes back while the step before fitted when it
/// was measured. A need that changed at the step in force says what is held
/// changed, so every other step is measured again on the way.
pub(crate) fn settle(bits: u64, list: &[&'static str], room: f32, need: f32) -> bool {
    FITS.with(|fits| {
        let mut fits = fits.borrow_mut();
        let fit = fits.entry(bits).or_insert_with(|| Fit {
            list: list.to_vec(),
            step: 0,
            needs: vec![None; list.len() + 1],
        });
        if fit.list != list {
            *fit = Fit {
                list: list.to_vec(),
                step: 0,
                needs: vec![None; list.len() + 1],
            };
        }
        let before = fit.step;
        if fit.needs[before].is_some_and(|had| (had - need).abs() > SLACK) {
            fit.needs.fill(None);
        }
        fit.needs[before] = Some(need);
        let mut step = before;
        if need > room + SLACK {
            // Up, past every step already known not to fit.
            step += 1;
            while step < list.len() && fit.needs[step].is_some_and(|n| n > room + SLACK) {
                step += 1;
            }
            step = step.min(list.len());
        } else {
            // Back, while the step before fitted; one not measured is tried.
            while step > 0 {
                match fit.needs[step - 1] {
                    Some(n) if n <= room + SLACK => step -= 1,
                    None => {
                        step -= 1;
                        break;
                    }
                    Some(_) => break,
                }
            }
        }
        fit.step = step;
        let moved = step != before;
        if moved {
            GENERATION.set(GENERATION.get().wrapping_add(1));
        }
        moved
    })
}

/// Forget the container at `bits`, gone or no longer fitting.
pub(crate) fn forget(bits: u64) {
    let had = FITS.with(|fits| fits.borrow_mut().remove(&bits));
    if had.is_some_and(|fit| fit.step > 0) {
        GENERATION.set(GENERATION.get().wrapping_add(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_is_its_words_in_override_order_each_once() {
        let set = Words::of([
            balaur_core::facts::WIDE,
            balaur_core::tags::TOUCH,
            balaur_core::facts::WIDE,
        ]);
        assert_eq!(
            &*set.list(),
            &[balaur_core::tags::TOUCH, balaur_core::facts::WIDE]
        );
        assert_eq!(
            set,
            Words::of([balaur_core::tags::TOUCH, balaur_core::facts::WIDE])
        );
        assert_eq!(Words::of([]), Words::default());
    }

    #[test]
    fn a_declared_word_comes_after_the_screen_s() {
        declare(&["zz_first".to_owned(), "zz_second".to_owned()]);
        let first = word("zz_first").expect("declared");
        let second = word("zz_second").expect("declared");
        let set = Words::of([second, balaur_core::facts::NARROW, first]);
        assert_eq!(&*set.list(), &[balaur_core::facts::NARROW, first, second]);
        assert!(word("zz_never").is_none());
    }

    #[test]
    fn a_step_climbs_while_it_overflows_and_comes_back_where_it_fitted() {
        let bits = 0xF1F0_0001;
        let list: Vec<&'static str> = vec!["one", "two", "three"];
        // Needs 300 at no step, 250 with one word, 180 with two.
        assert!(settle(bits, &list, 200.0, 300.0));
        assert_eq!(fit_words(bits), ["one"]);
        assert!(settle(bits, &list, 200.0, 250.0));
        assert_eq!(fit_words(bits), ["one", "two"]);
        assert!(!settle(bits, &list, 200.0, 180.0), "it fits");
        // Wider: every step known, so straight back to none.
        assert!(settle(bits, &list, 320.0, 180.0));
        assert!(fit_words(bits).is_empty());
        // Narrower again: past the steps known not to fit at once.
        assert!(settle(bits, &list, 200.0, 300.0));
        assert_eq!(fit_words(bits), ["one", "two"]);
        forget(bits);
    }

    #[test]
    fn a_need_that_moved_measures_the_steps_again() {
        let bits = 0xF1F0_0002;
        let list: Vec<&'static str> = vec!["one", "two"];
        assert!(settle(bits, &list, 100.0, 150.0));
        assert!(!settle(bits, &list, 100.0, 90.0), "one word fits");
        // What it holds grew shorter: the step before is tried again.
        assert!(settle(bits, &list, 100.0, 60.0));
        assert!(fit_words(bits).is_empty());
        forget(bits);
    }
}
