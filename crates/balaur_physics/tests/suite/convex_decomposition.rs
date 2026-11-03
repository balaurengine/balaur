//! The 2D `convex_decomposition` collider: exact convex pieces that overlap
//! across their seams, so a thin body cannot wedge into one.

use balaur::standard_app;
use balaur_core::App;
use balaur_core::scene::{Transform, find_node};
use balaur_physics::PhysicsState2d;

use crate::LOG;

/// A table, concave: a top from y 1 to 1.2 over two legs. Its area is 1.0, so
/// a collider at the default density should weigh exactly that.
const TABLE: &str = r#"[[assets]]
id = "table"
type = "mesh"
positions = [[0.0, 0.0], [0.3, 0.0], [0.3, 1.0], [1.7, 1.0], [1.7, 0.0], [2.0, 0.0], [2.0, 1.2], [0.0, 1.2]]
"#;

/// The table on one node, with whatever extra `collider2d` rows and
/// `convex_decomposition2d` rows a test wants.
fn table_app(collider: &str, decomposition: &str) -> App {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    // A row of the shape's own comes in its `shape`, which the default
    // otherwise names.
    let shape = if collider.starts_with("shape") {
        String::new()
    } else {
        r##"shape = { kind = "convex_decomposition", mesh = "#table" }"##.to_owned()
    };
    std::fs::write(
        dir.path().join("main.toml"),
        format!(
            r#"{TABLE}
[[nodes]]
id = "n_table"
name = "Table"

[nodes.body2d]
kind = "dynamic"

[nodes.collider2d]
{shape}
{collider}

[nodes."convex_decomposition2d"]
{decomposition}
"#
        ),
    )
    .unwrap();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    app
}

/// Every piece of the one collider the table node carries, as point rings.
fn pieces(app: &App) -> Vec<Vec<[f32; 2]>> {
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handle = *state
        .colliders
        .values()
        .next()
        .expect("the table has no collider")
        .first()
        .unwrap();
    let shape = state.world.colliders[handle].shape();
    let compound = shape.as_compound().expect("the collider is not a compound");
    compound
        .shapes()
        .iter()
        .map(|(_, piece)| {
            let ring = piece
                .as_convex_polygon()
                .expect("a piece is not a convex polygon");
            ring.points().iter().map(|p| [p.x, p.y]).collect()
        })
        .collect()
}

fn area(ring: &[[f32; 2]]) -> f32 {
    let mut sum = 0.0;
    for at in 0..ring.len() {
        let (here, next) = (ring[at], ring[(at + 1) % ring.len()]);
        sum += here[0] * next[1] - next[0] * here[1];
    }
    sum.abs() / 2.0
}

fn total_area(pieces: &[Vec<[f32; 2]>]) -> f32 {
    pieces.iter().map(|piece| area(piece)).sum()
}

/// The mass of the table's one collider.
fn mass(app: &App) -> f32 {
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handle = *state.colliders.values().next().unwrap().first().unwrap();
    state.world.colliders[handle].mass()
}

#[test]
fn a_concave_polygon_becomes_convex_pieces() {
    let plain = table_app("", "method = { kind = \"exact\", overlap = 0.0 }");
    let pieces = pieces(&plain);
    assert_eq!(pieces.len(), 3, "the table did not cut into three pieces");
    let covered = total_area(&pieces);
    assert!((covered - 1.0).abs() < 1.0e-3, "{covered} covered, not 1.0");
}

/// The overlap is what stops a beam wedging into a seam, so it has to reach
/// the shapes the solver actually sees.
#[test]
fn the_pieces_grow_into_each_other() {
    let plain = total_area(&pieces(&table_app(
        "",
        "method = { kind = \"exact\", overlap = 0.0 }",
    )));
    let grown = total_area(&pieces(&table_app(
        "",
        "method = { kind = \"exact\", overlap = 0.9 }",
    )));
    assert!(grown > plain + 0.05, "{grown} is no larger than {plain}");
}

/// Grown pieces share the ground they overlap on; counting it twice would
/// make a decomposed table heavier than the same table as a hull.
#[test]
fn the_pieces_weigh_what_the_polygon_weighs() {
    for overlap in ["0.0", "0.9", "1.0"] {
        let weighed = mass(&table_app(
            "",
            &format!("method = {{ kind = \"exact\", overlap = {overlap} }}"),
        ));
        assert!(
            (weighed - 1.0).abs() < 1.0e-2,
            "at overlap {overlap} the table weighs {weighed}, not 1.0"
        );
    }
}

/// Density scales it, and an explicit mass still wins.
#[test]
fn density_and_mass_still_say_what_it_weighs() {
    let dense = mass(&table_app("density = 3.0", ""));
    assert!(
        (dense - 3.0).abs() < 1.0e-2,
        "a denser table weighs {dense}"
    );
    let pinned = mass(&table_app("mass = 7.0", ""));
    assert!(
        (pinned - 7.0).abs() < 1.0e-2,
        "a pinned table weighs {pinned}"
    );
}

/// The approximate cut is still there for an outline too dense for the exact
/// one, and a border rounds whichever pieces it leaves.
#[test]
fn the_other_ways_to_cut_it_build() {
    for (collider, decomposition) in [
        ("", "method = { kind = \"vhacd\" }"),
        (
            "",
            "method = { kind = \"vhacd\", resolution = 32, max_convex_hulls = 8 }",
        ),
        (
            r##"shape = { kind = "convex_decomposition", mesh = "#table", edge_radius = 0.02 }"##,
            "",
        ),
    ] {
        let app = table_app(collider, decomposition);
        let state = app.engine.resource::<PhysicsState2d>();
        let state = state.borrow();
        assert!(
            !state.colliders.is_empty(),
            "no collider was built for {collider}{decomposition}"
        );
    }
}

/// The pieces a script can draw, tune, or hand back as hulls of its own.
#[test]
fn a_script_can_ask_for_the_pieces() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"scenario\"\nname = \"N\"\nscript = { source = \"scripts/scenario.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/scenario.bs"),
        r#"pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        let l = [vec2(0.0, 0.0), vec2(2.0, 0.0), vec2(2.0, 1.0), vec2(1.0, 1.0), vec2(1.0, 2.0), vec2(0.0, 2.0)];
        let plain = geometry2d::convex_decomposition(l, geometry2d::DecompositionOptions { overlap: 0.0 });
        if plain.len() != 2 { log::error("an L should cut into two pieces"); }
        let grown = geometry2d::convex_decomposition(l);
        if grown.len() != 2 { log::error("the overlap changed the piece count"); }
        if geometry2d::area(grown[0]) <= geometry2d::area(plain[0]) { log::error("no piece grew"); }
        let hull = geometry2d::convex_hull(l);
        if hull.len() != 5 { log::error("core's own geometry2d verbs are still there"); }
    }
}
"#,
    )
    .unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let errors: Vec<String> = balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

/// Catto's own scenario, end to end: a beam laid deeply inside the table
/// across a seam. Plain pieces push it opposite ways and it never leaves;
/// grown ones agree, and it is out of the table in three seconds.
///
/// The table's top face is y 1.2, and the beam starts at y 1.02.
fn wedged_beam(overlap: &str) -> f32 {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        format!(
            r##"{TABLE}
[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_table"
name = "Table"
parent = "n_world"

[nodes."convex_decomposition2d"]
method = {{ kind = "exact", overlap = {overlap} }}

[nodes.collider2d]
shape = {{ kind = "convex_decomposition", mesh = "#table" }}

[[nodes]]
id = "n_beam"
name = "Beam"
parent = "n_world"

[nodes.transform]
position = [0.25, 1.02, 0.0]

[nodes.body2d]
kind = "dynamic"
gravity_scale = 0.0

[nodes.collider2d]
shape = {{ kind = "rectangle", size = [1.2, 0.04] }}
"##
        ),
    )
    .unwrap();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    for _ in 0..180 {
        app.tick(1.0 / 60.0);
    }
    let world = app.engine.world();
    let root = app.engine.root();
    let beam = find_node(&world, root, "World/Beam").expect("no beam");
    world.get::<&Transform>(beam).unwrap().position.y
}

#[test]
fn a_beam_wedged_in_a_seam_is_pushed_out() {
    let stuck = wedged_beam("0.0");
    assert!(
        stuck < 1.1,
        "without the overlap the beam should still be wedged, and it reached {stuck}"
    );
    let freed = wedged_beam("0.9");
    assert!(
        freed > 1.2,
        "the grown pieces should have pushed the beam clear of the table, not to {freed}"
    );
}

#[test]
fn a_vhacd_cut_defaults_to_the_numbers_rapier2d_uses() {
    let defaults = {
        let mut app = App::new(balaur_core::AppConfig::bare(".")).unwrap();
        balaur_plugin::load(&mut app, &mut balaur_physics::PhysicsPlugin).unwrap();
        let registry = app
            .engine
            .resource::<balaur_core::components::ComponentRegistry>();
        let registry = registry.borrow();
        let schema = &registry
            .def("convex_decomposition2d")
            .expect("convex_decomposition2d is registered")
            .schema;
        // The tuning sits in the vhacd way of the `method` variant.
        let default = |key: &str| {
            schema
                .get("method")
                .and_then(|method| method.get("variants"))
                .and_then(|ways| ways.get("vhacd"))
                .and_then(|vhacd| vhacd.get("fields"))
                .and_then(|fields| fields.get(key))
                .and_then(|row| row.get("default"))
                .and_then(balaur_core::components::as_f64)
                .unwrap_or_else(|| panic!("convex_decomposition2d.{key} declares a default"))
        };
        (default("resolution"), default("max_concavity"))
    };
    assert!(
        (defaults.0 - 256.0).abs() < 1e-9,
        "resolution shows {}",
        defaults.0
    );
    assert!(
        (defaults.1 - 0.1).abs() < 1e-6,
        "max_concavity shows {}",
        defaults.1
    );

    let left = pieces(&table_app("", "method = { kind = \"vhacd\" }"));
    let stated = pieces(&table_app(
        "",
        "method = { kind = \"vhacd\", resolution = 256, max_concavity = 0.1 }",
    ));
    assert_eq!(
        left.len(),
        stated.len(),
        "the defaults cut differently from 256 and 0.1"
    );
    assert!((total_area(&left) - total_area(&stated)).abs() < 1e-5);
}
