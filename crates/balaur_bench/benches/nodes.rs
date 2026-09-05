//! Where a scene-tree operation's microsecond goes, layer by layer: the Rune
//! loop on its own, a binding call that does nothing, the node op through the
//! value layer from Rust, and the bare ECS spawn — so a slow `add_child` says
//! which of the four to look at.

use balaur_bench::{Backend, Project, app, attach_many};
use balaur_core::node_api::NODE_OPS;
use balaur_core::{Engine, scene};
use balaur_script::{Bindings, BindingsExt, Value};
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

const COUNT: usize = 1000;

fn op(name: &str) -> fn(&Engine, &[Value]) -> anyhow::Result<Value> {
    NODE_OPS
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("no node op {name}"))
        .call
}

/// The Rust half: the bare spawn, the op through the value layer with the
/// stable id it mints, a batched free, and a path lookup.
fn rust_side(c: &mut Criterion) {
    let mut group = c.benchmark_group("node_ops_rust");
    group.throughput(Throughput::Elements(COUNT as u64));

    let project = Project::new(Backend::Rune, "pub fn init(this) {}\n").unwrap();
    let app = app(Backend::Rune, &project).unwrap();
    let root = app.engine.root();

    group.bench_function(BenchmarkId::new("spawn_node", COUNT), |b| {
        b.iter(|| {
            let mut world = app.engine.world_mut();
            for i in 0..COUNT {
                scene::spawn_node(&mut world, &format!("n{i}"), root);
            }
        });
    });

    let add_child = op("add_child");
    let parent = Value::Node(balaur_core::node_id_of(root).0);
    group.bench_function(BenchmarkId::new("add_child_op", COUNT), |b| {
        b.iter(|| {
            for i in 0..COUNT {
                add_child(&app.engine, &[parent.clone(), Value::Str(format!("n{i}"))]).unwrap();
            }
        });
    });

    group.bench_function(BenchmarkId::new("free_nodes", COUNT), |b| {
        b.iter_batched(
            || {
                let holder = scene::spawn_node(&mut app.engine.world_mut(), "holder", root);
                let made: Vec<_> = {
                    let mut world = app.engine.world_mut();
                    (0..COUNT)
                        .map(|i| scene::spawn_node(&mut world, &format!("n{i}"), holder))
                        .collect()
                };
                made
            },
            |made| scene::free_nodes(&app.engine, &made),
            BatchSize::SmallInput,
        );
    });

    // A random tree, looked up by path from its root, as the Godot case does.
    let lookup_root = scene::spawn_node(&mut app.engine.world_mut(), "Lookups", root);
    let mut paths = Vec::with_capacity(COUNT);
    {
        let mut world = app.engine.world_mut();
        let mut nodes = vec![(lookup_root, String::new())];
        let mut seed = 0x9E37_79B9u64;
        for i in 0..COUNT {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let (parent, above) = nodes[(seed as usize) % nodes.len()].clone();
            let name = format!("n{i}");
            let e = scene::spawn_node(&mut world, &name, parent);
            let path = if above.is_empty() { name } else { format!("{above}/{name}") };
            paths.push(path.clone());
            nodes.push((e, path));
        }
    }
    group.bench_function(BenchmarkId::new("find_node", COUNT), |b| {
        b.iter(|| {
            let world = app.engine.world();
            for path in &paths {
                scene::find_node(&world, lookup_root, path).unwrap();
            }
        });
    });
    group.finish();
}

/// The Rune half: the loop alone, then with a call that does nothing, then
/// the real calls — so the seam is the difference between neighbours.
fn rune_side(c: &mut Criterion) {
    let mut group = c.benchmark_group("node_ops_rune");
    group.throughput(Throughput::Elements(COUNT as u64));
    let bodies = [
        ("loop_only", "pub fn update(this, dt) { for i in 0..1000 { let s = format!(\"n{}\", i); } }"),
        ("noop_call", "pub fn update(this, dt) { for i in 0..1000 { bench::noop(i); } }"),
        ("add_child", "pub fn update(this, dt) { let p = this.node.add_child(\"batch\"); for i in 0..1000 { p.add_child(format!(\"n{}\", i)); } }"),
        ("get_node", "pub fn init(this) { this.paths = []; let nodes = [this.node]; let names = [\"\"]; for i in 0..1000 { let at = rng::int(0, (nodes.len() - 1) as i64); let name = format!(\"n{}\", i); nodes.push(nodes[at].add_child(name)); let above = names[at]; let path = if above == \"\" { name } else { format!(\"{}/{}\", above, name) }; names.push(path); this.paths.push(path); } }\npub fn update(this, dt) { for path in this.paths { this.node.get_node(path); } }"),
    ];
    for (name, body) in bodies {
        let source = if body.contains("pub fn init") {
            body.to_string()
        } else {
            format!("pub fn init(this) {{}}\n{body}\n")
        };
        let project = Project::new(Backend::Rune, &source).unwrap();
        let mut app = app(Backend::Rune, &project).unwrap();
        {
            let mut m = app.script_module("bench").unwrap();
            let m: &mut dyn Bindings<Engine> = &mut *m;
            m.function("noop", |_: &Engine, _: i64| Ok(()));
        }
        attach_many(&app, Backend::Rune, 1).unwrap();
        let host = app.engine.script_host().unwrap();
        group.bench_function(BenchmarkId::new(name, COUNT), |b| b.iter(|| host.update(1.0 / 60.0)));
    }
    group.finish();
}

criterion_group!(benches, rust_side, rune_side);
criterion_main!(benches);
