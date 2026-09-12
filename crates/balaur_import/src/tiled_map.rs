//! `balaur import level.tmx`: a Tiled map as the files the editor edits.
//!
//! The engine never reads a `.tmx`: what a project keeps is a `tileset` asset
//! per sheet, the atlas beside it, and a scene rooted at the map, holding one
//! `tilemap` node per tile layer, in the order Tiled drew them.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

/// What an import wrote, so the caller can say so. Shared with the LDtk
/// importer, which lays out the same kinds of file.
pub(crate) struct Imported {
    pub files: Vec<(String, Vec<u8>)>,
    pub layers: usize,
}

/// Read a `.tmx` and lay out the files a project would keep.
pub(crate) fn import(file: &Path, stem: &str) -> Result<Imported> {
    let mut loader = tiled::Loader::new();
    let map = loader
        .load_tmx_map(file)
        .map_err(|why| anyhow!("that map will not load: {why}"))?;
    let mut files = Vec::new();
    let mut sets = Vec::new();
    for (index, set) in map.tilesets().iter().enumerate() {
        let name = if set.name.is_empty() {
            format!("{stem}_{index}")
        } else {
            tidy(&set.name)
        };
        let image = set
            .image
            .as_ref()
            .ok_or_else(|| anyhow!("tileset '{name}' has no image: a collection is not a grid"))?;
        let texture = format!("art/{name}.png");
        let bytes = std::fs::read(&image.source)
            .with_context(|| format!("reading {}", image.source.display()))?;
        files.push((texture.clone(), bytes));
        files.push((
            format!("tilesets/{name}.toml"),
            tileset_toml(set, &texture).into_bytes(),
        ));
        sets.push(name);
    }
    let scene = scene_toml(&map, &sets, stem)?;
    let layers = map
        .layers()
        .filter(|layer| matches!(layer.layer_type(), tiled::LayerType::Tiles(_)))
        .count();
    files.push((format!("scenes/{stem}.toml"), scene.into_bytes()));
    Ok(Imported { files, layers })
}

/// A name a TOML key can hold.
fn tidy(name: &str) -> String {
    name.to_ascii_lowercase().replace([' ', '-', '.'], "_")
}

/// One `tileset` asset: the grid, and what Tiled says about each tile.
fn tileset_toml(set: &tiled::Tileset, texture: &str) -> String {
    let mut out = format!(
        "type = \"tileset\"\ntexture = \"{texture}\"\ntile_size = [{}, {}]\ncolumns = {}\n",
        set.tile_width, set.tile_height, set.columns
    );
    if set.spacing > 0 {
        let _ = writeln!(out, "spacing = {}", set.spacing);
    }
    if set.margin > 0 {
        let _ = writeln!(out, "margin = {}", set.margin);
    }
    for (id, tile) in set.tiles() {
        let mut lines = Vec::new();
        if let Some(collision) = &tile.collision {
            lines.extend(collision_lines(collision, set));
        }
        if let Some(animation) = &tile.animation {
            let frames: Vec<String> = animation
                .iter()
                .map(|frame| frame.tile_id.to_string())
                .collect();
            let fps = animation
                .first()
                .filter(|frame| frame.duration > 0)
                .map_or(8.0, |frame| 1000.0 / f32::from(frame.duration as u16));
            lines.push(format!(
                "animation = {{ frames = [{}], fps = {fps:.3} }}",
                frames.join(", ")
            ));
        }
        if lines.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n[tiles.{id}]");
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// A tile's collision objects, as polygons in tile pixels.
///
/// A rectangle is the common case and becomes `collision = "full"` when it
/// covers the whole tile, which is what most maps mean by a solid tile.
fn collision_lines(group: &tiled::ObjectLayerData, set: &tiled::Tileset) -> Vec<String> {
    let (width, height) = (set.tile_width as f32, set.tile_height as f32);
    let mut polygons: Vec<String> = Vec::new();
    for object in group.object_data() {
        let points: Vec<[f32; 2]> = match &object.shape {
            tiled::ObjectShape::Rect {
                width: w,
                height: h,
            } => {
                if *w >= width - 0.5 && *h >= height - 0.5 && object.x <= 0.5 && object.y <= 0.5 {
                    return vec!["collision = \"full\"".to_string()];
                }
                vec![
                    [object.x, object.y],
                    [object.x + w, object.y],
                    [object.x + w, object.y + h],
                    [object.x, object.y + h],
                ]
            }
            tiled::ObjectShape::Polygon { points } => points
                .iter()
                .map(|(x, y)| [object.x + x, object.y + y])
                .collect(),
            _ => continue,
        };
        let spelled: Vec<String> = points.iter().map(|[x, y]| format!("[{x}, {y}]")).collect();
        polygons.push(format!("[{}]", spelled.join(", ")));
    }
    if polygons.is_empty() {
        return Vec::new();
    }
    vec![format!("collision = [{}]", polygons.join(", "))]
}

/// The scene: one `tilemap` node per tile layer, drawn in Tiled's own order.
fn scene_toml(map: &tiled::Map, sets: &[String], stem: &str) -> Result<String> {
    if sets.is_empty() {
        bail!("that map names no tileset, so its cells index nothing");
    }
    // A tileset kept in a file is named by its path; the scene does not
    // re-declare it as an asset of its own.
    let mut out = String::new();
    let mut z = 0;
    for layer in map.layers() {
        let tiled::LayerType::Tiles(tiles) = layer.layer_type() else {
            continue;
        };
        let name = tidy(&layer.name);
        let _ = write!(
            out,
            "[[nodes]]\nid = \"n_{stem}_{name}\"\nname = \"{}\"\nparent = \"n_{stem}\"\nz_index = {z}\n\n[nodes.tilemap]\ntileset = \"tilesets/{}.toml\"\npixels_per_unit = {}\ncells = [\n",
            layer.name, sets[0], map.tile_width
        );
        for row in 0..map.height {
            let mut line = Vec::new();
            for column in 0..map.width {
                let at = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
                let id = tiles
                    .get_tile(at(column), at(row))
                    .map_or(-1, |tile| i64::from(tile.id()));
                line.push(id.to_string());
            }
            let _ = writeln!(out, "  [{}],", line.join(", "));
        }
        out.push_str("]\n\n");
        z += 1;
    }
    if z == 0 {
        bail!("that map has no tile layers, so there is nothing to draw");
    }
    // A scene has one root, so the map is it and its layers are children.
    let root = format!("[[nodes]]\nid = \"n_{stem}\"\nname = \"{stem}\"\n\n");
    Ok(root + &out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_check::loaded;

    const MAP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="2" height="2" tilewidth="16" tileheight="16" infinite="0" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" name="dungeon" tilewidth="16" tileheight="16" tilecount="4" columns="2">
  <image source="dungeon.png" width="32" height="32"/>
  <tile id="0">
   <objectgroup draworder="index">
    <object id="1" x="0" y="0" width="16" height="16"/>
   </objectgroup>
  </tile>
 </tileset>
 <layer id="1" name="ground" width="2" height="2">
  <data encoding="csv">
1,2,
0,1
</data>
 </layer>
</map>
"#;

    fn imported() -> Imported {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("level.tmx"), MAP).unwrap();
        std::fs::write(dir.path().join("dungeon.png"), b"not really a png").unwrap();
        import(&dir.path().join("level.tmx"), "level").expect("the map imports")
    }

    fn written(files: &[(String, Vec<u8>)], name: &str) -> String {
        let (_, bytes) = files
            .iter()
            .find(|(path, _)| path == name)
            .unwrap_or_else(|| panic!("no {name} among {:?}", files.iter().map(|(p, _)| p)));
        String::from_utf8(bytes.clone()).unwrap()
    }

    #[test]
    fn a_tiled_map_becomes_a_tileset_an_atlas_and_a_scene() {
        let out = imported();
        assert_eq!(out.layers, 1);
        let set = written(&out.files, "tilesets/dungeon.toml");
        assert!(set.contains("tile_size = [16, 16]"), "{set}");
        assert!(set.contains("columns = 2"), "{set}");
        assert!(
            set.contains("[tiles.0]") && set.contains("collision = \"full\""),
            "a tile whose object covers it is solid, not a polygon: {set}"
        );
        written(&out.files, "art/dungeon.png");
    }

    #[test]
    fn every_tile_layer_becomes_a_map_of_its_own() {
        let out = imported();
        let scene = written(&out.files, "scenes/level.toml");
        assert!(scene.contains("[nodes.tilemap]"), "{scene}");
        assert_eq!(
            loaded(&scene),
            vec![("level".to_string(), vec!["ground".to_string()])],
            "the map is the one root and its layers hang from it: {scene}"
        );
        assert!(
            scene.contains("[0, 1],") && scene.contains("[-1, 0],"),
            "gid 0 is an empty cell, and the rest count from zero: {scene}"
        );
    }
}
