//! Where a frame's time goes outside scripting: the scene tree, the seam's
//! own conversions, and physics.

use balaur_bench::{app, Backend, Project};
use balaur_core::scene;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

const EMPTY: &str = "local S = {}\nfunction S:init() end\nreturn S\n";

/// Transform propagation over a tree, which runs every frame for every node.
fn propagate(c: &mut Criterion) {
    let mut group = c.benchmark_group("propagate_transforms");
    for count in [100usize, 1000, 10_000] {
        let project = Project::new(Backend::Luau, EMPTY).unwrap();
        let app = app(Backend::Luau, &project).unwrap();
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
fn spawn(c: &mut Criterion) {
    let mut group = c.benchmark_group("spawn_nodes");
    for count in [100usize, 1000] {
        let project = Project::new(Backend::Luau, EMPTY).unwrap();
        let app = app(Backend::Luau, &project).unwrap();
        let root = app.engine.root();
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                let mut world = app.engine.world_mut();
                for i in 0..count {
                    scene::spawn_node(&mut world, &format!("n{i}"), root);
                }
            });
        });
    }
    group.finish();
}

/// Instantiating a scene document: parsing plus spawning, which is what
/// loading a level costs.
fn instantiate_scene(c: &mut Criterion) {
    let mut group = c.benchmark_group("instantiate_scene");
    for count in [50usize, 500] {
        let mut doc = String::new();
        for i in 0..count {
            use std::fmt::Write as _;
            let _ = write!(
                doc,
                "[[nodes]]\nid = \"n{i}\"\nname = \"N{i}\"\nposition = [1.0, 2.0, 3.0]\n\n"
            );
        }
        let project = Project::new(Backend::Luau, EMPTY).unwrap();
        let app = app(Backend::Luau, &project).unwrap();
        let root = app.engine.root();
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                balaur_core::project::instantiate_scene(&app.engine, &doc, root, false).unwrap();
            });
        });
    }
    group.finish();
}

/// A physics step at a few body counts.
fn physics_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("physics_step");
    for count in [100usize, 1000] {
        let project = Project::new(Backend::Luau, EMPTY).unwrap();
        let mut app = app(Backend::Luau, &project).unwrap();
        let root = app.engine.root();
        for i in 0..count {
            let e = scene::spawn_node(&mut app.engine.world_mut(), &format!("b{i}"), root);
            let params = toml::toml! { kind = "dynamic" };
            let _ = balaur_core::components::add(&app.engine, e, "body", Some(&params.into()));
        }
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| app.tick(1.0 / 60.0));
        });
    }
    group.finish();
}

criterion_group!(benches, propagate, spawn, instantiate_scene, physics_step);
criterion_main!(benches);
