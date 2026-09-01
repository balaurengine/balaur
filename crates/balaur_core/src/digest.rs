//! The simulation digest: one number per tick, so divergence is findable.
//!
//! Determinism is a claim until two runs are compared, and comparing whole
//! worlds is impractical. [`entries`] hashes the simulation in labelled
//! slices, [`digest`] folds them to 64 bits, and [`first_divergence`] names
//! the slice two runs disagree on. That is what turns "the replay desynced"
//! into "tick 4213, `n_ball_7/body2d`".

use std::fmt;

use hecs::Entity;

use crate::components::{ComponentRegistry, StableId};
use crate::engine::Engine;
use crate::scene::{collect_subtree, Name, Transform};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a over the canonical byte form of simulation state.
///
/// Integer-only, so the fold is identical on every platform. Floats enter as
/// `to_bits`: a digest that compared them numerically would call two runs
/// equal when they had already drifted by an ulp.
#[derive(Clone, Copy)]
pub struct Hasher(u64);

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher {
    pub const fn new() -> Self {
        Self(FNV_OFFSET)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(FNV_PRIME);
        }
    }

    pub fn write_u64(&mut self, v: u64) {
        self.write(&v.to_le_bytes());
    }

    pub fn write_f32(&mut self, v: f32) {
        self.write(&v.to_bits().to_le_bytes());
    }

    pub fn write_f64(&mut self, v: f64) {
        self.write(&v.to_bits().to_le_bytes());
    }

    /// Terminated, so `("ab", "c")` and `("a", "bc")` do not collide.
    pub fn write_str(&mut self, s: &str) {
        self.write(s.as_bytes());
        self.write(&[0]);
    }

    pub const fn finish(self) -> Digest {
        Digest(self.0)
    }
}

/// A hash of simulation state: one tick's, or one slice of one tick's.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Digest(pub u64);

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

/// One labelled slice of simulation state.
///
/// The label is a node's stable id where it has one, so it survives the
/// rename and reparent that would move a path — the whole reason a
/// divergence report can name the same node on two machines.
pub struct Entry {
    pub label: String,
    pub digest: Digest,
}

/// Extra state a plugin folds in, in registration order.
pub type DigestFn = Box<dyn Fn(&Engine, &mut Vec<Entry>)>;

/// What plugins contribute beyond components.
///
/// A component's `get` reports what a scene author set, not everything a
/// step computes: `body` reports its kind and nothing about velocity, so
/// physics contributes that here.
#[derive(Default)]
pub struct DigestRegistry(pub Vec<(String, DigestFn)>);

/// Hash the simulation in labelled slices, in scene-tree order.
///
/// Tree order rather than sorted-by-id: two peers that agree have the same
/// tree, and a reparent is itself simulation state worth catching.
pub fn entries(eng: &Engine) -> Vec<Entry> {
    let mut out = Vec::new();
    let nodes: Vec<(Entity, String, Option<Transform>)> = {
        let world = eng.world();
        collect_subtree(&world, eng.root())
            .into_iter()
            .map(|e| {
                let transform = world.get::<&Transform>(e).ok().map(|t| *t);
                (e, node_label(&world, e), transform)
            })
            .collect()
    };

    for (entity, label, transform) in &nodes {
        if let Some(t) = transform {
            let mut h = Hasher::new();
            for v in [
                t.position.x,
                t.position.y,
                t.position.z,
                t.rotation.x,
                t.rotation.y,
                t.rotation.z,
                t.rotation.w,
                t.scale.x,
                t.scale.y,
                t.scale.z,
            ] {
                h.write_f32(v);
            }
            out.push(Entry {
                label: format!("{label}/transform"),
                digest: h.finish(),
            });
        }
        push_components(eng, *entity, label, &mut out);
    }

    if let Some(sources) = eng.try_resource::<DigestRegistry>() {
        let sources = sources.borrow();
        for (name, source) in &sources.0 {
            let start = out.len();
            source(eng, &mut out);
            for entry in &mut out[start..] {
                entry.label = format!("{name}/{}", entry.label);
            }
        }
    }

    if let Some(rng) = eng.try_resource::<crate::rng::RngState>() {
        let mut h = Hasher::new();
        h.write_u64(rng.borrow().0.state());
        out.push(Entry {
            label: String::from("rng"),
            digest: h.finish(),
        });
    }
    out
}

/// The whole tick, folded to one number.
pub fn digest(eng: &Engine) -> Digest {
    fold(&entries(eng))
}

/// Order-sensitive fold, so a reordered tree is a divergence too.
pub fn fold(entries: &[Entry]) -> Digest {
    let mut h = Hasher::new();
    for entry in entries {
        h.write_str(&entry.label);
        h.write_u64(entry.digest.0);
    }
    h.finish()
}

/// The first slice two runs disagree on, described for a human.
///
/// A slice present on one side only is reported as such: an entity that
/// exists on one peer and not the other is the most common desync there is.
pub fn first_divergence(a: &[Entry], b: &[Entry]) -> Option<String> {
    for (left, right) in a.iter().zip(b) {
        if left.label != right.label {
            return Some(format!(
                "{} vs {}: the trees differ in shape",
                left.label, right.label
            ));
        }
        if left.digest != right.digest {
            return Some(format!(
                "{}: {} vs {}",
                left.label, left.digest, right.digest
            ));
        }
    }
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Greater => Some(format!("{}: missing on the right", a[b.len()].label)),
        std::cmp::Ordering::Less => Some(format!("{}: missing on the left", b[a.len()].label)),
        std::cmp::Ordering::Equal => None,
    }
}

fn push_components(eng: &Engine, entity: Entity, label: &str, out: &mut Vec<Entry>) {
    let Some(registry) = eng.try_resource::<ComponentRegistry>() else {
        return;
    };
    let registry = registry.borrow();
    for (name, def) in &registry.0 {
        let Some(value) = (def.get)(eng, entity) else {
            continue;
        };
        let mut h = Hasher::new();
        hash_value(&mut h, &value);
        out.push(Entry {
            label: format!("{label}/{name}"),
            digest: h.finish(),
        });
    }
}

/// How a node is named in a divergence report: its stable id where it has
/// one, so the two peers agree on the name even after a reparent.
pub fn node_label(world: &hecs::World, entity: Entity) -> String {
    if let Ok(id) = world.get::<&StableId>(entity) {
        return id.0.clone();
    }
    let name = world
        .get::<&Name>(entity)
        .map_or_else(|_| String::from("node"), |n| n.0.clone());
    format!("{name}#{}", entity.id())
}

fn hash_value(h: &mut Hasher, value: &toml::Value) {
    match value {
        toml::Value::String(s) => {
            h.write(b"s");
            h.write_str(s);
        }
        toml::Value::Integer(i) => {
            h.write(b"i");
            h.write_u64(*i as u64);
        }
        toml::Value::Float(f) => {
            h.write(b"f");
            h.write_f64(*f);
        }
        toml::Value::Boolean(b) => {
            h.write(b"b");
            h.write(&[u8::from(*b)]);
        }
        toml::Value::Datetime(d) => {
            h.write(b"d");
            h.write_str(&d.to_string());
        }
        toml::Value::Array(a) => {
            h.write(b"a");
            for v in a {
                hash_value(h, v);
            }
        }
        toml::Value::Table(t) => {
            h.write(b"t");
            let mut keys: Vec<&String> = t.keys().collect();
            keys.sort_unstable();
            for k in keys {
                h.write_str(k);
                hash_value(h, &t[k]);
            }
        }
    }
}
