//! Where a frame's time goes outside scripting: the scene tree, the seam's
//! own conversions, and physics.

use std::time::{Duration, Instant};

use balaur_bench::{Backend, Project, app};
use balaur_core::scene;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

const EMPTY: &str = "local S = {}\nfunction S:init() end\nreturn S\n";

/// One looping track on the node itself, so the sampler runs and the write
/// lands without a rig to resolve first.
const CLIP: &str = r#"
type = "animation_clip"

[clips.spin]
length = 2.0
loop = "loop"

[[clips.spin.tracks]]
interp = "linear"
property = "position"
target = ""

[[clips.spin.tracks.keys]]
t = 0.0
value = [0.0, 0.0, 0.0]

[[clips.spin.tracks.keys]]
t = 2.0
value = [1.0, 0.0, 0.0]
"#;

/// Transform propagation over a tree, which runs every frame for every node.
fn propagate(c: &mut Criterion) {
    let mut group = c.benchmark_group("propagate_transforms");
    {
        let count = 1000usize;
        let project = Project::new(Backend::Rune, EMPTY).unwrap();
        let app = app(Backend::Rune, &project).unwrap();
        let root = app.engine.root();
        {
            let mut world = app.engine.world_mut();
            // A chain, not a fan: depth is what propagation actually walks.
            let mut parent = root;
            for i in 0..count {
                parent = scene::spawn_node(&mut world, &format!("n{i}"), parent);
            }
        }
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| scene::propagate_transforms(&mut app.engine.world_mut(), root));
        });
    }
    group.finish();
}

/// Spawning nodes, which a game does whenever it instantiates anything.
#[allow(
    clippy::disallowed_methods,
    reason = "this is the measurement, not simulation"
)]
fn spawn(c: &mut Criterion) {
    let mut group = c.benchmark_group("spawn_nodes");
    {
        let count = 1000usize;
        let project = Project::new(Backend::Rune, EMPTY).unwrap();
        let app = app(Backend::Rune, &project).unwrap();
        let root = app.engine.root();
        group.throughput(Throughput::Elements(count as u64));
        // Freed off the clock, as node_ops_rust does: a world that grows by
        // `count` every iteration times the world's size, not the spawn.
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let start = Instant::now();
                    let made: Vec<_> = {
                        let mut world = app.engine.world_mut();
                        (0..count)
                            .map(|i| scene::spawn_node(&mut world, &format!("n{i}"), root))
                            .collect()
                    };
                    total += start.elapsed();
                    scene::free_nodes(&app.engine, &made);
                }
                total
            });
        });
    }
    group.finish();
}

/// Instantiating a scene document: parsing plus spawning, which is what
/// loading a level costs.
#[allow(
    clippy::disallowed_methods,
    reason = "this is the measurement, not simulation"
)]
fn instantiate_scene(c: &mut Criterion) {
    let mut group = c.benchmark_group("instantiate_scene");
    {
        let count = 500usize;
        let mut doc = String::new();
        for i in 0..count {
            use std::fmt::Write as _;
            // One root with the rest under it, as a scene has: a file of N
            // roots is refused, and a level is a tree anyway.
            let parent = if i == 0 {
                String::new()
            } else {
                "parent = \"n0\"\n".into()
            };
            let _ = write!(
                doc,
                "[[nodes]]\nid = \"n{i}\"\nname = \"N{i}\"\n{parent}\
                 transform = {{ position = [1.0, 2.0, 3.0] }}\n\n"
            );
        }
        let project = Project::new(Backend::Rune, EMPTY).unwrap();
        let app = app(Backend::Rune, &project).unwrap();
        let root = app.engine.root();
        group.throughput(Throughput::Elements(count as u64));
        // Into a holder that is freed off the clock, so each iteration loads
        // the level into an empty world rather than onto the last one.
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let holder = scene::spawn_node(&mut app.engine.world_mut(), "holder", root);
                    let start = Instant::now();
                    balaur_core::project::instantiate_scene(&app.engine, &doc, holder, false)
                        .unwrap();
                    total += start.elapsed();
                    scene::free_nodes(&app.engine, &[holder]);
                }
                total
            });
        });
    }
    group.finish();
}

/// Parsing the scene document on its own.
///
/// `instantiate_scene` parses and then spawns. Without the split it is not
/// clear whether to optimise the engine or accept the TOML crate's cost.
fn parse_scene(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_scene_only");
    {
        let count = 500usize;
        let mut doc = String::new();
        for i in 0..count {
            use std::fmt::Write as _;
            let _ = write!(
                doc,
                "[[nodes]]\nid = \"n{i}\"\nname = \"N{i}\"\ntransform = {{ position = [1.0, 2.0, 3.0] }}\n\n"
            );
        }
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| toml::from_str::<toml::Value>(&doc).unwrap());
        });
    }
    group.finish();
}

/// A physics step at a few body counts, in both dimensions: a 2D game is the
/// common case and steps a different solver.
fn physics_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("physics_step");
    let count = 1000usize;
    for body in ["body2d", "body3d"] {
        {
            let project = Project::new(Backend::Rune, EMPTY).unwrap();
            let mut app = app(Backend::Rune, &project).unwrap();
            let root = app.engine.root();
            for i in 0..count {
                let e = scene::spawn_node(&mut app.engine.world_mut(), &format!("b{i}"), root);
                let params = toml::toml! { kind = "dynamic" };
                let _ = balaur_core::components::add(&app.engine, e, body, Some(&params.into()));
            }
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::new(body, count), &count, |b, _| {
                b.iter(|| app.tick(1.0 / 60.0));
            });
        }
    }
    group.finish();
}

/// Sampling clips onto nodes, which every animated scene pays every frame.
fn animation_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("animation_step");
    {
        let count = 1000usize;
        let project = Project::new(Backend::Rune, EMPTY).unwrap();
        std::fs::create_dir_all(project.path().join("animations")).unwrap();
        std::fs::write(project.path().join("animations/bench.toml"), CLIP).unwrap();
        let mut app = app(Backend::Rune, &project).unwrap();
        let root = app.engine.root();
        for i in 0..count {
            let e = scene::spawn_node(&mut app.engine.world_mut(), &format!("a{i}"), root);
            let params = toml::toml! {
                autoplay = "spin"
                library = "animations/bench.toml"
            };
            let _ = balaur_core::components::add(&app.engine, e, "animation", Some(&params.into()));
        }
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| app.tick(1.0 / 60.0));
        });
    }
    group.finish();
}

/// One event against a crowd of subscribers. `emit` only queues: the pump
/// that reaches the listeners runs inside the tick, so the tick is what is
/// timed, and `node_ops_rust/tick_empty` is what it is worth reading against.
fn event_delivery(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_delivery");
    {
        let count = 1000usize;
        let project = Project::new(Backend::Rune, EMPTY).unwrap();
        let mut app = app(Backend::Rune, &project).unwrap();
        let root = app.engine.root();
        for i in 0..count {
            let e = scene::spawn_node(&mut app.engine.world_mut(), &format!("l{i}"), root);
            balaur_core::events::subscribe(&app.engine, e, "ping", None);
        }
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                balaur_core::events::emit(&app.engine, "ping", balaur_script::Value::Nil);
                app.tick(1.0 / 60.0);
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    propagate,
    spawn,
    parse_scene,
    instantiate_scene,
    physics_step,
    animation_step,
    event_delivery
);
criterion_main!(benches);
