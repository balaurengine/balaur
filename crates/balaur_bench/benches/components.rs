//! What reading and writing one component property costs.
//!
//! The paths a frame actually takes: a script reading `node.transform.position`
//! through the component handle, the same write, and the Rust entry points
//! underneath them. A UI pass reads hundreds of these a frame and animation
//! writes one per track per tick, so the per-call number is the budget.

use balaur_bench::{Backend, Project, app, attach_many};
use balaur_core::{Engine, components};
use balaur_script::{Bindings, Value};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use hecs::Entity;

/// A booted app with one node carrying a `transform`.
fn one_node() -> (tempfile::TempDir, balaur_core::App, Entity) {
    let project = Project::new(Backend::Rune, "pub fn init(this) {}\n").unwrap();
    let app = app(Backend::Rune, &project).unwrap();
    let entity =
        balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "n", app.engine.root());
    let Project { dir } = project;
    (dir, app, entity)
}

fn vec3(x: f64, y: f64, z: f64) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(x),
        toml::Value::Float(y),
        toml::Value::Float(z),
    ])
}

/// The Rust entry points, with no script in the measurement.
fn rust_side(c: &mut Criterion) {
    let mut group = c.benchmark_group("component_rust");
    let (_dir, app, entity) = one_node();
    let eng: &Engine = &app.engine;

    group.bench_function("get_whole_table", |b| {
        b.iter(|| components::get(eng, entity, "transform"));
    });
    group.bench_function("property_one_key", |b| {
        b.iter(|| components::property(eng, entity, "transform", "position"));
    });
    // `transform` registers first and `widget` last, so the two together say
    // what a name lookup costs at both ends of the registry rather than at
    // the end a scan happens to be quickest at.
    group.bench_function("lookup_first_registered", |b| {
        b.iter(|| components::is_registered(eng, "transform"));
    });
    group.bench_function("lookup_last_registered", |b| {
        b.iter(|| components::is_registered(eng, "widget"));
    });
    // On its own node: adding one puts a second component's `get` in the way
    // of `present_on` below, which is measured over a node carrying only what
    // the bundle gave it.
    let widgeted =
        balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "w", app.engine.root());
    components::add(eng, widgeted, "widget", None).unwrap();
    // `transform` rides in the node bundle, so its `Attached` bit is clear and
    // a presence test falls through to the definition. A component the
    // registry attached answers from the bit alone.
    group.bench_function("has_bundle_attached", |b| {
        b.iter(|| components::has(eng, widgeted, "transform"));
    });
    group.bench_function("has_registry_attached", |b| {
        b.iter(|| components::has(eng, widgeted, "widget"));
    });
    group.bench_function("present_on", |b| {
        b.iter(|| components::present_on(eng, entity));
    });

    let one = toml::Value::Table(toml::map::Map::from_iter([(
        String::from("position"),
        vec3(1.0, 2.0, 3.0),
    )]));
    group.bench_function("patch_one_property", |b| {
        b.iter(|| components::patch(eng, entity, "transform", &one).unwrap());
    });
    group.finish();
}

/// The script-facing paths, which is what the editor and a game both spell.
fn rune_side(c: &mut Criterion) {
    let mut group = c.benchmark_group("component_rune");
    let count = 1000usize;
    let cases: [(&str, &str); 4] = [
        (
            "handle_read",
            "pub fn update(this, dt) { for i in 0..1000 { this.node.transform.position; } }",
        ),
        (
            "handle_write",
            "pub fn update(this, dt) { for i in 0..1000 { this.node.transform.position = [1.0, 2.0, 3.0]; } }",
        ),
        (
            "get_component_keyed",
            "pub fn update(this, dt) { for i in 0..1000 { \
             this.node.get_component(\"transform\", \"position\"); } }",
        ),
        (
            "has_component",
            "pub fn update(this, dt) { for i in 0..1000 { \
             this.node.has_component(\"transform\"); } }",
        ),
    ];
    for (name, body) in cases {
        let source = format!("pub fn init(this) {{}}\n{body}\n");
        let project = Project::new(Backend::Rune, &source).unwrap();
        let app = app(Backend::Rune, &project).unwrap();
        attach_many(&app, Backend::Rune, 1).unwrap();
        let host = app.engine.script_host().unwrap();
        group.throughput(Throughput::Elements(count as u64));
        group.bench_function(BenchmarkId::new(name, count), |b| {
            b.iter(|| host.update(1.0 / 60.0));
        });
    }
    group.finish();
}

/// What one argument costs to cross the seam, by shape.
///
/// Every string argument is a `String` in `balaur_script::Value`, so a
/// component name or a property key allocates on the way in. These three say
/// what that is worth against the call itself, which is the number a decision
/// to intern them has to beat.
fn seam_arg_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("seam_arg");
    let count = 1000usize;
    let cases: [(&str, &str); 5] = [
        ("none", "bench::take0()"),
        ("int", "bench::take1(7)"),
        ("str_short", "bench::take1(\"transform\")"),
        (
            "str_boundary",
            // 22 bytes, the last length that still inlines.
            "bench::take1(\"aaaaaaaaaaaaaaaaaaaaaa\")",
        ),
        (
            "str_long",
            "bench::take1(\"a component name far past what a small string inlines\")",
        ),
    ];
    for (name, call) in cases {
        let body = format!(
            "pub fn init(this) {{}}\npub fn update(this, dt) {{ for i in 0..1000 {{ {call}; }} }}\n"
        );
        let project = Project::new(Backend::Rune, &body).unwrap();
        let mut app = app(Backend::Rune, &project).unwrap();
        {
            let mut m = app.script_module("bench").unwrap();
            let m: &mut dyn Bindings<Engine> = &mut *m;
            m.function_raw("take0", Box::new(|_, _| Ok(Value::Nil)));
            m.function_raw("take1", Box::new(|_, _| Ok(Value::Nil)));
        }
        attach_many(&app, Backend::Rune, 1).unwrap();
        let host = app.engine.script_host().unwrap();
        group.throughput(Throughput::Elements(count as u64));
        group.bench_function(BenchmarkId::new(name, count), |b| {
            b.iter(|| host.update(1.0 / 60.0));
        });
    }
    group.finish();
}

criterion_group!(benches, rust_side, rune_side, seam_arg_cost);
criterion_main!(benches);
