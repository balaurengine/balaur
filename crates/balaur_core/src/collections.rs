//! Deterministic collections for order-sensitive engine state.
//!
//! Plain `HashMap`/`HashSet` iteration order is unspecified and varies
//! between runs, which silently breaks reproducibility the moment anyone
//! iterates one in a simulation-affecting path. Engine and plugin state
//! whose iteration can influence the simulation must use these aliases:
//! `IndexMap` iterates in insertion order and the FxHasher build hasher is
//! unseeded, so behavior is identical on every run and platform (this is the
//! same recipe parry uses under its `enhanced-determinism` feature).

use core::hash::BuildHasherDefault;
use rustc_hash::FxHasher;

pub type DetHashMap<K, V> = indexmap::IndexMap<K, V, BuildHasherDefault<FxHasher>>;
pub type DetHashSet<K> = indexmap::IndexSet<K, BuildHasherDefault<FxHasher>>;
