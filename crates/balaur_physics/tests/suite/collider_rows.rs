//! The collider keys and readers rapier and parry have and `collider3d`,
//! `collider2d` now reach: capsule axes, rounded convex kinds, the VHACD and
//! voxel keys, mesh cleanup, holes, a collider's own mass, the layer test
//! mode, and the readers behind them.

use balaur_core::hecs::Entity;
use balaur_core::scene;
use balaur_core::{App, AppConfig, components};
use balaur_physics::{PhysicsPlugin, PhysicsState2d, PhysicsState3d};
use balaur_script::Value;
use std::fmt::Write as _;

use crate::boot::{self, Booted, entry, float, floats, number};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app
}

fn node_with(app: &App, name: &str, components: &[(&str, &str)]) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    for (component, params) in components {
        let params: toml::Value = toml::from_str(params).unwrap();
        components::add(&app.engine, e, component, Some(&params)).unwrap();
    }
    e
}

/// The first collider on `node`, in the 3D world.
fn collider3d<R>(
    app: &App,
    node: Entity,
    f: impl FnOnce(&balaur_physics::rapier3d::prelude::Collider) -> R,
) -> R {
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    f(&state.world.colliders[state.colliders[&node][0]])
}

fn collider2d<R>(
    app: &App,
    node: Entity,
    f: impl FnOnce(&balaur_physics::rapier2d::prelude::Collider) -> R,
) -> R {
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    f(&state.world.colliders[state.colliders[&node][0]])
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn a_capsule_lies_along_its_up_axis_or_runs_between_its_ends() {
    let app = app();
    let along_x = node_with(
        &app,
        "AlongX",
        &[(
            "collider3d",
            "kind = \"capsule\"\nradius = 0.5\nheight = 3.0\nup_axis = \"x\"",
        )],
    );
    let ends = node_with(
        &app,
        "Ends",
        &[(
            "collider3d",
            "kind = \"capsule\"\nradius = 0.25\nheight = 0.0\na = [0.0, 0.0, 0.0]\nb = [0.0, 1.0, 1.0]",
        )],
    );
    let segment = collider3d(&app, along_x, |c| c.shape().as_capsule().unwrap().segment);
    assert_eq!((segment.a.x, segment.b.x), (-1.0, 1.0), "{segment:?}");
    let back = components::get(&app.engine, along_x, "collider3d").unwrap();
    assert_eq!(back.get("up_axis").and_then(toml::Value::as_str), Some("x"));
    assert!((float(&back, "height") - 3.0).abs() < 1e-5);
    let segment = collider3d(&app, ends, |c| c.shape().as_capsule().unwrap().segment);
    assert_eq!(segment.b.to_array(), [0.0, 1.0, 1.0]);
    let back = components::get(&app.engine, ends, "collider3d").unwrap();
    assert_eq!(float(&back, "height"), 0.0);
    assert_eq!(floats(&back, "b"), [0.0, 1.0, 1.0]);
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn a_2d_capsule_lies_along_x_or_runs_between_its_ends() {
    let app = app();
    let along_x = node_with(
        &app,
        "AlongX",
        &[(
            "collider2d",
            "kind = \"capsule\"\nradius = 0.5\nheight = 3.0\nup_axis = \"x\"",
        )],
    );
    let ends = node_with(
        &app,
        "Ends",
        &[(
            "collider2d",
            "kind = \"capsule\"\nheight = 0.0\na = [0.0, 0.0]\nb = [2.0, 1.0]",
        )],
    );
    let segment = collider2d(&app, along_x, |c| c.shape().as_capsule().unwrap().segment);
    assert_eq!((segment.a.x, segment.b.x), (-1.0, 1.0));
    let back = components::get(&app.engine, along_x, "collider2d").unwrap();
    assert_eq!(back.get("up_axis").and_then(toml::Value::as_str), Some("x"));
    let segment = collider2d(&app, ends, |c| c.shape().as_capsule().unwrap().segment);
    assert_eq!(segment.b.to_array(), [2.0, 1.0]);
    let back = components::get(&app.engine, ends, "collider2d").unwrap();
    assert_eq!(floats(&back, "b"), [2.0, 1.0]);
}

/// A cube's corners and outward triangles, `half` from its centre to a face.
fn cube_mesh(half: f32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let h = half;
    let corners = vec![
        [-h, -h, -h],
        [h, -h, -h],
        [h, h, -h],
        [-h, h, -h],
        [-h, -h, h],
        [h, -h, h],
        [h, h, h],
        [-h, h, h],
    ];
    let faces = vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [2, 3, 7],
        [2, 7, 6],
        [1, 2, 6],
        [1, 6, 5],
        [0, 4, 7],
        [0, 7, 3],
    ];
    (corners, faces)
}

fn mesh_asset(id: &str, positions: &[[f32; 3]], indices: &[[u32; 3]]) -> String {
    format!(
        "[[assets]]\nid = \"{id}\"\ntype = \"mesh\"\npositions = {positions:?}\nindices = {indices:?}\n"
    )
}

fn cube(id: &str, half: f32) -> String {
    let (corners, faces) = cube_mesh(half);
    mesh_asset(id, &corners, &faces)
}

/// One child of a `World` root per collider table, named in order.
fn scene_of(assets: &str, colliders: &[(&str, &str)]) -> String {
    let mut scene = format!("{assets}\n[[nodes]]\nid = \"n_world\"\nname = \"World\"\n");
    for (name, collider) in colliders {
        let _ = write!(
            scene,
            "\n[[nodes]]\nid = \"n_{name}\"\nname = \"{name}\"\nparent = \"n_world\"\n{collider}\n"
        );
    }
    scene
}

fn booted(assets: &str, colliders: &[(&str, &str)]) -> Booted {
    let booted = boot::project(&scene_of(assets, colliders), "");
    let errors = boot::errors();
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    booted
}

#[test]
fn edge_radius_rounds_the_convex_kinds_and_convex_mesh_takes_the_mesh_as_given() {
    let b = booted(
        &cube("cube", 0.5),
        &[
            (
                "Hull",
                "collider3d = { kind = \"convex_hull\", mesh = \"#cube\", edge_radius = 0.1 }",
            ),
            (
                "Mesh",
                "collider3d = { kind = \"convex_mesh\", mesh = \"#cube\" }",
            ),
            (
                "RoundMesh",
                "collider3d = { kind = \"convex_mesh\", mesh = \"#cube\", edge_radius = 0.1 }",
            ),
            (
                "Pieces",
                "collider3d = { kind = \"convex_decomposition\", mesh = \"#cube\", edge_radius = 0.1, max_convex_hulls = 1 }",
            ),
        ],
    );
    let app = &b.app;
    assert!(collider3d(app, b.node("World/Hull"), |c| c
        .shape()
        .as_round_convex_polyhedron()
        .is_some()));
    let corners = collider3d(app, b.node("World/Mesh"), |c| {
        c.shape().as_convex_polyhedron().map(|p| p.points().len())
    });
    assert_eq!(
        corners,
        Some(8),
        "convex_mesh is the cube's own eight corners"
    );
    assert!(collider3d(app, b.node("World/RoundMesh"), |c| c
        .shape()
        .as_round_convex_polyhedron()
        .is_some()));
    let pieces = collider3d(app, b.node("World/Pieces"), |c| {
        let compound = c
            .shape()
            .as_compound()
            .expect("a decomposition is a compound");
        compound
            .shapes()
            .iter()
            .all(|(_, piece)| piece.as_round_convex_polyhedron().is_some())
    });
    assert!(pieces, "edge_radius left a decomposition's pieces sharp");
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn the_vhacd_keys_and_method_shape_a_3d_decomposition() {
    let b = booted(
        &cube("cube", 0.5),
        &[
            (
                "Fine",
                "collider3d = { kind = \"convex_decomposition\", mesh = \"#cube\" }",
            ),
            (
                "One",
                "collider3d = { kind = \"convex_decomposition\", mesh = \"#cube\", max_concavity = 1.0, max_convex_hulls = 1, resolution = 16, symmetry_bias = 0.1, revolution_bias = 0.1, plane_downsampling = 2, hull_downsampling = 2, approximate_hulls = false }",
            ),
            (
                "Voxels",
                "collider3d = { kind = \"convex_decomposition\", mesh = \"#cube\", method = \"voxels\", resolution = 8 }",
            ),
        ],
    );
    let pieces = |name: &str| {
        collider3d(&b.app, b.node(name), |c| {
            c.shape().as_compound().map(|s| s.shapes().len())
        })
    };
    assert!(
        pieces("World/Fine") > Some(1),
        "the default cut kept the cube whole"
    );
    assert_eq!(
        pieces("World/One"),
        Some(1),
        "a loose max_concavity still cut the cube"
    );
    let voxels = collider3d(&b.app, b.node("World/Voxels"), |c| {
        let compound = c.shape().as_compound().expect("voxel parts are a compound");
        compound
            .shapes()
            .iter()
            .all(|(_, part)| part.as_voxels().is_some())
    });
    assert!(
        voxels,
        "method = voxels built something other than voxel parts"
    );
    let back = b.get(b.node("World/One"), "collider3d");
    assert_eq!(float(&back, "max_convex_hulls"), 1.0);
    assert_eq!(
        back.get("approximate_hulls").and_then(toml::Value::as_bool),
        Some(false)
    );
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn voxel_size_resizes_an_asset_and_sizes_voxelized_points() {
    let b = booted(
        &(cube("cube", 0.5)
            + "\n[[assets]]\nid = \"cells\"\ntype = \"voxels\"\nsize = [1.0, 1.0, 1.0]\ncells = [[0, 0, 0], [1, 0, 0]]\n"),
        &[
            (
                "Grid",
                "collider3d = { kind = \"voxels\", voxels = \"#cells\", voxel_size = 0.5 }",
            ),
            (
                "Own",
                "collider3d = { kind = \"voxels\", voxels = \"#cells\" }",
            ),
            (
                "Points",
                "collider3d = { kind = \"voxelized_points\", mesh = \"#cube\", voxel_size = 0.5 }",
            ),
        ],
    );
    let size = |name: &str| {
        collider3d(&b.app, b.node(name), |c| {
            c.shape().as_voxels().unwrap().voxel_size().x
        })
    };
    assert_eq!(size("World/Grid"), 0.5);
    assert_eq!(size("World/Own"), 1.0, "0 keeps the asset's own size");
    assert_eq!(size("World/Points"), 0.5);
}

#[test]
fn fill_cavities_leaves_a_walled_off_hollow_empty() {
    // A cube with a smaller one walled up inside it, facing inwards.
    let hollow = {
        let (mut points, mut faces) = cube_mesh(1.0);
        let (inner, inner_faces) = cube_mesh(0.5);
        points.extend(inner);
        faces.extend(inner_faces.iter().map(|&[a, b, c]| [a + 8, c + 8, b + 8]));
        mesh_asset("hollow", &points, &faces)
    };
    let b = booted(
        &hollow,
        &[
            (
                "Filled",
                "collider3d = { kind = \"voxelized_mesh\", mesh = \"#hollow\", voxel_size = 0.25 }",
            ),
            (
                "Hollow",
                "collider3d = { kind = \"voxelized_mesh\", mesh = \"#hollow\", voxel_size = 0.25, fill_cavities = true }",
            ),
        ],
    );
    let count = |name: &str| {
        collider3d(&b.app, b.node(name), |c| {
            c.shape()
                .as_voxels()
                .unwrap()
                .voxels()
                .filter(|v| !v.state.is_empty())
                .count()
        })
    };
    assert!(
        count("World/Hollow") < count("World/Filled"),
        "fill_cavities filled the hollow: {} cells against {}",
        count("World/Hollow"),
        count("World/Filled")
    );
}

/// Two triangles of one quad on six points, two of them doubled, and a
/// third triangle naming the first one's points again.
const DOUBLED: &str = r#"[[assets]]
id = "quad"
type = "mesh"
positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]
indices = [[0, 2, 1], [3, 5, 4], [0, 2, 1]]
"#;

#[test]
fn the_triangle_mesh_cleanup_keys_reach_the_mesh() {
    let b = booted(
        DOUBLED,
        &[
            (
                "Raw",
                "collider3d = { kind = \"triangle_mesh\", mesh = \"#quad\", fix_internal_edges = false }",
            ),
            (
                "Merged",
                "collider3d = { kind = \"triangle_mesh\", mesh = \"#quad\", fix_internal_edges = false, merge_vertices = true }",
            ),
            (
                "Deduped",
                "collider3d = { kind = \"triangle_mesh\", mesh = \"#quad\", fix_internal_edges = false, drop_duplicate_triangles = true }",
            ),
            (
                "Topology",
                "collider3d = { kind = \"triangle_mesh\", mesh = \"#quad\", topology = true, connected_components = true, drop_bad_topology = true, drop_degenerate_triangles = true }",
            ),
            (
                "TwoSided",
                "collider3d = { kind = \"triangle_mesh\", mesh = \"#quad\", two_sided_edges = true }",
            ),
        ],
    );
    let mesh = |name: &str| {
        collider3d(&b.app, b.node(name), |c| {
            let mesh = c.shape().as_trimesh().unwrap();
            (
                mesh.vertices().len(),
                mesh.indices().len(),
                mesh.topology().is_some(),
                mesh.connected_components().is_some(),
                mesh.flags(),
            )
        })
    };
    assert_eq!(mesh("World/Raw").0, 6);
    assert_eq!(
        mesh("World/Merged").0,
        4,
        "merge_vertices kept the doubled points"
    );
    assert_eq!(
        mesh("World/Deduped").1,
        2,
        "drop_duplicate_triangles kept the repeat"
    );
    let topology = mesh("World/Topology");
    assert!(
        topology.2 && topology.3,
        "topology or connected components were not built"
    );
    assert!(
        mesh("World/TwoSided").4.contains(
            balaur_physics::rapier3d::prelude::TriMeshFlags::FIX_INTERNAL_EDGES_TWO_SIDED
        )
    );
    let back = b.get(b.node("World/Merged"), "collider3d");
    assert_eq!(
        back.get("merge_vertices").and_then(toml::Value::as_bool),
        Some(true)
    );
    assert!(
        back.get("weld_vertices").is_none(),
        "the old spelling is still read back"
    );
}

#[test]
fn a_polyline_takes_every_edge_of_its_mesh_when_asked() {
    let b = booted(
        DOUBLED,
        &[
            (
                "Chain",
                "collider3d = { kind = \"polyline\", mesh = \"#quad\" }",
            ),
            (
                "Edges",
                "collider3d = { kind = \"polyline\", mesh = \"#quad\", edges = \"mesh\" }",
            ),
        ],
    );
    let segments = |name: &str| {
        collider3d(&b.app, b.node(name), |c| {
            c.shape().as_polyline().unwrap().num_segments()
        })
    };
    assert_eq!(
        segments("World/Chain"),
        5,
        "a chain joins the six points in order"
    );
    assert_eq!(
        segments("World/Edges"),
        6,
        "every distinct edge of the triangles"
    );
}

const HOLED: &str = r#"[[assets]]
id = "ground"
type = "heightfield"
rows = 3
columns = 3
heights = [0, 0, 0, 0, 0, 0, 0, 0, 0]
holes = [[0, 1]]
"#;

#[test]
fn a_heightfield_loses_its_asset_holes_and_the_ones_a_script_cuts() {
    let mut b = boot::project(
        &(scene_of(
            HOLED,
            &[(
                "Ground",
                "collider3d = { kind = \"heightfield\", heightfield = \"#ground\" }",
            )],
        )
        .replace(
            "parent = \"n_world\"\n",
            "parent = \"n_world\"\nscript = { source = \"scripts/s.rn\" }\n",
        )),
        r"pub fn init(this) {
    this.node.collider3d.set_heightfield_hole(1, 1, true);
}

pub fn probe(this) {
    [this.node.collider3d.heightfield_hole(0, 1), this.node.collider3d.heightfield_hole(0, 0)]
}
",
    );
    b.tick(1);
    let ground = b.node("World/Ground");
    let removed = |i: usize, j: usize| {
        collider3d(&b.app, ground, |c| {
            c.shape()
                .as_heightfield()
                .unwrap()
                .cell_status(i, j)
                .contains(
                    balaur_physics::rapier3d::parry::shape::HeightFieldCellStatus::CELL_REMOVED,
                )
        })
    };
    assert!(removed(0, 1), "the asset's hole is solid ground");
    assert!(removed(1, 1), "the script's hole is solid ground");
    assert!(!removed(0, 0));
    assert_eq!(
        b.call(ground, "probe"),
        Value::List(vec![Value::Bool(true), Value::Bool(false)])
    );
}

#[test]
fn a_2d_heightfield_loses_the_segments_its_holes_name() {
    let mut b = boot::project(
        &(scene_of(
            HOLED,
            &[(
                "Ground",
                "collider2d = { kind = \"heightfield\", heightfield = \"#ground\" }",
            )],
        )
        .replace(
            "parent = \"n_world\"\n",
            "parent = \"n_world\"\nscript = { source = \"scripts/s.rn\" }\n",
        )),
        r"pub fn init(this) {
    this.node.collider2d.set_heightfield_hole(5, true);
}

pub fn probe(this) {
    [this.node.collider2d.heightfield_hole(1), this.node.collider2d.heightfield_hole(0)]
}
",
    );
    b.tick(1);
    let ground = b.node("World/Ground");
    let removed = |i: usize| {
        collider2d(&b.app, ground, |c| {
            c.shape().as_heightfield().unwrap().is_segment_removed(i)
        })
    };
    assert!(
        removed(1),
        "the asset's hole at row 0, column 1 is segment 1"
    );
    assert!(removed(5));
    assert_eq!(
        b.call(ground, "probe"),
        Value::List(vec![Value::Bool(true), Value::Bool(false)])
    );
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn a_collider_states_its_own_mass_properties() {
    let app = app();
    let stated = node_with(
        &app,
        "Stated",
        &[(
            "collider3d",
            "kind = \"box\"\nmass = 2.0\ncenter_of_mass = [0.0, 1.0, 0.0]\ninertia = [1.0, 2.0, 3.0]\ninertia_rotation = [0.5, 0.0, 0.0]",
        )],
    );
    let shifted = node_with(
        &app,
        "Shifted",
        &[(
            "collider3d",
            "kind = \"box\"\nmass = 2.0\ncenter_of_mass = [0.0, 1.0, 0.0]",
        )],
    );
    let props = collider3d(
        &app,
        stated,
        balaur_physics::rapier3d::prelude::Collider::mass_properties,
    );
    assert_eq!(props.local_com.to_array(), [0.0, 1.0, 0.0]);
    let inertia = props.principal_inertia().to_array();
    assert!(
        (inertia[0] - 1.0).abs() < 1e-4 && (inertia[2] - 3.0).abs() < 1e-4,
        "{inertia:?}"
    );
    let props = collider3d(
        &app,
        shifted,
        balaur_physics::rapier3d::prelude::Collider::mass_properties,
    );
    let mut inertia = props.principal_inertia().to_array();
    inertia.sort_by(f32::total_cmp);
    // A unit cube of mass 2 is 1/3 about each axis; moved a unit up the y
    // axis, x and z gain m d squared.
    let third = 2.0 / 12.0 * 2.0;
    assert!((inertia[0] - third).abs() < 1e-3, "{inertia:?}");
    assert!((inertia[2] - (third + 2.0)).abs() < 1e-3, "{inertia:?}");
    let back = components::get(&app.engine, stated, "collider3d").unwrap();
    assert_eq!(floats(&back, "inertia_rotation"), [0.5, 0.0, 0.0]);
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn a_collider_of_density_zero_adds_no_mass_and_says_so() {
    let mut app = app();
    let body = node_with(
        &app,
        "Body",
        &[
            ("body3d", "kind = \"dynamic\""),
            ("collider3d", "kind = \"box\""),
        ],
    );
    let ghost = scene::spawn_node(&mut app.engine.world_mut(), "Ghost", body);
    let params: toml::Value =
        toml::from_str("kind = \"box\"\nsize = [4.0, 4.0, 4.0]\ndensity = 0.0").unwrap();
    components::add(&app.engine, ghost, "collider3d", Some(&params)).unwrap();
    app.tick(1.0 / 60.0);
    let mass = {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        state.world.bodies[state.bodies[&body]].mass()
    };
    assert!(
        (mass - 1.0).abs() < 1e-4,
        "a massless collider weighed in: {mass}"
    );
    let back = components::get(&app.engine, ghost, "collider3d").unwrap();
    assert_eq!(float(&back, "density"), 0.0);
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn restitution_may_exceed_one() {
    let app = app();
    let e = node_with(
        &app,
        "Bouncy",
        &[("collider3d", "kind = \"sphere\"\nrestitution = 1.5")],
    );
    assert_eq!(
        collider3d(
            &app,
            e,
            balaur_physics::rapier3d::prelude::Collider::restitution
        ),
        1.5
    );
    let back = components::get(&app.engine, e, "collider3d").unwrap();
    assert_eq!(float(&back, "restitution"), 1.5);
}

/// The ground's mask leaves out the ball's layer: under `both` the ball falls
/// through it, and under `either` the ball's own mask is enough.
#[test]
fn either_layer_test_lets_one_side_accept_the_pair() {
    let land = |test: &str| {
        let mut app = app();
        node_with(
            &app,
            "Ground",
            &[(
                "collider3d",
                &format!(
                    "kind = \"box\"\nsize = [10.0, 1.0, 10.0]\ncollision_mask = [\"2\"]\ncollision_test = \"{test}\""
                ),
            )],
        );
        let ball = node_with(
            &app,
            "Ball",
            &[
                ("body3d", "kind = \"dynamic\""),
                (
                    "collider3d",
                    &format!("kind = \"sphere\"\nradius = 0.5\ncollision_test = \"{test}\""),
                ),
            ],
        );
        app.engine
            .world()
            .get::<&mut balaur_core::Transform>(ball)
            .unwrap()
            .position
            .y = 2.0;
        for _ in 0..90 {
            app.tick(1.0 / 60.0);
        }
        let back = components::get(&app.engine, ball, "collider3d").unwrap();
        assert_eq!(
            back.get("collision_test").and_then(toml::Value::as_str),
            Some(test)
        );
        app.engine
            .world()
            .get::<&balaur_core::Transform>(ball)
            .unwrap()
            .position
            .y
    };
    assert!(
        land("both") < 0.0,
        "under both, the ground's mask should let the ball through"
    );
    assert!(
        land("either") > 0.5,
        "under either, the ball's own mask should land it"
    );
}

#[test]
fn the_collider_readers_answer_what_rapier_works_out() {
    let b = boot::project(
        r#"[[nodes]]
id = "n_body"
name = "Body"
body3d = { kind = "dynamic", gravity_scale = 0.0 }

[[nodes]]
id = "n_part"
name = "Part"
parent = "n_body"
script = { source = "scripts/s.rn" }
collider3d = { kind = "box", mass = 3.0, collision_margin = 0.1 }
transform = { position = [1.0, 0.0, 0.0] }
"#,
        r"pub fn probe(this) {
    let c = this.node.collider3d;
    let (ax, ay, az, bx, by, bz) = c.aabb();
    let (cx, cy, cz, dx, dy, dz) = c.collision_aabb();
    let (ex, ey, ez, fx, fy, fz) = c.broad_phase_aabb();
    [c.collider_mass_properties(), c.pose_in_body(), bx, dx, fx, true]
}
",
    );
    let part = b.node("Body/Part");
    let Value::List(found) = b.call(part, "probe") else {
        panic!("the probe answered no list");
    };
    assert_eq!(found[5], Value::Bool(true), "the probe ran");
    assert!((number(entry(&found[0], "mass")) - 3.0).abs() < 1e-5);
    assert_eq!(*entry(&found[1], "position"), Value::Vec3([1.0, 0.0, 0.0]));
    let (shape, collision, broad) = (number(&found[2]), number(&found[3]), number(&found[4]));
    assert!(
        collision > shape + 0.09,
        "the collision box ignores the margin: {collision} against {shape}"
    );
    assert!(
        broad >= shape,
        "the broad-phase box is smaller than the shape"
    );
}

#[test]
fn a_contact_force_event_carries_the_whole_force() {
    let mut b = boot::project(
        r#"[variables]
max_force = { type = "float", value = 0.0 }
total_y = { type = "float", value = 0.0 }
started = { type = "int", value = 0 }

[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"
collider3d = { kind = "box", size = [10.0, 1.0, 10.0] }

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_world"
script = { source = "scripts/s.rn" }
body3d = { kind = "dynamic" }
collider3d = { kind = "box", events = ["contact_force"] }
transform = { position = [0.0, 1.0, 0.0] }
"#,
        r#"pub fn on_contact_force(this, contact) {
    scene::set_variable("max_force", contact.max_force);
    scene::set_variable("total_y", contact.total_force.y);
    if contact.started {
        scene::set_variable("started", scene::variable("started") + 1);
    }
}
"#,
    );
    b.tick(30);
    let read = |name: &str| {
        let variables = b.app.engine.resource::<balaur_core::variables::Variables>();
        let held = variables.borrow();
        held.get(name).map_or(-1.0, balaur_core::variables::as_num)
    };
    assert!(
        read("max_force") > 0.0,
        "no contact force reached the crate: {:#?}",
        boot::errors()
    );
    assert!(read("total_y").abs() > 0.0, "the total force has no y");
    assert!(
        read("started") >= 1.0,
        "no event said the force had just started"
    );
}
