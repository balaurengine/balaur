//! `balaur import level.ldtk`: an LDtk project as the files the editor edits.
//!
//! The `.ldtk` is JSON with a published shape, so it is read here directly
//! rather than through a generated type: what a project keeps is a `tileset`
//! per sheet, the atlas beside it, and a scene per level — tile layers as
//! `tilemap` nodes, entities as nodes with the fields they carried.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::import_tiled::Imported;

/// Read a `.ldtk` and lay out the files a project would keep.
pub(crate) fn import(file: &Path, stem: &str) -> Result<Imported> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let project: Value = serde_json::from_str(&text).context("that .ldtk is not JSON")?;
    let here = file.parent().unwrap_or(Path::new("."));
    let mut files = Vec::new();
    let mut sets: BTreeMap<i64, String> = BTreeMap::new();
    for set in array(&project, &["defs", "tilesets"]) {
        let Some(uid) = set.get("uid").and_then(Value::as_i64) else {
            continue;
        };
        let name = tidy(text_of(set, "identifier", &format!("{stem}_{uid}")));
        let Some(path) = set.get("relPath").and_then(Value::as_str) else {
            // An internal icon set has no file of its own; nothing to copy.
            continue;
        };
        let texture = format!("art/{name}.png");
        let bytes = std::fs::read(here.join(path))
            .with_context(|| format!("reading the atlas {path} beside the project"))?;
        files.push((texture.clone(), bytes));
        files.push((format!("tilesets/{name}.toml"), tileset_toml(set, &texture)));
        sets.insert(uid, name);
    }
    if sets.is_empty() {
        bail!("that project names no tileset with an image, so its cells index nothing");
    }
    let mut layers = 0;
    for level in array(&project, &["levels"]) {
        let name = tidy(text_of(level, "identifier", stem));
        let (scene, count) = level_toml(level, &sets)?;
        layers += count;
        files.push((format!("scenes/{name}.toml"), scene.into_bytes()));
    }
    Ok(Imported { files, layers })
}

fn array<'a>(value: &'a Value, path: &[&str]) -> &'a [Value] {
    let mut at = value;
    for key in path {
        match at.get(key) {
            Some(next) => at = next,
            None => return &[],
        }
    }
    at.as_array().map_or(&[], Vec::as_slice)
}

fn text_of<'a>(value: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or(fallback)
}

fn number(value: &Value, key: &str, fallback: i64) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(fallback)
}

/// A name a TOML key can hold.
fn tidy(name: &str) -> String {
    name.to_ascii_lowercase().replace([' ', '-', '.'], "_")
}

/// One `tileset` asset from a tileset definition.
fn tileset_toml(set: &Value, texture: &str) -> Vec<u8> {
    let grid = number(set, "tileGridSize", 16);
    let mut out = format!(
        "type = \"tileset\"\ntexture = \"{texture}\"\ntile_size = [{grid}, {grid}]\ncolumns = {}\n",
        number(set, "__cWid", 1).max(1)
    );
    let spacing = number(set, "spacing", 0);
    let margin = number(set, "padding", 0);
    if spacing > 0 {
        let _ = writeln!(out, "spacing = {spacing}");
    }
    if margin > 0 {
        let _ = writeln!(out, "margin = {margin}");
    }
    out.into_bytes()
}

/// One scene: a `tilemap` per tile layer, and a node per entity.
///
/// LDtk lists its layers top first, so the z-index counts down as the file
/// reads and what it drew on top draws on top here too.
fn level_toml(level: &Value, sets: &BTreeMap<i64, String>) -> Result<(String, usize)> {
    let layers = array(level, &["layerInstances"]);
    // A tileset kept in a file is named by its path, not re-declared here.
    let mut out = String::new();
    let mut drawn = 0;
    let mut z = i64::try_from(layers.len()).unwrap_or(0);
    for layer in layers {
        z -= 1;
        let kind = text_of(layer, "__type", "");
        let name = tidy(text_of(layer, "__identifier", "layer"));
        if kind == "Entities" {
            out.push_str(&entities_toml(layer, &name));
            continue;
        }
        let Some(set) = layer
            .get("__tilesetDefUid")
            .and_then(Value::as_i64)
            .and_then(|uid| sets.get(&uid))
        else {
            continue;
        };
        let grid = number(layer, "__gridSize", 16).max(1);
        let (columns, rows) = (
            number(layer, "__cWid", 0).max(0),
            number(layer, "__cHei", 0).max(0),
        );
        let mut cells = vec![vec![-1i64; columns as usize]; rows as usize];
        let mut flags = vec![vec![0i64; columns as usize]; rows as usize];
        let mut any = false;
        for tile in array(layer, &["gridTiles"])
            .iter()
            .chain(array(layer, &["autoLayerTiles"]))
        {
            let at = tile.get("px").and_then(Value::as_array);
            let (Some(at), Some(id)) = (at, tile.get("t").and_then(Value::as_i64)) else {
                continue;
            };
            let column = at.first().and_then(Value::as_i64).unwrap_or(0) / grid;
            let row = at.get(1).and_then(Value::as_i64).unwrap_or(0) / grid;
            let (Ok(column), Ok(row)) = (usize::try_from(column), usize::try_from(row)) else {
                continue;
            };
            if row >= cells.len() || column >= cells[row].len() {
                continue;
            }
            cells[row][column] = id;
            // LDtk's `f` is two bits: 1 mirrors across x, 2 across y.
            flags[row][column] = number(tile, "f", 0);
            any = true;
        }
        if !any {
            continue;
        }
        drawn += 1;
        let _ = write!(
            out,
            "[[nodes]]\nid = \"n_{name}\"\nname = \"{}\"\nz_index = {z}\n\n[nodes.tilemap]\ntileset = \"tilesets/{set}.toml\"\npixels_per_unit = {grid}\ncells = [\n",
            text_of(layer, "__identifier", "Layer")
        );
        for row in &cells {
            let _ = writeln!(out, "  [{}],", spell(row));
        }
        out.push_str("]\n");
        if flags.iter().any(|row| row.iter().any(|bits| *bits != 0)) {
            out.push_str("flags = [\n");
            for row in &flags {
                let _ = writeln!(out, "  [{}],", spell(row));
            }
            out.push_str("]\n");
        }
        out.push('\n');
    }
    if out.is_empty() {
        return Err(anyhow!("that level has nothing in it"));
    }
    Ok((out, drawn))
}

fn spell(row: &[i64]) -> String {
    row.iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// An entity layer: one node per entity, at its own place, carrying the
/// fields it was given.
fn entities_toml(layer: &Value, layer_name: &str) -> String {
    let mut out = String::new();
    let grid = number(layer, "__gridSize", 16).max(1) as f64;
    for (index, entity) in array(layer, &["entityInstances"]).iter().enumerate() {
        let name = text_of(entity, "__identifier", "Entity");
        let at = entity.get("px").and_then(Value::as_array);
        let x = at
            .and_then(|at| at.first())
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            / grid;
        let y = at
            .and_then(|at| at.get(1))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            / grid;
        let _ = write!(
            out,
            "[[nodes]]\nid = \"n_{layer_name}_{index}\"\nname = \"{name}\"\nposition = [{x}, {}, 0.0]\n\n",
            -y
        );
        let fields = array(entity, &["fieldInstances"]);
        if fields.is_empty() {
            continue;
        }
        let _ = writeln!(out, "[nodes.props.data_{layer_name}_{index}]");
        for field in fields {
            let key = tidy(text_of(field, "__identifier", "field"));
            let value = field.get("__value").cloned().unwrap_or(Value::Null);
            let _ = writeln!(out, "{key} = {}", spell_value(&value));
        }
        out.push('\n');
    }
    out
}

/// A field's value as TOML. Anything with no TOML shape becomes its text, so
/// nothing an author wrote is silently dropped.
fn spell_value(value: &Value) -> String {
    match value {
        Value::Null => "\"\"".to_string(),
        Value::Bool(yes) => yes.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => format!("{text:?}"),
        Value::Array(list) => format!(
            "[{}]",
            list.iter().map(spell_value).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(_) => format!("{:?}", value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = r#"{
      "defs": { "tilesets": [
        { "uid": 1, "identifier": "Blocks", "relPath": "blocks.png",
          "tileGridSize": 16, "spacing": 0, "padding": 0, "__cWid": 4, "__cHei": 1 }
      ]},
      "levels": [
        { "identifier": "Cave", "layerInstances": [
          { "__identifier": "Walls", "__type": "Tiles", "__cWid": 2, "__cHei": 2,
            "__gridSize": 16, "__tilesetDefUid": 1,
            "gridTiles": [
              { "px": [0, 0], "src": [0, 0], "f": 0, "t": 3 },
              { "px": [16, 16], "src": [0, 0], "f": 1, "t": 2 }
            ]},
          { "__identifier": "Things", "__type": "Entities", "__gridSize": 16,
            "entityInstances": [
              { "__identifier": "Chest", "px": [32, 16],
                "fieldInstances": [ { "__identifier": "gold", "__value": 12 } ] }
            ]}
        ]}
      ]
    }"#;

    fn imported() -> Imported {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cave.ldtk"), PROJECT).unwrap();
        std::fs::write(dir.path().join("blocks.png"), b"not really a png").unwrap();
        import(&dir.path().join("cave.ldtk"), "cave").expect("the project imports")
    }

    fn written(files: &[(String, Vec<u8>)], name: &str) -> String {
        let (_, bytes) = files
            .iter()
            .find(|(path, _)| path == name)
            .unwrap_or_else(|| panic!("no {name} among {:?}", files.iter().map(|(p, _)| p)));
        String::from_utf8(bytes.clone()).unwrap()
    }

    #[test]
    fn a_level_becomes_a_scene_of_its_layers() {
        let out = imported();
        assert_eq!(out.layers, 1);
        let scene = written(&out.files, "scenes/cave.toml");
        assert!(scene.contains("[nodes.tilemap]"), "{scene}");
        assert!(
            scene.contains("[3, -1],") && scene.contains("[-1, 2],"),
            "a tile sits where its pixel place puts it: {scene}"
        );
        assert!(
            scene.contains("flags = ["),
            "a mirrored tile keeps its turn: {scene}"
        );
    }

    #[test]
    fn an_entity_becomes_a_node_with_the_fields_it_carried() {
        let scene = written(&imported().files, "scenes/cave.toml");
        assert!(scene.contains("name = \"Chest\""), "{scene}");
        assert!(scene.contains("gold = 12"), "{scene}");
    }

    #[test]
    fn a_tileset_definition_becomes_a_tileset_asset() {
        let set = written(&imported().files, "tilesets/blocks.toml");
        assert!(
            set.contains("tile_size = [16, 16]") && set.contains("columns = 4"),
            "{set}"
        );
    }
}
