//! A word turned into triangles: the same word twice is the same mesh, a
//! letter's counters stay holes, and the whole thing is a `mesh` asset a
//! collider can be fitted to.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::mesh::{MeshData, TextShape};

/// A project with the editor's own face under `fonts/`, so the test does not
/// depend on what the machine running it happens to have installed.
fn app_with_a_font() -> (tempfile::TempDir, App) {
    app_with_a_scene("")
}

fn app_with_a_scene(scene: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"g\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::create_dir(dir.path().join("fonts")).unwrap();
    let face = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../editor/fonts/ui-SourceSans3-Regular.ttf");
    std::fs::copy(&face, dir.path().join("fonts/ui-SourceSans3-Regular.ttf")).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    // The main scene, and the assets it declares, arrive on the first tick.
    app.tick(1.0 / 60.0);
    (dir, app)
}

fn word(app: &App, text: &str, size: f32) -> MeshData {
    let shape = TextShape {
        text: text.to_string(),
        font: String::new(),
        size,
        weight: 400,
        italic: false,
    };
    let definition = MeshData {
        text: Some(shape),
        ..MeshData::default()
    };
    balaur_core::mesh::load_from(&app.engine, &definition).expect("the word has an outline")
}

/// Twice the area the triangles cover: what tells a filled ring from a
/// filled disc without looking at a single vertex.
fn area(mesh: &MeshData) -> f32 {
    mesh.indices
        .iter()
        .map(|&[a, b, c]| {
            let at = |i: u32| mesh.positions[i as usize];
            let (a, b, c) = (at(a), at(b), at(c));
            (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])
        })
        .sum::<f32>()
        .abs()
        / 2.0
}

fn bounds(mesh: &MeshData) -> ([f32; 3], [f32; 3]) {
    mesh.bounds().expect("a word has vertices")
}

#[test]
fn a_word_becomes_triangles_in_the_plane() {
    let (_dir, app) = app_with_a_font();
    let mesh = word(&app, "balaur", 1.0);
    assert!(mesh.indices.len() > 20, "{} triangles", mesh.indices.len());
    assert!(mesh.positions.iter().all(|p| p[2] == 0.0), "not flat");
    let normals = mesh.normals.as_ref().expect("normals");
    // A filled outline faces +z exactly; the value is written, not computed.
    #[allow(clippy::float_cmp, reason = "a placed normal, not an arithmetic one")]
    let facing = normals.iter().all(|n| *n == [0.0, 0.0, 1.0]);
    assert!(facing, "a filled word faces the camera");
    assert_eq!(
        mesh.uvs.as_ref().map(Vec::len),
        Some(mesh.positions.len()),
        "one uv per vertex"
    );
}

/// The cache is keyed by what the request says, so asking twice is the same
/// geometry and not merely a similar one.
#[test]
fn the_same_word_comes_back_the_same() {
    let (_dir, app) = app_with_a_font();
    assert_eq!(word(&app, "balaur", 1.0), word(&app, "balaur", 1.0));
}

/// A letter with a counter is a ring: filled over, an `o` would cover its
/// whole box, and it covers well under half of it.
#[test]
fn a_counter_stays_a_hole() {
    let (_dir, app) = app_with_a_font();
    let mesh = word(&app, "o", 1.0);
    let (min, max) = bounds(&mesh);
    let box_area = (max[0] - min[0]) * (max[1] - min[1]);
    let covered = area(&mesh);
    assert!(
        covered < 0.75 * box_area,
        "an o covered {covered} of a {box_area} box, so its counter was filled in"
    );
    assert!(covered > 0.2 * box_area, "an o covered almost nothing");
}

/// Size is world units, and the mesh is measured in them: a word set twice
/// as large is twice as wide.
#[test]
fn a_word_is_sized_in_world_units() {
    let (_dir, app) = app_with_a_font();
    let (small_min, small_max) = bounds(&word(&app, "balaur", 1.0));
    let (big_min, big_max) = bounds(&word(&app, "balaur", 2.0));
    let width = |min: [f32; 3], max: [f32; 3]| max[0] - min[0];
    let small = width(small_min, small_max);
    let big = width(big_min, big_max);
    assert!(small > 1.0, "six letters at one unit came to {small}");
    assert!(
        (big / small - 2.0).abs() < 0.05,
        "twice the size gave {big} against {small}"
    );
}

/// The mesh sits on its baseline, which is what lets a caller place a word
/// the way a font places one.
#[test]
fn a_word_sits_on_its_baseline() {
    let (_dir, app) = app_with_a_font();
    let (min, max) = bounds(&word(&app, "balaur", 1.0));
    assert!(min[1] < 0.05, "the descender of a b should reach the line");
    assert!(max[1] > 0.4, "an ascender should stand well above it");
}

#[test]
fn a_word_nothing_can_draw_says_so() {
    let (_dir, app) = app_with_a_font();
    let definition = MeshData {
        text: Some(TextShape {
            text: "   ".to_string(),
            size: 1.0,
            weight: 400,
            ..TextShape::default()
        }),
        ..MeshData::default()
    };
    let refused = balaur_core::mesh::load_from(&app.engine, &definition);
    assert!(refused.is_err(), "blank text is not geometry");
}

/// The whole point of building it headless: a scene names a word and the
/// triangles a collider would be fitted to are already there.
#[test]
fn a_word_is_a_mesh_asset_a_scene_can_name() {
    let (_dir, app) = app_with_a_font();
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        r##"
[[assets]]
id = "title"
type = "mesh"
kind = "text"
text = "hi"
size = 1.0

[[nodes]]
id = "sign"
name = "Sign"
mesh = { source = "#title" }
"##,
        root,
        false,
    )
    .expect("the scene loads");
    let definition = balaur_core::assets::load_typed::<MeshData>(&app.engine, "#title")
        .expect("the scene declared a text mesh");
    let mesh = balaur_core::mesh::load_from(&app.engine, &definition).expect("it shapes");
    assert!(!mesh.indices.is_empty(), "a named word has triangles");
    let (min, max) = mesh.bounds().expect("bounds");
    assert!(max[0] - min[0] > 0.4, "two letters are wider than that");
}
