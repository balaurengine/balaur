//! What reading a large JSON document into a script costs: a word pack of
//! a few megabytes (thousands of words, each a map with a nested map), read
//! with `fs::read` and handed to the script by `json::parse`, then dropped,
//! as an app's dictionary reads a language. Measured as the switch to a
//! screen whose `init` does it; *read* is the file alone, so what *parse*
//! adds is the parse and the value written into the script's heap.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

/// Categories, subcategories each, words each: 8,400 words, about a pack.
const CATEGORIES: usize = 18;
const SUBCATEGORIES: usize = 14;
const WORDS: usize = 34;

/// A pack shaped as the app's: names, then each slot's word with its facts,
/// recording and, now and then, a search key and a sentence; some slots
/// empty.
fn pack() -> String {
    let mut out = String::from("{\"categories\":[");
    for c in 0..CATEGORIES {
        if c > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"name\":\"Category {c}\",\"subcategories\":[");
        for s in 0..SUBCATEGORIES {
            if s > 0 {
                out.push(',');
            }
            let _ = write!(out, "{{\"name\":\"Subcategory {c}.{s}\",\"words\":[");
            for w in 0..WORDS {
                if w > 0 {
                    out.push(',');
                }
                if w % 17 == 16 {
                    out.push_str("null");
                    continue;
                }
                let word = format!("word{c}x{s}x{w}");
                let _ = write!(
                    out,
                    "{{\"audio\":\"/audio/words/xx/man/{word}.mp3?vsn=63962147971-7358\",\"facts\":{{\"gender\":\"m\",\"plural\":\"{word}s\"}},\"word\":\"{word}\""
                );
                if w % 5 == 0 {
                    let _ = write!(out, ",\"search\":\"{word}\"");
                }
                if w % 3 == 0 {
                    let _ = write!(
                        out,
                        ",\"sentence\":\"The {word} is on the table, next to the other one.\""
                    );
                }
                out.push('}');
            }
            out.push_str("]}");
        }
        out.push_str("]}");
    }
    out.push_str("],\"code\":\"xx\"}");
    out
}

const SCREEN: &str = r#"
[[nodes]]
id = "screen"
name = "Screen"
script = { source = "screen.bs" }
"#;

const SCREEN_SCRIPT: &str = r#"
pub class Screen {
    node: Node,
    n: int,
}

impl Screen {
    pub fn init() {
        let text = fs::read("pack.json").unwrap();
        BODY
    }
}
"#;

/// An app whose screen reads the pack, and parses it where `parse` says.
fn app(parse: bool) -> (tempfile::TempDir, App, usize) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.path().join(name), text).unwrap();
    write(
        "project.toml",
        "[application]\nname = \"json\"\nmain_scene = \"main.toml\"\n",
    );
    write("main.toml", "");
    write(
        "other.toml",
        "[[nodes]]\nid = \"other\"\nname = \"Other\"\n",
    );
    let text = pack();
    write("pack.json", &text);
    write("screen.toml", SCREEN);
    write(
        "screen.bs",
        &SCREEN_SCRIPT.replace(
            "BODY",
            if parse {
                "let pack = json::parse(text);\n        self.n = pack[\"categories\"].len();"
            } else {
                "self.n = text.len();"
            },
        ),
    );
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    config.script_backend = Some(balaur::script_bs::factory());
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app, text.len())
}

fn switch(app: &App, scene: &str) {
    balaur_core::scene_switch::request(&app.engine, scene);
    balaur_core::scene_switch::apply_system(&app.engine, 0.0);
}

/// The switch into the screen that reads the pack; the switch back, off the
/// clock.
#[allow(
    clippy::disallowed_methods,
    reason = "this is the measurement, not simulation"
)]
fn json(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_pack");
    group.sample_size(10);
    for parse in [false, true] {
        let (_dir, app, bytes) = app(parse);
        switch(&app, "screen.toml");
        switch(&app, "other.toml");
        group.throughput(Throughput::Bytes(bytes as u64));
        let name = if parse { "parse" } else { "read" };
        group.bench_with_input(BenchmarkId::new(name, bytes), &bytes, |b, _| {
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
    group.finish();
}

criterion_group!(benches, json);
criterion_main!(benches);
