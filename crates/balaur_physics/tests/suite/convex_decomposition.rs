//! The 2D `convex_decomposition` collider: exact convex pieces that overlap
//! across their seams, so a thin body cannot wedge into one.

use balaur::{AppConfig, standard_app};
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

/// The table on one node, with whatever extra collider keys a test wants.
fn table_app(extra: &str) -> App {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        format!(
            r##"{TABLE}
[[nodes]]
id = "n_table"
name = "Table"

[nodes.body2d]
kind = "dynamic"

[nodes.collider2d]
kind = "convex_decomposition"
mesh = "#table"
{extra}
"##
        ),
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
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
    let plain = table_app("overlap = 0.0");
    let pieces = pieces(&plain);
    assert_eq!(pieces.len(), 3, "the table did not cut into three pieces");
    let covered = total_area(&pieces);
    assert!((covered - 1.0).abs() < 1.0e-3, "{covered} covered, not 1.0");
}

/// The overlap is what stops a beam wedging into a seam, so it has to reach
/// the shapes the solver actually sees.
#[test]
fn the_pieces_grow_into_each_other() {
    let plain = total_area(&pieces(&table_app("overlap = 0.0")));
    let grown = total_area(&pieces(&table_app("overlap = 0.9")));
    assert!(grown > plain + 0.05, "{grown} is no larger than {plain}");
}

/// Grown pieces share the ground they overlap on; counting it twice would
/// make a decomposed table heavier than the same table as a hull.
#[test]
fn the_pieces_weigh_what_the_polygon_weighs() {
    for overlap in ["0.0", "0.9", "1.0"] {
        let weighed = mass(&table_app(&format!("overlap = {overlap}")));
        assert!(
            (weighed - 1.0).abs() < 1.0e-2,
            "at overlap {overlap} the table weighs {weighed}, not 1.0"
        );
    }
}

/// Density scales it, and an explicit mass still wins.
#[test]
fn density_and_mass_still_say_what_it_weighs() {
    let dense = mass(&table_app("density = 3.0"));
    assert!(
        (dense - 3.0).abs() < 1.0e-2,
        "a denser table weighs {dense}"
    );
    let pinned = mass(&table_app("mass = 7.0"));
    assert!(
        (pinned - 7.0).abs() < 1.0e-2,
        "a pinned table weighs {pinned}"
    );
}

/// The approximate cut is still there for an outline too dense for the exact
/// one, and a border rounds whichever pieces it leaves.
#[test]
fn the_other_ways_to_cut_it_build() {
    for extra in [
        "method = \"vhacd\"",
        "method = \"vhacd\"\nresolution = 32.0\nmax_convex_hulls = 8.0",
        "edge_radius = 0.02",
    ] {
        let app = table_app(extra);
        let state = app.engine.resource::<PhysicsState2d>();
        let state = state.borrow();
        assert!(
            !state.colliders.is_empty(),
            "no collider was built for {extra}"
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
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"N\"\nscript = { source = \"scripts/s.rn\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        r#"pub fn init(this) {
    let l = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [0.0, 2.0]];
    let plain = geometry2d::convex_decomposition(l, #{ overlap: 0.0 });
    assert!(plain.len() == 2, "an L should cut into two pieces");
    let grown = geometry2d::convex_decomposition(l);
    assert!(grown.len() == 2, "the overlap changed the piece count");
    assert!(geometry2d::area(grown[0]) > geometry2d::area(plain[0]), "no piece grew");
    let hull = geometry2d::convex_hull(l);
    assert!(hull.len() == 5, "core's own geometry2d verbs are still there");
}
"#,
    )
    .unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let errors: Vec<String> = balaur_core::logbuf::recent(80)
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
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
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

[nodes.collider2d]
kind = "convex_decomposition"
mesh = "#table"
overlap = {overlap}

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
kind = "rectangle"
half_extents = [0.6, 0.02]
"##
        ),
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
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
