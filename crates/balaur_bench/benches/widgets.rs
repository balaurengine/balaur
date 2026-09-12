//! What a screen of a thousand widget nodes costs a frame.
//!
//! Three cases, because they answer different questions. *Idle* is a tree
//! nothing changed — what an editor sitting still pays, where the layout is
//! already solved and only the draw runs. *Walk* hides the root, so the arena
//! is still built and nothing is solved or drawn: the walk on its own.
//! *Changed* rewrites a label every iteration, dirtying the box that holds it
//! and re-solving what that reaches, which is what an animating panel pays.

use std::fmt::Write as _;

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
        for count in [1_000usize] {
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

/// A project whose scene is `scene` and whose one script is `script`.
fn app_from(scene: &str, script: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"w\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    // The scene is read here, not by `standard_app`: without it the world is
    // empty and both cases below would benchmark a blank pass.
    app.load_project().unwrap();
    (dir, app)
}

/// `count` captioned cells as one widget node each: what a pooled strip or a
/// synced form leaves in the scene.
fn as_nodes(count: usize) -> (tempfile::TempDir, App) {
    let mut scene = String::from(
        "[[nodes]]\nid = \"screen\"\nname = \"Screen\"\n\
         [nodes.widget]\nkind = \"row\"\nanchor = \"fill\"\ngap = 2\n",
    );
    for i in 0..count {
        let _ = write!(
            scene,
            "\n[[nodes]]\nid = \"c{i}\"\nname = \"Cell\"\nparent = \"screen\"\n\
             [nodes.widget]\nkind = \"button\"\ntext = \"cell\"\n"
        );
    }
    app_from(&scene, "pub fn init(this) {}\n")
}

/// The same `count` cells as one `draw` node, painted by a script: what the
/// top bar and the status strip do.
fn as_one_draw(count: usize) -> (tempfile::TempDir, App) {
    let scene = "[[nodes]]\nid = \"screen\"\nname = \"Screen\"\nscript = { source = \"scripts/s.rn\" }\n\
         [nodes.widget]\nkind = \"row\"\nanchor = \"fill\"\ngap = 2\n\
         \n[[nodes]]\nid = \"hatch\"\nname = \"Hatch\"\nparent = \"screen\"\n\
         [nodes.widget]\nkind = \"draw\"\ndraw = \"cells\"\n";
    let script = format!(
        "pub fn init(this) {{}}\n\
         pub fn cells(this) {{\n\
         \x20   ui::horizontal(#{{ tight: true }}, || {{\n\
         \x20       for i in 0..{count} {{\n\
         \x20           ui::pill(\"cell\", #{{}});\n\
         \x20       }}\n\
         \x20   }});\n\
         }}\n"
    );
    app_from(scene, &script)
}

/// How many captions a pass painted: a script that failed to compile draws
/// nothing, and an empty pass would benchmark as a very fast one.
fn captions(app: &App, ctx: &egui::Context) -> usize {
    ctx.begin_pass(input());
    balaur_ui::run_pass(&app.engine, ctx);
    let out = ctx.end_pass();
    out.shapes.iter().map(|s| texts_in(&s.shape)).sum()
}

/// Text shapes at any depth: a layout that groups its children hands back one
/// `Shape::Vec` holding them, and counting only the top level sees none.
fn texts_in(shape: &egui::epaint::Shape) -> usize {
    match shape {
        egui::epaint::Shape::Text(_) => 1,
        egui::epaint::Shape::Vec(inner) => inner.iter().map(texts_in).sum(),
        _ => 0,
    }
}

/// The same row of cells, built the two ways the shell builds UI. Not a
/// controlled comparison of painters — a `button` node and `ui::pill` are
/// different code — but it is the choice a panel actually faces.
fn hatch_or_nodes(c: &mut Criterion) {
    let mut group = c.benchmark_group("widget_hatch");
    for count in [64usize] {
        for (way, built) in [("nodes", as_nodes(count)), ("one_draw", as_one_draw(count))] {
            let (_dir, app) = built;
            let ctx = egui::Context::default();
            for _ in 0..3 {
                one_pass(&app, &ctx);
            }
            // Something, not how much: a row clips past about 39 captions.
            // Zero is the catch, since an empty pass benchmarks as a fast one.
            let drawn = captions(&app, &ctx);
            assert!(drawn > 0, "{way}/{count} painted nothing");
            group.throughput(Throughput::Elements(count as u64));
            group.bench_function(BenchmarkId::new(way, count), |b| {
                b.iter(|| one_pass(&app, &ctx));
            });
        }
    }
    group.finish();
}

criterion_group!(benches, widgets, hatch_or_nodes);
criterion_main!(benches);
