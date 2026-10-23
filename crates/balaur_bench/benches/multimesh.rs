//! What moving a multimesh's instances from a script costs.
//!
//! Ten thousand instances moved every frame by one `set_buffer` carrying
//! Godot's flat layout for all of them. The per-frame number is what decides
//! whether a game can do it at 60 Hz.

use balaur_bench::{Backend, Project, app, script};
use balaur_core::components;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

const COUNT: usize = 10_000;

fn source() -> String {
    script(
        ", t: float",
        &format!(
            "    fn init() {{
        self.node.multimesh3d.set_instance_count({COUNT});
    }}
    fn update(dt: float) {{
        self.t = self.t + dt;
        let floats: [float] = [];
        for i in 0..{COUNT} {{
            floats.extend([1.0, 0.0, 0.0, i as float, 0.0, 1.0, 0.0, self.t, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        }}
        self.node.multimesh3d.set_buffer(floats);
    }}"
        ),
    )
}

fn move_every_instance(c: &mut Criterion) {
    let mut group = c.benchmark_group("multimesh_move");
    group.throughput(Throughput::Elements(COUNT as u64));
    group.sample_size(20);
    let project = Project::new(Backend::Bs, &source()).unwrap();
    let app = app(Backend::Bs, &project).unwrap();
    let entity =
        balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "field", app.engine.root());
    let params: toml::Value = toml::from_str(
        "source = { type = \"multimesh\", mesh = { type = \"mesh\", kind = \"box\" } }",
    )
    .unwrap();
    components::add(&app.engine, entity, "multimesh3d", Some(&params)).unwrap();
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(entity), "s.bs")
        .unwrap();
    group.bench_function("set_buffer", |b| b.iter(|| host.update(1.0 / 60.0)));
    group.finish();
}

criterion_group!(benches, move_every_instance);
criterion_main!(benches);
