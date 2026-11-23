//! What a screen of instanced rows costs to build: a list of a few hundred
//! words, each a part scene of a dozen widget nodes with a scripted part
//! inside it, made by `scene.repeat` from the screen's `init` and filled
//! row by row, as an app's word list is. Measured as the switch to the
//! screen, which frees the one before it: what a tap into the list waits for.
//!
//! *plain* rows hold widgets only; *scripted* rows hold a part with its own
//! script, called once a row, which is what the scripts each instance
//! attaches cost on top.

use std::time::{Duration, Instant};

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

/// A row: a dozen widgets in rows, a column and a grid, as a word's line is.
const ROW: &str = r#"
[[nodes]]
id = "row"
name = "Row"
bindings = { rows = [{ event = "pointer_click", action = "call", value = "pressed" }] }

[nodes.widget]
kind = "row"

[nodes.widget_layout]
gap = [12, 0]
align_items = "center"

[[nodes]]
id = "picture"
name = "Picture"
parent = "row"

[nodes.widget]
kind = "image"
interactive = false

[[nodes]]
id = "cols"
name = "Cols"
parent = "row"

[nodes.widget]
kind = "grid"
grow = 1

[nodes.widget_grid]
grid_columns = "2fr 3fr"

[[nodes]]
id = "base"
name = "Base"
parent = "cols"

[nodes.widget]
kind = "column"

[[nodes]]
id = "head"
name = "Head"
parent = "base"

[nodes.widget]
kind = "row"

[nodes.widget_layout]
gap = [8, 0]

[[nodes]]
id = "word"
name = "Word"
parent = "head"

[nodes.widget]
kind = "label"
text = "coffee"

[[nodes]]
id = "info"
name = "Info"
parent = "head"
instance = "INFO"

[[nodes]]
id = "sentence"
name = "Sentence"
parent = "base"
visible = false

[nodes.widget]
kind = "label"
text = ""

[nodes.widget_text]
wrap = true

[[nodes]]
id = "target"
name = "Target"
parent = "cols"

[nodes.widget]
kind = "column"

[[nodes]]
id = "line"
name = "Line"
parent = "target"

[nodes.widget]
kind = "row"

[[nodes]]
id = "gender"
name = "Gender"
parent = "line"

[nodes.widget]
kind = "button"
text = "m"

[[nodes]]
id = "said"
name = "Said"
parent = "line"

[nodes.widget]
kind = "label"
text = "le café"

[[nodes]]
id = "form"
name = "Form"
parent = "line"

[nodes.widget]
kind = "label"
text = ""

[[nodes]]
id = "play"
name = "Play"
parent = "row"

[nodes.widget]
kind = "button"
interactive = false

[nodes.widget_button]
icon = "speaker-wave"
"#;

/// The nodes a row holds.
const ROW_NODES: usize = 14;

/// The part inside a row: a button, its script or none.
const INFO: &str = r#"
[[nodes]]
id = "info"
name = "Info"
SCRIPT

[nodes.widget]
kind = "button"
text = ""
"#;

const INFO_SCRIPT: &str = r#"
pub class Info {
    node: Node,
    pos: str,
}

impl Info {
    pub fn show(pos: str) {
        self.pos = pos;
        self.node.widget.text = pos;
        self.node.set_visible(pos != "");
    }
}
"#;

/// The screen: a column whose script makes `ROWS` rows and fills each.
const SCREEN: &str = r#"
[[nodes]]
id = "screen"
name = "Screen"
script = { source = "screen.bs" }

[nodes.widget]
kind = "column"
anchor = "fill"
"#;

const SCREEN_SCRIPT: &str = r#"
pub class Screen {
    node: Node,
}

impl Screen {
    pub fn init() {
        let items: [scene::RepeatItem] = [];
        for i in 0..ROWS {
            items.push(scene::RepeatItem { key: "word_{i}", scene: "row.toml" });
        }
        let made = scene::repeat(self.node, items);
        for i in 0..made.len() {
            made[i].get_node("Cols/Base/Head/Word").widget.text = "word {i}";
            made[i].get_node("Cols/Target/Line/Said").widget.text = "mot {i}";
            CALL
        }
    }

    pub fn pressed(_node: Node) {}
}
"#;

/// An app whose screen makes `rows` rows, scripted inside or not.
fn app(rows: usize, scripted: bool) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.path().join(name), text).unwrap();
    write(
        "project.toml",
        "[application]\nname = \"rows\"\nmain_scene = \"main.toml\"\n",
    );
    write("main.toml", "");
    write(
        "other.toml",
        "[[nodes]]\nid = \"other\"\nname = \"Other\"\n",
    );
    write("screen.toml", SCREEN);
    write(
        "screen.bs",
        &SCREEN_SCRIPT.replace("ROWS", &rows.to_string()).replace(
            "CALL",
            if scripted {
                r#"made[i].get_node("Cols/Base/Head/Info").call("show", "noun");"#
            } else {
                ""
            },
        ),
    );
    write("row.toml", &ROW.replace("INFO", "info.toml"));
    write(
        "info.toml",
        &INFO.replace(
            "SCRIPT",
            if scripted {
                "script = { source = \"info.bs\" }"
            } else {
                ""
            },
        ),
    );
    write("info.bs", INFO_SCRIPT);
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    config.script_backend = Some(balaur::script_bs::factory());
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn switch(app: &App, scene: &str) {
    balaur_core::scene_switch::request(&app.engine, scene);
    balaur_core::scene_switch::apply_system(&app.engine, 0.0);
}

/// The switch into the screen, from another; the switch back, off the clock.
#[allow(
    clippy::disallowed_methods,
    reason = "this is the measurement, not simulation"
)]
fn rows(c: &mut Criterion) {
    let mut group = c.benchmark_group("screen_rows");
    group.sample_size(10);
    for scripted in [false, true] {
        for count in [50usize, 150] {
            let (_dir, app) = app(count, scripted);
            // Each script compiled once, before the clock.
            switch(&app, "screen.toml");
            switch(&app, "other.toml");
            group.throughput(Throughput::Elements((count * ROW_NODES) as u64));
            let name = if scripted { "scripted" } else { "plain" };
            group.bench_with_input(BenchmarkId::new(name, count), &count, |b, _| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let start = Instant::now();
                        switch(&app, "screen.toml");
                        total += start.elapsed();
                        switch(&app, "other.toml");
                    }
                    total
                });
            });
        }
    }
    group.finish();
}

/// The switch away from the screen, every row freed; the switch in, off the
/// clock.
#[allow(
    clippy::disallowed_methods,
    reason = "this is the measurement, not simulation"
)]
fn leave(c: &mut Criterion) {
    let mut group = c.benchmark_group("screen_rows_leave");
    group.sample_size(10);
    for scripted in [false, true] {
        let count = 150usize;
        let (_dir, app) = app(count, scripted);
        switch(&app, "screen.toml");
        switch(&app, "other.toml");
        group.throughput(Throughput::Elements((count * ROW_NODES) as u64));
        let name = if scripted { "scripted" } else { "plain" };
        group.bench_with_input(BenchmarkId::new(name, count), &count, |b, _| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    switch(&app, "screen.toml");
                    let start = Instant::now();
                    switch(&app, "other.toml");
                    total += start.elapsed();
                }
                total
            });
        });
    }
    group.finish();
}

criterion_group!(benches, rows, leave);
criterion_main!(benches);
