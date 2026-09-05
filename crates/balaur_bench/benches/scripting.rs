//! What scripting costs, measured the same way on every backend.
//!
//! The interesting number is per node per frame: a game's budget is 16.6 ms,
//! and `update` runs on every scripted node in it. Everything here is headless.

use balaur_bench::{Backend, Project, app, attach_many};
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt};
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

/// Bodies that do the same work in each language.
fn source(backend: Backend, body: Body) -> String {
    match (backend, body) {
        (Backend::Rune, Body::Empty) => {
            "pub fn init(this) {}\npub fn update(this, dt) {}\n".into()
        }
        (Backend::Rune, Body::State) => {
            "pub fn init(this) { this.n = 0.0; }\npub fn update(this, dt) { this.n = this.n + dt; }\n".into()
        }
        (Backend::Rune, Body::NodeApi) => {
            "pub fn init(this) {}\npub fn update(this, dt) { this.node.translate(dt, 0.0, 0.0); }\n".into()
        }
    }
}

#[derive(Clone, Copy)]
enum Body {
    /// The floor: dispatch and nothing else.
    Empty,
    /// Dispatch plus reading and writing instance state.
    State,
    /// Dispatch plus a call back into the engine through the seam.
    NodeApi,
}

impl Body {
    fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::State => "state",
            Self::NodeApi => "node_api",
        }
    }
}

/// One `update` across N scripted nodes — the shape of a real frame.
fn update_across_nodes(c: &mut Criterion) {
    let mut group = c.benchmark_group("update_per_node");
    for body in [Body::Empty, Body::State, Body::NodeApi] {
        for count in [100usize, 1000] {
            group.throughput(Throughput::Elements(count as u64));
            for backend in Backend::ALL {
                let project = Project::new(backend, &source(backend, body)).unwrap();
                let app = app(backend, &project).unwrap();
                attach_many(&app, backend, count).unwrap();
                let host = app.engine.script_host().unwrap();
                group.bench_with_input(
                    BenchmarkId::new(format!("{}/{}", body.name(), backend.name()), count),
                    &count,
                    |b, _| b.iter(|| host.update(1.0 / 60.0)),
                );
            }
        }
    }
    group.finish();
}

/// A binding call on its own: script in, Rust out, value back.
fn binding_call(c: &mut Criterion) {
    let mut group = c.benchmark_group("binding_call");
    for backend in Backend::ALL {
        let body = match backend {
            Backend::Rune => {
                "pub fn init(this) {}\npub fn update(this, dt) { bench::noop(1, 2.0, \"three\"); }\n"
            }
        };
        let project = Project::new(backend, body).unwrap();
        let mut app = app(backend, &project).unwrap();
        {
            let mut m = app.script_module("bench").unwrap();
            let m: &mut dyn Bindings<Engine> = &mut *m;
            m.function("noop", |_: &Engine, (a, b, c): (i64, f64, String)| {
                Ok(a as f64 + b + c.len() as f64)
            });
        }
        attach_many(&app, backend, 1).unwrap();
        let host = app.engine.script_host().unwrap();
        group.bench_function(backend.name(), |b| b.iter(|| host.update(1.0 / 60.0)));
    }
    group.finish();
}

/// First attach: compiling the script and, for Rune, building the context.
///
/// Separated from the per-node cost because they are different problems. This
/// one is paid once per script file, and again on every hot reload.
fn attach_first(c: &mut Criterion) {
    let mut group = c.benchmark_group("attach_first");
    for backend in Backend::ALL {
        let project = Project::new(backend, &source(backend, Body::State)).unwrap();
        group.bench_function(backend.name(), |b| {
            b.iter_batched(
                || app(backend, &project).unwrap(),
                |app| attach_many(&app, backend, 1).unwrap(),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Attaching to more nodes once the script is compiled — what spawning a wave
/// of enemies actually costs.
fn attach_more(c: &mut Criterion) {
    let mut group = c.benchmark_group("attach_more");
    let count = 100usize;
    for backend in Backend::ALL {
        let project = Project::new(backend, &source(backend, Body::State)).unwrap();
        group.throughput(Throughput::Elements(count as u64));
        group.bench_function(BenchmarkId::new(backend.name(), count), |b| {
            b.iter_batched(
                || {
                    let app = app(backend, &project).unwrap();
                    // Pay the compile in setup, so it is not in the number.
                    attach_many(&app, backend, 1).unwrap();
                    app
                },
                |app| attach_many(&app, backend, count).unwrap(),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Compiling a second script once the context exists.
///
/// Splits `attach_first` in two: if this is cheap, the cost there is building
/// the language context and is paid once at startup. If it is expensive, every
/// hot reload pays it, which is a very different problem.
fn compile_second_script(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile_second_script");
    for backend in Backend::ALL {
        let project = Project::new(backend, &source(backend, Body::State)).unwrap();
        let second = format!("s2.{}", backend.extension());
        std::fs::write(project.path().join(&second), source(backend, Body::Empty)).unwrap();
        group.bench_function(backend.name(), |b| {
            b.iter_batched(
                || {
                    let app = app(backend, &project).unwrap();
                    attach_many(&app, backend, 1).unwrap();
                    app
                },
                |app| {
                    let host = app.engine.script_host().unwrap();
                    let e = balaur_core::scene::spawn_node(
                        &mut app.engine.world_mut(),
                        "second",
                        app.engine.root(),
                    );
                    host.attach(balaur_core::node_id_of(e), &second).unwrap();
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// What each argument shape costs to cross the seam. Every binding argument
/// is converted to a neutral value and back, so a map is not a free parameter.
fn binding_arg_shapes(c: &mut Criterion) {
    let mut group = c.benchmark_group("binding_arg");
    let shapes: [(&str, &str); 4] = [
        ("int", "42"),
        ("str", "\"a moderate string\""),
        ("list", "[1, 2, 3]"),
        ("map", "#{x: 1.0, y: 2.0, name: \"widget\"}"),
    ];
    // A literal is rebuilt by the script on every call, so those numbers are
    // allocation plus conversion. `map_reused` passes one built up front, which
    // isolates what crossing the seam actually costs.
    let reused: [(&str, &str); 1] = [("map_reused", "this.opts")];
    for (name, arg) in shapes.into_iter().chain(reused) {
        for backend in Backend::ALL {
            let setup = if name == "map_reused" {
                "this.opts = #{x: 1.0, y: 2.0, name: \"widget\"};"
            } else {
                ""
            };
            let body = match backend {
                Backend::Rune => format!(
                    "pub fn init(this) {{ {setup} }}\npub fn update(this, dt) {{ bench::take({arg}); }}\n"
                ),
            };
            let project = Project::new(backend, &body).unwrap();
            let mut app = app(backend, &project).unwrap();
            {
                let mut m = app.script_module("bench").unwrap();
                let m: &mut dyn Bindings<Engine> = &mut *m;
                m.function_raw("take", Box::new(|_, _| Ok(balaur_script::Value::Nil)));
            }
            attach_many(&app, backend, 1).unwrap();
            let host = app.engine.script_host().unwrap();
            group.bench_function(BenchmarkId::new(backend.name(), name), |b| {
                b.iter(|| host.update(1.0 / 60.0));
            });
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    update_across_nodes,
    binding_call,
    binding_arg_shapes,
    attach_first,
    attach_more,
    compile_second_script
);
criterion_main!(benches);
