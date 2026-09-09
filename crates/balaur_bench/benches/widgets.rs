//! What a screen of widget nodes costs a frame: the whole pass, at a hundred
//! nodes, a thousand and ten thousand.
//!
//! Two cases per size, because they answer different questions. *Idle* is a
//! tree nothing changed — what an editor sitting still pays, where the layout
//! is already solved and only the draw runs. *Changed* rewrites a label every
//! iteration, which dirties the box that holds it and re-solves what that
//! reaches, which is what an animating panel pays.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::hecs::Entity;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

/// Rows a column, so the tree has the shape a real screen does rather than
/// one container with ten thousand children.
const PER_ROW: usize = 8;

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"w\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    (dir, standard_app(config).unwrap())
}

fn add(app: &App, parent: Entity, name: &str, params: &toml::Value) -> Entity {
    let entity = balaur::scene::spawn_node(&mut app.engine.world_mut(), name, parent);
    balaur::components::add(&app.engine, entity, "widget", Some(params)).unwrap();
    entity
}

/// A column of rows of buttons, `count` widgets in all, and the leaf a
/// changing case rewrites.
fn screen(app: &App, count: usize, cell: &toml::Value) -> (Entity, Entity) {
    let root = app.engine.root();
    let column = add(
        app,
        root,
        "Screen",
        &toml::toml! { kind = "column" anchor = "fill" gap = 2 }.into(),
    );
    let mut leaf = column;
    let mut made = 1;
    while made < count {
        let row = add(
            app,
            column,
            "Row",
            &toml::toml! { kind = "row" gap = 2 }.into(),
        );
        made += 1;
        for _ in 0..PER_ROW {
            if made >= count {
                break;
            }
            leaf = add(app, row, "Cell", &cell.clone());
            made += 1;
        }
    }
    (column, leaf)
}

fn input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    }
}

fn one_pass(app: &App, ctx: &egui::Context) {
    ctx.begin_pass(input());
    balaur_ui::run_pass(&app.engine, ctx);
    let mut out = ctx.end_pass();
    out.textures_delta.clear();
}

fn widgets(c: &mut Criterion) {
    let mut group = c.benchmark_group("widget_pass");
    // A cell with a caption and one without: the difference is what shaping
    // and painting text costs, which is the half a container never pays.
    let captioned: toml::Value = toml::toml! { kind = "button" text = "cell" }.into();
    let bare: toml::Value = toml::toml! { kind = "row" width = 40 height = 18 }.into();
    for (shape, cell) in [("text", &captioned), ("boxes", &bare)] {
        for count in [100usize, 1_000, 10_000] {
            let (_dir, app) = app();
            let (column, leaf) = screen(&app, count, cell);
            let ctx = egui::Context::default();
            // Three passes to settle: the first installs fonts and an area is
            // invisible until it has been sized once.
            for _ in 0..3 {
                one_pass(&app, &ctx);
            }
            group.throughput(Throughput::Elements(count as u64));
            group.bench_function(BenchmarkId::new(format!("idle_{shape}"), count), |b| {
                b.iter(|| one_pass(&app, &ctx));
            });
            // The root hidden: the arena is still built from the world and every
            // widget still cloned into it, but nothing is solved or drawn. What
            // is left is the walk itself.
            balaur::components::add(
                &app.engine,
                column,
                "widget",
                Some(&toml::toml! { kind = "column" anchor = "fill" visible = false }.into()),
            )
            .unwrap();
            one_pass(&app, &ctx);
            group.bench_function(BenchmarkId::new(format!("walk_{shape}"), count), |b| {
                b.iter(|| one_pass(&app, &ctx));
            });
            balaur::components::add(
                &app.engine,
                column,
                "widget",
                Some(&toml::toml! { kind = "column" anchor = "fill" gap = 2 }.into()),
            )
            .unwrap();
            one_pass(&app, &ctx);
            let mut tick = 0u32;
            group.bench_function(BenchmarkId::new(format!("changed_{shape}"), count), |b| {
                b.iter(|| {
                    tick = tick.wrapping_add(1);
                    let params = toml::toml! { kind = "button" text = (format!("cell {tick}")) };
                    balaur::components::add(&app.engine, leaf, "widget", Some(&params.into()))
                        .unwrap();
                    one_pass(&app, &ctx);
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, widgets);
criterion_main!(benches);
