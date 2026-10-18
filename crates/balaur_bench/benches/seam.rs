//! What one `ui::*` call costs a script, crossing into Rust and back.
//!
//! Timed, and counted: the allocations a call makes are the same on a loaded
//! machine as on an idle one, so they say what changed when the times cannot.
//! Its own binary because the counter is the process's.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

/// Allocations since the process started, reallocations included.
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract, passed straight through.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract, passed straight through.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract, passed straight through.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Calls a pass makes, so a call's share of the pass is well above the pass's
/// own floor.
const CALLS: u32 = 200;

/// One `draw` node whose callback runs `call` [`CALLS`] times a pass, after
/// `before`, and records that it reached the end.
fn calling(before: &str, call: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"seam\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"screen\"\nname = \"Screen\"\nscript = { source = \"scripts/s.rn\" }\n\
         [nodes.widget]\nkind = \"column\"\nanchor = \"fill\"\n\
         \n[[nodes]]\nid = \"hatch\"\nname = \"Hatch\"\nparent = \"screen\"\n\
         [nodes.widget]\nkind = \"draw\"\ndraw = \"calls\"\nanchor = \"fill\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        format!(
            "pub fn init(this) {{ this.made = 0; }}\n\
             pub fn made(this) {{ this.made }}\n\
             pub fn calls(this) {{\n\
             \x20   this.made = 0;\n\
             \x20   {before}\n\
             \x20   for i in 0..{CALLS} {{\n\
             \x20       {call};\n\
             \x20   }}\n\
             \x20   this.made = {CALLS};\n\
             }}\n"
        ),
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn one_pass(app: &App, ctx: &egui::Context) {
    ctx.begin_pass(egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    });
    balaur_ui::run_pass(&app.engine, ctx);
    ctx.end_pass().textures_delta.clear();
}

/// Whether the last pass ran the loop to its end: a callback that threw stops
/// short, and a pass that called nothing benchmarks as a fast one.
fn ran(app: &App) -> bool {
    let world = app.engine.world();
    let Some(screen) = balaur_core::scene::find_node(&world, app.engine.root(), "Screen") else {
        return false;
    };
    drop(world);
    let made = app
        .engine
        .script_host()
        .and_then(|host| host.call_on(balaur_core::node_id_of(screen), "made", &[]));
    made == Some(balaur_script::Value::Int(i64::from(CALLS)))
}

/// The shapes the editor's shell calls in: a number, an options table written
/// at the call, the same table built once, and a container with a callback.
/// The loop alone is the floor, so a case less `loop`, over [`CALLS`], is what
/// one call costs.
fn ui_call(c: &mut Criterion) {
    let mut group = c.benchmark_group("ui_call");
    let table = "#{ font_size: 13, text_color: \"#c8ccd4\", truncate: true }";
    let cases = [
        ("loop", String::new(), "()".to_string()),
        ("add_space", String::new(), "ui::add_space(2.0)".to_string()),
        (
            "label_table",
            String::new(),
            format!("ui::label(\"name\", {table})"),
        ),
        (
            "label_held",
            format!("let opts = {table};"),
            "ui::label(\"name\", opts)".to_string(),
        ),
        (
            "row_callback",
            String::new(),
            "ui::row(#{ height: 24 }, || {})".to_string(),
        ),
    ];
    let mut floor = 0.0;
    for (name, before, call) in cases {
        let (_dir, app) = calling(&before, &call);
        let ctx = egui::Context::default();
        // Three passes to settle: the first installs fonts, and an area is
        // invisible until it has been sized once.
        for _ in 0..3 {
            one_pass(&app, &ctx);
        }
        assert!(ran(&app), "{name} did not run to its end");
        let passes = 50u32;
        let start = ALLOCATIONS.load(Ordering::Relaxed);
        for _ in 0..passes {
            one_pass(&app, &ctx);
        }
        #[allow(clippy::cast_precision_loss, reason = "a count far under 2^52")]
        let per_pass = (ALLOCATIONS.load(Ordering::Relaxed) - start) as f64 / f64::from(passes);
        if name == "loop" {
            floor = per_pass;
        }
        println!(
            "ui_call/{name}: {:.2} allocations a call",
            (per_pass - floor) / f64::from(CALLS)
        );
        group.throughput(Throughput::Elements(u64::from(CALLS)));
        group.bench_function(BenchmarkId::new(name, CALLS), |b| {
            b.iter(|| one_pass(&app, &ctx));
        });
    }
    group.finish();
}

criterion_group!(benches, ui_call);
criterion_main!(benches);
