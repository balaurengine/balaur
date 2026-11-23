//! What a large component costs a node, kept three ways: inline, behind an
//! `Arc` copied on write, and as a handle into a shared table of the distinct
//! values a screen uses (a style a role names), with a small override the node
//! holds only when it sets one. Each at a component's size from a transform's
//! to a widget's, timed building a screen (the node, the component, three more
//! after it: an archetype move each), cloning every value as the widget arena
//! does, reading one field from each as a frame does, writing one node in ten
//! while the arena holds its clones, and freeing it.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use hecs::{Entity, World};

const NODES: usize = 2000;
/// The distinct values a screen's nodes share, as its roles and classes are.
const DISTINCT: usize = 16;

/// What every node carries before its component: about the engine's name,
/// transforms, appearance, children and name index.
#[derive(Clone)]
#[allow(
    dead_code,
    reason = "the bytes a node carries, which nothing here reads"
)]
struct Base([u64; 36]);

/// A component added after the large one, as `widget_layout` and the rest are.
#[derive(Clone)]
#[allow(
    dead_code,
    reason = "the bytes a node carries, which nothing here reads"
)]
struct Extra<const I: usize>([u64; 16]);

/// A component of about `8 * W + 16` bytes: words, and a shared name, as a
/// widget's text is.
#[derive(Clone)]
struct Payload<const W: usize> {
    words: [u64; W],
    name: Arc<str>,
}

impl<const W: usize> Payload<W> {
    fn nth(at: usize) -> Self {
        Self {
            words: [at as u64; W],
            name: Arc::from(format!("value {at}").as_str()),
        }
    }
}

/// A node's component kept behind an `Arc`, written on copy.
#[derive(Clone)]
struct Shared<const W: usize>(Arc<Payload<W>>);

/// A node's handle into the shared table, and what it sets over its entry.
#[derive(Clone)]
struct Styled {
    style: u32,
    over: Option<Box<Override>>,
}

/// The one field a node set itself.
#[derive(Clone)]
struct Override {
    first: u64,
}

/// One way of keeping the component, over a shared table it may use.
trait Kept<const W: usize> {
    type Held: hecs::Component + Clone;
    const NAME: &'static str;
    fn make(table: &[Arc<Payload<W>>], at: usize) -> Self::Held;
    fn read(table: &[Arc<Payload<W>>], held: &Self::Held) -> u64;
    fn write(held: &mut Self::Held);
}

struct Inline;
impl<const W: usize> Kept<W> for Inline {
    type Held = Payload<W>;
    const NAME: &'static str = "inline";
    fn make(_: &[Arc<Payload<W>>], at: usize) -> Self::Held {
        Payload::nth(at % DISTINCT)
    }
    fn read(_: &[Arc<Payload<W>>], held: &Self::Held) -> u64 {
        held.words[0] + held.name.len() as u64
    }
    fn write(held: &mut Self::Held) {
        held.words[0] += 1;
    }
}

struct ArcKept;
impl<const W: usize> Kept<W> for ArcKept {
    type Held = Shared<W>;
    const NAME: &'static str = "arc";
    fn make(_: &[Arc<Payload<W>>], at: usize) -> Self::Held {
        Shared(Arc::new(Payload::nth(at % DISTINCT)))
    }
    fn read(_: &[Arc<Payload<W>>], held: &Self::Held) -> u64 {
        held.0.words[0] + held.0.name.len() as u64
    }
    fn write(held: &mut Self::Held) {
        Arc::make_mut(&mut held.0).words[0] += 1;
    }
}

struct Resource;
impl<const W: usize> Kept<W> for Resource {
    type Held = Styled;
    const NAME: &'static str = "resource";
    fn make(_: &[Arc<Payload<W>>], at: usize) -> Self::Held {
        Styled {
            style: (at % DISTINCT) as u32,
            over: None,
        }
    }
    fn read(table: &[Arc<Payload<W>>], held: &Self::Held) -> u64 {
        held.over
            .as_ref()
            .map_or_else(|| table[held.style as usize].words[0], |over| over.first)
    }
    fn write(held: &mut Self::Held) {
        let first = held.over.as_ref().map_or(0, |over| over.first) + 1;
        held.over = Some(Box::new(Override { first }));
    }
}

fn table<const W: usize>() -> Vec<Arc<Payload<W>>> {
    (0..DISTINCT).map(|at| Arc::new(Payload::nth(at))).collect()
}

/// A screen: each node spawned, given the component, then three more.
fn build<const W: usize, K: Kept<W>>(table: &[Arc<Payload<W>>]) -> (World, Vec<Entity>) {
    let mut world = World::new();
    let mut nodes = Vec::with_capacity(NODES);
    for at in 0..NODES {
        let node = world.spawn((Base([at as u64; 36]),));
        world.insert_one(node, K::make(table, at)).unwrap();
        world.insert_one(node, Extra::<0>([0; 16])).unwrap();
        world.insert_one(node, Extra::<1>([0; 16])).unwrap();
        world.insert_one(node, Extra::<2>([0; 16])).unwrap();
        nodes.push(node);
    }
    (world, nodes)
}

fn clone_all<const W: usize, K: Kept<W>>(world: &World) -> Vec<K::Held> {
    world.query::<&K::Held>().iter().cloned().collect()
}

fn one_way<const W: usize, K: Kept<W>>(c: &mut Criterion, size: &str) {
    let table = table::<W>();
    let mut group = c.benchmark_group(format!("storage/{size}"));
    group.sample_size(20);
    group.throughput(Throughput::Elements(NODES as u64));
    group.bench_function(BenchmarkId::new("build", K::NAME), |b| {
        b.iter_batched(|| (), |()| build::<W, K>(&table), BatchSize::PerIteration);
    });
    group.bench_function(BenchmarkId::new("free", K::NAME), |b| {
        b.iter_batched(
            || build::<W, K>(&table),
            |(mut world, nodes)| {
                for node in nodes {
                    world.despawn(node).unwrap();
                }
                world
            },
            BatchSize::PerIteration,
        );
    });
    let (world, nodes) = build::<W, K>(&table);
    group.bench_function(BenchmarkId::new("clone", K::NAME), |b| {
        b.iter(|| black_box(clone_all::<W, K>(&world)));
    });
    group.bench_function(BenchmarkId::new("read", K::NAME), |b| {
        b.iter(|| {
            world
                .query::<&K::Held>()
                .iter()
                .map(|held| K::read(&table, held))
                .sum::<u64>()
        });
    });
    group.bench_function(BenchmarkId::new("write_tenth", K::NAME), |b| {
        b.iter_batched(
            || clone_all::<W, K>(&world),
            |arena| {
                for node in nodes.iter().step_by(10) {
                    K::write(&mut world.get::<&mut K::Held>(*node).unwrap());
                }
                arena
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn every_way<const W: usize>(c: &mut Criterion, size: &str) {
    one_way::<W, Inline>(c, size);
    one_way::<W, ArcKept>(c, size);
    one_way::<W, Resource>(c, size);
}

/// A transform's size, the limit, a widget's properties', a widget's.
fn storage(c: &mut Criterion) {
    every_way::<4>(c, "48B");
    every_way::<30>(c, "256B");
    every_way::<91>(c, "744B");
    every_way::<228>(c, "1840B");
}

criterion_group!(benches, storage);
criterion_main!(benches);
