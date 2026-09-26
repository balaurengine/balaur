//! What moving a multimesh's instances from a script costs.
//!
//! Ten thousand instances moved every frame, two ways: one handle call per
//! instance, and one `set_buffer` carrying Godot's flat layout for all of
//! them. The per-frame number is what decides whether a game can do it at
//! 60 Hz.

use balaur_bench::{Backend, Project, app};
use balaur_core::components;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

const COUNT: usize = 10_000;

fn update_body(way: &str) -> String {
    let body = match way {
        "per_call" => {
            "let mm = this.node.multimesh3d;
    let i = 0;
    while i < 10000 {
        mm.set_instance_transform(i, balaur::Transform3d::from_translation(balaur::Vec3::new(i as f64, this.t, 0.0)));
        i += 1;
    }"
        }
        _ => {
            "let floats = [];
    let i = 0;
    while i < 10000 {
        floats.extend([1.0, 0.0, 0.0, i as f64, 0.0, 1.0, 0.0, this.t, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        i += 1;
    }
    this.node.multimesh3d.set_buffer(floats);"
        }
    };
    format!(
        "pub fn init(this) {{\n    this.t = 0.0;\n    this.node.multimesh3d.set_instance_count({COUNT});\n}}\npub fn update(this, dt) {{\n    this.t = this.t + dt;\n    {body}\n}}\n"
    )
}

fn move_every_instance(c: &mut Criterion) {
    let mut group = c.benchmark_group("multimesh_move");
    group.throughput(Throughput::Elements(COUNT as u64));
    group.sample_size(20);
    for way in ["per_call", "set_buffer"] {
        let project = Project::new(Backend::Rune, &update_body(way)).unwrap();
        let app = app(Backend::Rune, &project).unwrap();
        let entity =
            balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "field", app.engine.root());
        let params: toml::Value = toml::from_str(
            "source = { type = \"multimesh\", mesh = { type = \"mesh\", kind = \"box\" } }",
        )
        .unwrap();
        components::add(&app.engine, entity, "multimesh3d", Some(&params)).unwrap();
        let host = app.engine.script_host().unwrap();
        host.attach(balaur_core::node_id_of(entity), "s.rn")
            .unwrap();
        group.bench_function(way, |b| b.iter(|| host.update(1.0 / 60.0)));
    }
    group.finish();
}

criterion_group!(benches, move_every_instance);
criterion_main!(benches);
