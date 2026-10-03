//! `multimesh3d` and `multimesh2d` over the `multimesh` asset, headless:
//! where each instance lands, what the node keeps of the asset, and the
//! handle a script drives it through.

use balaur::{AppConfig, standard_app};
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, GlobalTransform, Transform};
use balaur_core::{App, components};
use balaur_render::{MultiMesh, MultiMeshAsset, RenderPlugin};
use glamx::{Quat, Vec3};

use crate::LOG;

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(balaur_core::AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    (dir, app)
}

fn node(app: &App, name: &str, parent: Entity) -> Entity {
    scene::spawn_node(&mut app.engine.world_mut(), name, parent)
}

fn add(app: &App, entity: Entity, key: &str, params: &str) {
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, key, Some(&params)).unwrap();
}

/// A box drawn at each of `instances`, inline.
fn boxes(instances: &str) -> String {
    format!(
        "source = {{ type = \"multimesh\", mesh = {{ type = \"mesh\", kind = \"box\" }}, instances = [{instances}] }}"
    )
}

/// Where each drawn instance of `entity` sits in the world.
fn placed(app: &App, entity: Entity) -> Vec<Vec3> {
    let world = app.engine.world();
    let multimesh = world.get::<&MultiMesh>(entity).expect("a multimesh node");
    let global = world.get::<&GlobalTransform>(entity).expect("a posed node");
    multimesh
        .placed(&global)
        .iter()
        .map(|placed| placed.at.w_axis.truncate())
        .collect()
}

/// The first node anywhere in the tree called `name`.
fn named(app: &App, name: &str) -> Entity {
    let world = app.engine.world();
    let mut query = world.query::<(Entity, &scene::Name)>();
    query
        .iter()
        .find(|(_, n)| n.0 == name)
        .map_or_else(|| panic!("no node called {name}"), |(entity, _)| entity)
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

fn same(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn an_instance_lands_where_its_transform_puts_it() {
    let (_dir, mut app) = app();
    let owner = node(&app, "Row", app.engine.root());
    add(
        &app,
        owner,
        "multimesh3d",
        &boxes("{ position = [0.0, 0.0, 0.0] }, { position = [2.0, 1.0, 0.0] }"),
    );
    app.tick(1.0 / 60.0);
    let at = placed(&app, owner);
    assert_eq!(at.len(), 2);
    assert!(near(at[0], Vec3::ZERO), "{at:?}");
    assert!(near(at[1], Vec3::new(2.0, 1.0, 0.0)), "{at:?}");
}

/// An instance's transform is in its node's space, so turning and moving the
/// node carries every instance with it.
#[test]
fn turning_the_node_turns_its_instances() {
    let (_dir, mut app) = app();
    let owner = node(&app, "Row", app.engine.root());
    add(
        &app,
        owner,
        "multimesh3d",
        &boxes("{ position = [2.0, 0.0, 0.0] }"),
    );
    if let Ok(mut transform) = app.engine.world_mut().get::<&mut Transform>(owner) {
        transform.position = Vec3::new(0.0, 0.0, 5.0);
        transform.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    }
    app.tick(1.0 / 60.0);
    let at = placed(&app, owner);
    assert!(near(at[0], Vec3::new(0.0, 0.0, 3.0)), "{at:?}");
}

#[test]
fn an_instances_colour_and_custom_data_reach_the_draw() {
    let (_dir, mut app) = app();
    let owner = node(&app, "Row", app.engine.root());
    add(
        &app,
        owner,
        "multimesh3d",
        &boxes("{ color = \"#ff000080\", custom = [1.0, 2.0, 3.0, 4.0] }"),
    );
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let multimesh = world.get::<&MultiMesh>(owner).unwrap();
    let global = world.get::<&GlobalTransform>(owner).unwrap();
    let drawn = multimesh.placed(&global);
    assert!(same(drawn[0].color, [1.0, 0.0, 0.0, 128.0 / 255.0]));
    assert!(same(drawn[0].custom, [1.0, 2.0, 3.0, 4.0]));
}

#[test]
fn an_empty_list_draws_nothing() {
    let (_dir, mut app) = app();
    let owner = node(&app, "Row", app.engine.root());
    add(&app, owner, "multimesh3d", &boxes(""));
    app.tick(1.0 / 60.0);
    assert!(placed(&app, owner).is_empty());
    balaur_render::stats::measure(&app.engine);
    let stats = app.engine.resource::<balaur_render::stats::Stats>();
    assert!(
        stats.borrow().by_node.is_empty(),
        "an empty multimesh costs nothing"
    );
}

/// A child is not an instance: it draws once, where it is.
#[test]
fn a_child_draws_once() {
    let (_dir, mut app) = app();
    let owner = node(&app, "Row", app.engine.root());
    add(
        &app,
        owner,
        "multimesh3d",
        &boxes(
            "{ position = [0.0, 0.0, 0.0] }, { position = [1.0, 0.0, 0.0] }, { position = [2.0, 0.0, 0.0] }",
        ),
    );
    let child = node(&app, "Label", owner);
    add(
        &app,
        child,
        "shape3d",
        "kind = \"box\"\nsize = [1.0, 1.0, 1.0]",
    );
    app.tick(1.0 / 60.0);
    balaur_render::stats::measure(&app.engine);
    let stats = app.engine.resource::<balaur_render::stats::Stats>();
    let stats = stats.borrow();
    let copies = |name: &str| {
        stats
            .by_node
            .iter()
            .find(|(path, _)| path.ends_with(name))
            .map(|(_, cost)| cost.copies)
    };
    assert_eq!(copies("Row"), Some(3));
    assert_eq!(copies("Label"), Some(1));
}

#[test]
fn visible_instance_count_cuts_the_draw() {
    let (_dir, mut app) = app();
    let owner = node(&app, "Row", app.engine.root());
    add(
        &app,
        owner,
        "multimesh3d",
        "source = { type = \"multimesh\", mesh = { type = \"mesh\", kind = \"box\" }, visible_instance_count = 1, instances = [{}, { position = [1.0, 0.0, 0.0] }] }",
    );
    app.tick(1.0 / 60.0);
    assert_eq!(placed(&app, owner).len(), 1);
    let world = app.engine.world();
    assert_eq!(world.get::<&MultiMesh>(owner).unwrap().instances.len(), 2);
}

/// Two nodes naming one asset read one parsed definition, and each keeps a
/// copy of its instances: a change to one node's leaves the other's alone.
#[test]
fn two_nodes_share_one_asset_and_part_ways_once_one_is_changed() {
    let (dir, mut app) = app();
    std::fs::create_dir_all(dir.path().join("multimeshes")).unwrap();
    std::fs::write(
        dir.path().join("multimeshes/posts.toml"),
        "type = \"multimesh\"\nmesh = { type = \"mesh\", kind = \"box\" }\ninstances = [{ position = [1.0, 0.0, 0.0] }]\n",
    )
    .unwrap();
    let a = node(&app, "A", app.engine.root());
    let b = node(&app, "B", app.engine.root());
    add(
        &app,
        a,
        "multimesh3d",
        "source = \"multimeshes/posts.toml\"",
    );
    add(
        &app,
        b,
        "multimesh3d",
        "source = \"multimeshes/posts.toml\"",
    );
    let first =
        balaur_core::assets::load_typed::<MultiMeshAsset>(&app.engine, "multimeshes/posts.toml")
            .unwrap();
    let second =
        balaur_core::assets::load_typed::<MultiMeshAsset>(&app.engine, "multimeshes/posts.toml")
            .unwrap();
    assert!(std::rc::Rc::ptr_eq(&first, &second), "one parse, shared");
    app.engine
        .world_mut()
        .get::<&mut MultiMesh>(a)
        .unwrap()
        .instances[0]
        .position = Vec3::new(9.0, 0.0, 0.0);
    app.tick(1.0 / 60.0);
    assert!(near(placed(&app, a)[0], Vec3::new(9.0, 0.0, 0.0)));
    assert!(near(placed(&app, b)[0], Vec3::new(1.0, 0.0, 0.0)));
    assert!(near(first.instances[0].position, Vec3::new(1.0, 0.0, 0.0)));
}

/// A patch of another key keeps what was done to the node's instances; a new
/// source copies the new asset's.
#[test]
fn a_patch_that_keeps_the_source_keeps_the_nodes_instances() {
    let (_dir, mut app) = app();
    let owner = node(&app, "Row", app.engine.root());
    add(
        &app,
        owner,
        "multimesh3d",
        &boxes("{ position = [1.0, 0.0, 0.0] }"),
    );
    app.engine
        .world_mut()
        .get::<&mut MultiMesh>(owner)
        .unwrap()
        .instances[0]
        .position = Vec3::new(4.0, 0.0, 0.0);
    let patch: toml::Value = toml::from_str("cast_shadow = false").unwrap();
    components::patch(&app.engine, owner, "multimesh3d", &patch).unwrap();
    app.tick(1.0 / 60.0);
    assert!(near(placed(&app, owner)[0], Vec3::new(4.0, 0.0, 0.0)));
    let patch: toml::Value = toml::from_str(&boxes("{ position = [7.0, 0.0, 0.0] }")).unwrap();
    components::patch(&app.engine, owner, "multimesh3d", &patch).unwrap();
    app.tick(1.0 / 60.0);
    assert!(near(placed(&app, owner)[0], Vec3::new(7.0, 0.0, 0.0)));
}

/// What a scene saves: the component, not a `mesh` or a `polygon` beside it.
#[test]
fn a_multimesh_reads_back_as_itself_and_nothing_else() {
    let (_dir, mut app) = app();
    let deep = node(&app, "Deep", app.engine.root());
    let flat = node(&app, "Flat", app.engine.root());
    add(&app, deep, "multimesh3d", &boxes("{}"));
    add(
        &app,
        flat,
        "multimesh2d",
        "source = { type = \"multimesh\", mesh = { type = \"mesh\", positions = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], indices = [[0, 1, 2]] }, instances = [{}] }\ncolor = \"#00ff00\"",
    );
    app.tick(1.0 / 60.0);
    let eng = &app.engine;
    let got = components::get(eng, deep, "multimesh3d").expect("the 3D component");
    assert!(got["source"].as_str().unwrap().starts_with("#!"));
    assert_eq!(got["cast_shadow"].as_bool(), Some(true));
    assert!(components::get(eng, deep, "mesh").is_none());
    assert!(components::get(eng, deep, "multimesh2d").is_none());
    let got = components::get(eng, flat, "multimesh2d").expect("the 2D component");
    assert_eq!(got["color"][1].as_float(), Some(1.0));
    assert!(components::get(eng, flat, "polygon").is_none());
    assert!(components::get(eng, flat, "multimesh3d").is_none());
}

#[test]
fn a_2d_instance_turns_about_z_and_lands_in_the_plane() {
    let (_dir, mut app) = app();
    let owner = node(&app, "Flat", app.engine.root());
    add(
        &app,
        owner,
        "multimesh2d",
        "source = { type = \"multimesh\", mesh = { type = \"mesh\", positions = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], indices = [[0, 1, 2]] }, instances = [{ position = [3.0, 1.0], rotation_euler = [0.0, 0.0, 1.5707964] }] }",
    );
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let multimesh = world.get::<&MultiMesh>(owner).unwrap();
    let global = world.get::<&GlobalTransform>(owner).unwrap();
    let at = multimesh.placed(&global)[0].at;
    assert!(near(at.w_axis.truncate(), Vec3::new(3.0, 1.0, 0.0)));
    assert!(
        near(at.x_axis.truncate(), Vec3::Y),
        "x turned onto y: {:?}",
        at.x_axis
    );
}

#[test]
fn a_missing_asset_draws_nothing_rather_than_failing_the_node() {
    let (_dir, mut app) = app();
    let owner = node(&app, "Row", app.engine.root());
    add(
        &app,
        owner,
        "multimesh3d",
        "source = \"multimeshes/nowhere.toml\"",
    );
    app.tick(1.0 / 60.0);
    assert!(placed(&app, owner).is_empty());
}

/// A project whose main scene holds one `multimesh` asset and the two nodes
/// that draw it; the first carries `script`.
fn run_shared(script: &str, frames: usize) -> (App, Vec<String>, Vec<String>) {
    run_shared_as("multimesh3d", script, frames)
}

/// `run_shared` with the two nodes drawing through `component`.
fn run_shared_as(component: &str, script: &str, frames: usize) -> (App, Vec<String>, Vec<String>) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::create_dir_all(root.join("multimeshes")).unwrap();
    std::fs::write(
        root.join("project.toml"),
        "[application]\nname = \"m\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.toml"),
        r##"[[assets]]
id = "posts"
type = "multimesh"
mesh = { type = "mesh", kind = "box" }
instances = [{ position = [0.0, 0.0, 0.0] }, { position = [1.0, 0.0, 0.0] }]

[[nodes]]
id = "w"
name = "World"

[[nodes]]
id = "a"
name = "A"
parent = "w"
script = { source = "scripts/s.rn" }

[nodes.COMPONENT]
source = "#posts"

[[nodes]]
id = "b"
name = "B"
parent = "w"

[nodes.COMPONENT]
source = "#posts"
"##
        .replace("COMPONENT", component),
    )
    .unwrap();
    std::fs::write(root.join("scripts/s.rn"), script).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(root.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
    let lines = balaur_core::logbuf::recent(80);
    let errors = lines
        .iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message.clone())
        .collect();
    let all = lines.into_iter().map(|e| e.message).collect();
    drop(dir);
    (app, errors, all)
}

/// The instance a script moves every frame reads back where it was put, and
/// the other node drawing the same asset keeps its own.
#[test]
fn a_script_moves_an_instance_every_frame_and_reads_it_back() {
    let (app, errors, lines) = run_shared(
        r"pub fn update(this, dt) {
    let mm = this.node.multimesh3d;
    let x = mm.instance_transform(1).to_scale_rotation_translation().2.x + 1.0;
    mm.set_instance_transform(1, balaur::Transform3d::from_translation(balaur::Vec3::new(x, 0.0, 0.0)));
    mm.set_instance_color(0, balaur::Color::new(1.0, 0.0, 0.0, 1.0));
    log::info(`moved ${x} of ${mm.instance_count()}`);
}
",
        3,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        lines.iter().any(|l| l.contains("moved 4.0 of 2")),
        "expected three moves from 1, got {lines:#?}"
    );
    let a = named(&app, "A");
    let b = named(&app, "B");
    let world = app.engine.world();
    let moved = world.get::<&MultiMesh>(a).unwrap();
    let kept = world.get::<&MultiMesh>(b).unwrap();
    assert!(near(moved.instances[1].position, Vec3::new(4.0, 0.0, 0.0)));
    assert!(same(moved.instances[0].color, [1.0, 0.0, 0.0, 1.0]));
    assert!(near(kept.instances[1].position, Vec3::new(1.0, 0.0, 0.0)));
    assert!(same(kept.instances[0].color, [1.0; 4]));
}

#[test]
fn the_count_grows_with_plain_instances_and_keeps_those_below_it() {
    let (app, errors, lines) = run_shared(
        r"pub fn init(this) {
    let mm = this.node.multimesh3d;
    mm.set_instance_count(4);
    let grown = mm.instances();
    mm.set_instance_count(1);
    mm.set_visible_instance_count(0);
    log::info(`grown ${grown.len()} ${grown[1].position[0]} ${grown[3].scale[1]} now ${mm.instance_count()} showing ${mm.visible_instance_count()}`);
}
",
        1,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("grown 4 1.0 1.0 now 1 showing 0")),
        "{lines:#?}"
    );
    let a = named(&app, "A");
    assert!(
        app.engine
            .world()
            .get::<&MultiMesh>(a)
            .unwrap()
            .drawn()
            .is_empty()
    );
}

#[test]
fn an_index_past_the_end_is_an_error_that_names_the_count() {
    let (_app, errors, _) = run_shared(
        r"pub fn init(this) {
    this.node.multimesh3d.set_instance_color(5, balaur::Color::new(1.0, 1.0, 1.0, 1.0));
}
",
        1,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("instance 5") && e.contains("holds 2")),
        "{errors:#?}"
    );
}

/// `instances()` hands back the asset's own shape, so a list read off one
/// node sets another's whole.
#[test]
fn instances_read_off_one_node_set_another_whole() {
    let (app, errors, _) = run_shared(
        r#"pub fn init(this) {
    let mine = this.node.multimesh3d.instances();
    mine[0].position = [0.0, 5.0, 0.0];
    mine[1].custom = [1.0, 2.0, 3.0, 4.0];
    this.node.get_node("../B").multimesh3d.set_instances(mine);
}
"#,
        1,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let b = named(&app, "B");
    let world = app.engine.world();
    let multimesh = world.get::<&MultiMesh>(b).unwrap();
    assert!(near(
        multimesh.instances[0].position,
        Vec3::new(0.0, 5.0, 0.0)
    ));
    assert!(same(multimesh.instances[1].custom, [1.0, 2.0, 3.0, 4.0]));
}

/// A shear a script hands an instance is kept as its basis, and a 2D
/// instance's texture rectangle reads back as it was set.
#[test]
fn a_script_keeps_a_shear_and_sets_a_texture_rectangle() {
    let (app, errors, _) = run_shared(
        r"pub fn init(this) {
    let sheared = balaur::Transform3d::from_cols(
        balaur::Vec3::new(1.0, 0.0, 0.0),
        balaur::Vec3::new(0.5, 1.0, 0.0),
        balaur::Vec3::new(0.0, 0.0, 1.0),
        balaur::Vec3::new(2.0, 0.0, 0.0),
    );
    this.node.multimesh3d.set_instance_transform(0, sheared);
}
",
        1,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let a = named(&app, "A");
    let basis = app.engine.world().get::<&MultiMesh>(a).unwrap().instances[0].basis;
    assert!(
        basis.is_some_and(|b| (b.y_axis.x - 0.5).abs() < 1e-5),
        "{basis:?}"
    );

    let (_app, errors, lines) = run_shared_as(
        "multimesh2d",
        r"pub fn init(this) {
    let mm = this.node.multimesh2d;
    mm.set_instance_region(1, [8.0, 16.0], [32.0, 24.0]);
    let region = mm.instance_region(1);
    log::info(`region ${region[1]} ${region[3]} ${mm.instance_region(0) == ()}`);
}
",
        1,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        lines.iter().any(|l| l.contains("region 16.0 24.0 true")),
        "{lines:#?}"
    );
}
