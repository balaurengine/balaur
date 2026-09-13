//! The `tileset` asset and the `tilemap` component, without a window.

use balaur_core::{App, AppConfig, components, scene};
use balaur_render::{TileSet, Tilemap};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).expect("App::new builds headless");
    balaur_plugin::load(&mut app, &mut balaur_render::RenderPlugin::default())
        .expect("the render plugin builds headless");
    app
}

fn node(app: &App) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

/// A tilemap authored inline: the `tileset` property carries a definition
/// table, `cells` a two-row map.
fn tilemap_table() -> toml::Value {
    toml::from_str(
        r#"cells = [
  [-1,  0],
  [ 1, 10],
]
pixels_per_unit = 50.0

[tileset]
texture = "tests/fixtures/sprite_200x100.png"
tile_size = 50.0
columns = 4
"#,
    )
    .expect("the tilemap params are valid TOML")
}

#[test]
fn a_tilemap_parses_cells_and_round_trips() {
    let app = app();
    let entity = node(&app);
    components::add(&app.engine, entity, "tilemap", Some(&tilemap_table()))
        .expect("a valid tilemap applies");

    // Control that apply really parsed: row 0 is the text's top line, `.` is
    // empty, digits then letters index the tileset.
    {
        let world = app.engine.world();
        let map = world
            .get::<&Tilemap>(entity)
            .expect("apply writes a Tilemap on the node");
        assert_eq!(
            map.grid,
            vec![vec![None, Some(0)], vec![Some(1), Some(10)]],
            "the cells text did not parse into the expected grid"
        );
    }

    let saved = components::get(&app.engine, entity, "tilemap").expect("the tilemap reads back");
    let table = saved.as_table().expect("get returns a property table");
    let rows = table["cells"]
        .as_array()
        .expect("cells reads back as rows of tile ids");
    let ids: Vec<Vec<i64>> = rows
        .iter()
        .map(|row| {
            row.as_array()
                .expect("a cells row is a list")
                .iter()
                .map(|id| id.as_integer().expect("a cell is a whole number"))
                .collect()
        })
        .collect();
    assert_eq!(ids, vec![vec![-1, 0], vec![1, 10]]);
    assert!(
        (table["pixels_per_unit"]
            .as_float()
            .expect("pixels_per_unit reads back as a float")
            - 50.0)
            .abs()
            < 1e-6
    );
    let reference = table["tileset"]
        .as_str()
        .expect("the inline tileset reads back as a reference string");
    assert!(
        reference.starts_with("#!"),
        "an inline definition should have become a reference, got '{reference}'"
    );

    let tileset = balaur_core::assets::load_typed::<TileSet>(&app.engine, reference)
        .expect("the inline tileset parses");
    assert_eq!(tileset.texture, "tests/fixtures/sprite_200x100.png");
    #[allow(clippy::float_cmp, reason = "a parsed size, not an arithmetic one")]
    let square = tileset.tile_size == [50.0, 50.0];
    assert!(square, "a square size reads as a pair");
    assert_eq!(tileset.columns, 4);

    let reloaded = node(&app);
    components::add(&app.engine, reloaded, "tilemap", Some(&saved))
        .expect("a saved tilemap reloads");
    {
        let world = app.engine.world();
        let a = world.get::<&Tilemap>(entity).expect("original still there");
        let b = world
            .get::<&Tilemap>(reloaded)
            .expect("reload writes a Tilemap");
        assert_eq!(a.grid, b.grid);
    }

    // Cells are rows of ids: a string that is not a `.cells` file is refused,
    // and the error says what the property takes.
    let bad: toml::Value = toml::from_str("cells = \".X\"").expect("valid TOML");
    let entity = node(&app);
    let err = components::add(&app.engine, entity, "tilemap", Some(&bad))
        .expect_err("a string that names no `.cells` file must be rejected");
    assert!(
        format!("{err:#}").contains(".cells"),
        "the error should name the file form: {err:#}"
    );
}

#[test]
fn reapplying_the_same_tilemap_does_not_bump_the_version() {
    let app = app();
    let entity = node(&app);
    components::add(&app.engine, entity, "tilemap", Some(&tilemap_table()))
        .expect("a valid tilemap applies");
    let before = app
        .engine
        .world()
        .get::<&Tilemap>(entity)
        .expect("apply writes a Tilemap")
        .version;
    components::add(&app.engine, entity, "tilemap", Some(&tilemap_table()))
        .expect("re-applying the same tilemap succeeds");
    assert_eq!(
        app.engine
            .world()
            .get::<&Tilemap>(entity)
            .expect("still there")
            .version,
        before,
        "identical content must not force a backend rebuild"
    );

    let mut changed = tilemap_table();
    changed.as_table_mut().expect("params are a table").insert(
        "cells".into(),
        toml::Value::Array(vec![toml::Value::Array(vec![
            toml::Value::Integer(2),
            toml::Value::Integer(2),
        ])]),
    );
    components::add(&app.engine, entity, "tilemap", Some(&changed))
        .expect("a changed tilemap applies");
    assert!(
        app.engine
            .world()
            .get::<&Tilemap>(entity)
            .expect("still there")
            .version
            > before,
        "changed cells must bump the version"
    );
}

#[test]
fn a_tileset_that_declares_no_grid_is_refused() {
    let app = app();
    let entity = node(&app);
    // Well-formed component, ill-formed asset: `columns` is missing.
    let params: toml::Value = toml::from_str(
        r#"cells = [[0]]

[tileset]
texture = "tests/fixtures/sprite_200x100.png"
tile_size = 50.0
"#,
    )
    .expect("the params are valid TOML");
    components::add(&app.engine, entity, "tilemap", Some(&params))
        .expect("a bad tileset warns rather than killing the scene");
    let reference = components::get(&app.engine, entity, "tilemap")
        .expect("the tilemap reads back")
        .get("tileset")
        .and_then(|v| v.as_str().map(str::to_string))
        .expect("the tileset reference reads back");
    let Err(err) = balaur_core::assets::load_typed::<TileSet>(&app.engine, &reference) else {
        panic!("a tileset without `columns` must not parse");
    };
    assert!(
        format!("{err:#}").contains("columns"),
        "the error should name the missing field: {err:#}"
    );
}

#[test]
fn cells_as_rows_of_ids_reach_past_the_thirty_sixth_tile() {
    let app = app();
    let entity = node(&app);
    let mut table = tilemap_table();
    table.as_table_mut().unwrap().insert(
        "cells".into(),
        toml::from_str::<toml::Value>("v = [[40, -1], [0, 99]]").unwrap()["v"].clone(),
    );
    components::add(&app.engine, entity, "tilemap", Some(&table)).unwrap();
    let world = app.engine.world();
    let map = world.get::<&Tilemap>(entity).unwrap();
    assert_eq!(
        map.grid,
        vec![vec![Some(40), None], vec![Some(0), Some(99)]]
    );
}

#[test]
fn a_material_on_the_map_is_kept_and_bumps_the_version_when_it_changes() {
    let app = app();
    let entity = node(&app);
    components::add(&app.engine, entity, "tilemap", Some(&tilemap_table())).unwrap();
    let before = app.engine.world().get::<&Tilemap>(entity).unwrap().version;
    let mut table = tilemap_table();
    table.as_table_mut().unwrap().insert(
        "material".into(),
        toml::Value::String("materials/water.toml".into()),
    );
    components::add(&app.engine, entity, "tilemap", Some(&table)).unwrap();
    let world = app.engine.world();
    let map = world.get::<&Tilemap>(entity).unwrap();
    assert_eq!(map.material, "materials/water.toml");
    assert_eq!(map.version, before + 1);
}

/// A level too big to read in a scene keeps its rows in a file of its own.
#[test]
fn a_map_may_keep_its_cells_in_a_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("levels")).unwrap();
    std::fs::write(dir.path().join("levels/cave.cells"), "0 1 -1\n1 1 0\n").unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[assets]]\nid = \"set\"\ntype = \"tileset\"\ntexture = \"tests/fixtures/sprite_200x100.png\"\ntile_size = 50\ncolumns = 4\n\n[[nodes]]\nid = \"n\"\nname = \"Map\"\n\n[nodes.tilemap]\ntileset = \"#set\"\ncells = \"levels/cave.cells\"\n",
    )
    .unwrap();
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        dir.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let node = balaur_core::scene::find_node(&world, app.engine.root(), "Map").unwrap();
    let map = world
        .get::<&Tilemap>(node)
        .expect("the map loaded its file");
    assert_eq!(map.grid.len(), 2, "one row per line");
    assert_eq!(
        map.grid[0],
        vec![Some(0), Some(1), None],
        "-1 is an empty cell"
    );
}
