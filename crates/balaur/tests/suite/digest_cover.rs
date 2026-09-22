//! What a digest covers, against what a node actually has.
//!
//! `digest::push_components` walks the `Attached` bits rather than asking all
//! forty-eight definitions, which is right only while the bits are the whole
//! truth. If a component ever reaches a node without marking one, the digest
//! stops hashing it: both peers of a match still agree, because both dropped
//! it, so nothing fails. What breaks is quieter — a recording made before the
//! drop no longer replays, and a desync in that component is invisible.
//!
//! So this asserts the invariant the walk rests on, over a scene carrying a
//! spread of component kinds.

use balaur::{AppConfig, components, standard_app};

const SCENE: &str = r#"
[[nodes]]
id = "n_root"
name = "Root"
[nodes.transform]
position = [1.0, 2.0, 3.0]

[[nodes]]
id = "n_body"
name = "Body"
parent = "n_root"
[nodes.transform]
position = [0.0, 4.0, 0.0]
[nodes.body3d]
kind = "dynamic"
[nodes.collider3d]
kind = "ball"
radius = 0.5

[[nodes]]
id = "n_sprite"
name = "Sprite"
parent = "n_root"
[nodes.transform]
scale = [2.0, 2.0, 1.0]
[nodes.sprite]
pixels_per_unit = 50.0

[[nodes]]
id = "n_shape"
name = "Shape"
parent = "n_root"
[nodes.shape3d]
kind = "cuboid"
[nodes.material]
name = ""

[[nodes]]
id = "n_bare"
name = "Bare"
parent = "n_root"
"#;

/// Every component that answers `get` on a node has its bit set. The digest
/// hashes the bits, so a component missing one is a component nothing checks.
///
/// Over the scene above and then over every component the registry will add
/// with its own defaults, which is most of them.
#[test]
fn the_bits_a_digest_walks_cover_every_component_a_node_reports() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"cover\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), SCENE).unwrap();
    let mut app = standard_app(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    app.load_project().unwrap();

    let names = components::names(&app.engine);
    // A node of its own for each, so one component's `apply` cannot decide
    // what the next one reports.
    let root = app.engine.root();
    let mut added = 0;
    for name in &names {
        let entity = balaur::scene::spawn_node(&mut app.engine.world_mut(), name, root);
        if components::add(&app.engine, entity, name, None).is_ok() {
            added += 1;
        }
    }

    let entities: Vec<_> = app.engine.world().iter().map(|e| e.entity()).collect();
    let mut checked = 0;
    for entity in entities {
        let bits = components::attached_of(&app.engine, entity);
        for (index, name) in names.iter().enumerate() {
            let reported = components::get(&app.engine, entity, name).is_some();
            if reported {
                checked += 1;
            }
            assert!(
                !reported || bits.has(index),
                "`{name}` answers on a node and has no bit, so a digest would not hash it"
            );
        }
    }
    assert!(
        added >= 20 && checked >= 25,
        "only {added} components added and {checked} answered, so this proved little"
    );
    eprintln!("digest cover: {added} components added, {checked} answered on a node");
}
