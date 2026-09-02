//! Authored geometry: the mesh data a model file describes, the parsers that
//! produce it, and the `mesh` asset type that names it.
//!
//! Deliberately free of any backend type. This module is compiled in headless
//! builds too, so a tool or a test can load a model without a GPU, and the
//! windowed backend converts [`MeshData`] into its own buffers at upload.
//!
//! Parsers take bytes rather than a path: a shipped game's models live inside
//! its pack (`crate::project::ProjectFiles`), not beside it on disk.

use crate::collections::DetHashMap;
use crate::App;
use anyhow::{anyhow, bail, Result};

/// One triangulated mesh, indexed.
///
/// `normals` and `uvs` are optional because a backend can compute normals
/// from the faces and default the UVs; a file that carries them is simply
/// better than one that does not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    /// Triangles, as indices into `positions`.
    pub indices: Vec<[u32; 3]>,
    /// Per-vertex, same length as `positions` when present.
    pub normals: Option<Vec<[f32; 3]>>,
    /// Per-vertex, same length as `positions` when present.
    pub uvs: Option<Vec<[f32; 2]>>,
    /// Set when the definition named a file instead of carrying vertices.
    /// Resolved by [`load_from`], which has the project reader to do it with.
    pub source: Option<String>,
    /// Bone influences, when the mesh is meant to deform with a rig.
    pub skin: Option<MeshSkin>,
}

/// Per-vertex bone influences, in the form a GPU reads: up to four joints
/// and their weights per vertex, and the bones those joints stand for.
///
/// Authored per bone (`[[skin.bones]]`, one weight per vertex) and folded to
/// this at parse: the four heaviest influences, renormalised. A vertex no
/// bone claims keeps zero weights and is not deformed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshSkin {
    /// Bone node paths relative to the rig root, in joint-index order.
    pub bones: Vec<String>,
    /// Per vertex, same length as `positions`.
    pub joints: Vec<[u32; 4]>,
    /// Per vertex, same length as `positions`; sums to one or to zero.
    pub weights: Vec<[f32; 4]>,
    /// Per bone, in rig space: what an imported model carries instead of a
    /// rest pose to invert. Absent for a rig authored in the editor.
    pub inverse_bind: Option<Vec<glamx::Mat4>>,
}

/// How many bones may influence one vertex, which is what the shader blends.
pub const INFLUENCES_PER_VERTEX: usize = 4;

impl MeshData {
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len()
    }

    /// The axis-aligned bounds, for sizing a selection box or a collider.
    /// `None` when the mesh has no vertices.
    #[must_use]
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let first = *self.positions.first()?;
        let mut min = first;
        let mut max = first;
        for p in &self.positions {
            for axis in 0..3 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        Some((min, max))
    }
}

/// Which parser a file name selects. Unknown extensions are an error rather
/// than a guess: loading a `.blend` as OBJ produces nonsense, not a mesh.
///
/// # Errors
/// If the extension names no parser, or the bytes do not parse.
pub fn parse(bytes: &[u8], name: &str) -> Result<MeshData> {
    parse_with(bytes, name, &crate::glb::no_side_files)
}

/// [`parse`], with a reader for the files a `.gltf` keeps beside itself.
///
/// # Errors
/// As [`parse`], and if a side file the model names cannot be read.
pub fn parse_with(bytes: &[u8], name: &str, side: crate::glb::SideReader<'_>) -> Result<MeshData> {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "obj" => parse_obj(bytes, name),
        "glb" | "gltf" => crate::glb::parse_gltf(bytes, name, side),
        other => bail!("no mesh parser for '.{other}' ({name}); balaur reads .obj, .glb and .gltf"),
    }
}

/// An OBJ corner: indices into the file's own position, texture and normal
/// lists. One GPU vertex per distinct triple, because a GPU mesh has a single
/// index stream and OBJ has three.
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct Corner {
    position: usize,
    uv: Option<usize>,
    normal: Option<usize>,
}

/// Wavefront OBJ. Positions, texture coordinates, normals and faces; the
/// material library is ignored, since balaur has no material model to put it in.
///
/// # Errors
/// If a face names a vertex the file never declared, or a number does not parse.
pub fn parse_obj(bytes: &[u8], name: &str) -> Result<MeshData> {
    let text = std::str::from_utf8(bytes).map_err(|e| anyhow!("{name} is not UTF-8: {e}"))?;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut corners: Vec<Corner> = Vec::new();
    let mut seen: DetHashMap<Corner, u32> = DetHashMap::default();
    let mut out = MeshData::default();
    let mut had_uv = false;
    let mut had_normal = false;

    for (number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let mut parts = line.split_whitespace();
        let Some(tag) = parts.next() else { continue };
        let at = || format!("{name}:{}", number + 1);
        match tag {
            "v" => positions.push(vec3(&mut parts, &at())?),
            "vt" => {
                let u = number_at(parts.next(), &at())?;
                let v = number_at(parts.next(), &at()).unwrap_or(0.0);
                uvs.push([u, v]);
            }
            "vn" => normals.push(vec3(&mut parts, &at())?),
            "f" => {
                // A face may be any convex polygon; a fan turns it into
                // triangles without needing to know how many corners it has.
                let mut fan: Vec<u32> = Vec::new();
                for token in parts {
                    let corner =
                        corner_of(token, positions.len(), uvs.len(), normals.len(), &at())?;
                    had_uv |= corner.uv.is_some();
                    had_normal |= corner.normal.is_some();
                    let index = if let Some(index) = seen.get(&corner) {
                        *index
                    } else {
                        let index = u32::try_from(corners.len())
                            .map_err(|_| anyhow!("{name}: more vertices than a u32 index"))?;
                        seen.insert(corner, index);
                        corners.push(corner);
                        index
                    };
                    fan.push(index);
                }
                if fan.len() < 3 {
                    bail!("{}: a face needs at least three corners", at());
                }
                for i in 1..fan.len() - 1 {
                    out.indices.push([fan[0], fan[i], fan[i + 1]]);
                }
            }
            // `usemtl`, `mtllib`, `o`, `g`, `s` and anything else: balaur has
            // no material or group model, so they are read past, not rejected.
            _ => {}
        }
    }

    if corners.is_empty() {
        bail!("{name} declares no faces, so there is no mesh in it");
    }
    out.positions = corners.iter().map(|c| positions[c.position]).collect();
    if had_uv {
        out.uvs = Some(
            corners
                .iter()
                .map(|c| c.uv.map_or([0.0, 0.0], |i| uvs[i]))
                .collect(),
        );
    }
    if had_normal {
        out.normals = Some(
            corners
                .iter()
                .map(|c| c.normal.map_or([0.0, 1.0, 0.0], |i| normals[i]))
                .collect(),
        );
    }
    Ok(out)
}

fn vec3<'a>(parts: &mut impl Iterator<Item = &'a str>, at: &str) -> Result<[f32; 3]> {
    Ok([
        number_at(parts.next(), at)?,
        number_at(parts.next(), at)?,
        number_at(parts.next(), at)?,
    ])
}

fn number_at(token: Option<&str>, at: &str) -> Result<f32> {
    let token = token.ok_or_else(|| anyhow!("{at}: too few numbers"))?;
    token
        .parse()
        .map_err(|_| anyhow!("{at}: '{token}' is not a number"))
}

/// `v`, `v/vt`, `v//vn` or `v/vt/vn`, one-based, and negative meaning "counting
/// back from the newest" — which is how a concatenated OBJ stays valid.
fn corner_of(
    token: &str,
    positions: usize,
    uvs: usize,
    normals: usize,
    at: &str,
) -> Result<Corner> {
    let mut fields = token.split('/');
    let position = one_based(fields.next(), positions, at, "vertex")?
        .ok_or_else(|| anyhow!("{at}: a face corner needs a vertex"))?;
    let uv = one_based(fields.next(), uvs, at, "texture coordinate")?;
    let normal = one_based(fields.next(), normals, at, "normal")?;
    Ok(Corner {
        position,
        uv,
        normal,
    })
}

fn one_based(field: Option<&str>, len: usize, at: &str, what: &str) -> Result<Option<usize>> {
    let Some(field) = field.map(str::trim).filter(|f| !f.is_empty()) else {
        return Ok(None);
    };
    let raw: i64 = field
        .parse()
        .map_err(|_| anyhow!("{at}: '{field}' is not a {what} index"))?;
    let len = i64::try_from(len).unwrap_or(i64::MAX);
    let index = if raw < 0 { len + raw } else { raw - 1 };
    if index < 0 || index >= len {
        bail!("{at}: {what} {raw} is out of range (the file declares {len})");
    }
    Ok(Some(usize::try_from(index).unwrap_or(0)))
}

/// The `mesh` asset type: geometry as a project resource.
pub const MESH_ASSET_TYPE: &str = "mesh";

/// What a definition table holds, for the generated reference.
const MESH_ASSET_DOC: &str = r##"Geometry for `mesh`-typed properties. A definition either names a `source`
model file to import or carries the vertices itself as `positions` and
`indices`, which is what lets a script build one at run time; naming both is
refused. A `skin` table adds bone weights for skeletal animation.

```toml
[[assets]]
id = "blade"
type = "mesh"
source = "models/blade.obj"      # imported...
# ...or, instead of `source`:
positions = [[0, 0, 0], [1, 0, 0], [0, 1, 0]]
indices = [[0, 1, 2]]
```"##;

/// Register `mesh` so scenes, components and scripts all name geometry the
/// same way. A definition is either a reference to a model file or the
/// vertices themselves, which is what lets a script author one at run time:
///
/// ```toml
/// [[assets]]
/// id = "blade"
/// type = "mesh"
/// source = "models/blade.obj"      # imported…
/// # …or, instead of `source`:
/// positions = [[0,0,0], [1,0,0], [0,1,0]]
/// indices = [[0, 1, 2]]
/// ```
pub(crate) fn register_mesh_asset(app: &mut App) {
    app.register_asset_type(MESH_ASSET_TYPE, "models", MESH_ASSET_DOC, |value| {
        Ok(std::rc::Rc::new(parse_definition(value)?) as std::rc::Rc<dyn std::any::Any>)
    });
}

/// A `mesh` definition: `source` names a file to import, or `positions` and
/// `indices` carry the geometry directly. Naming both is a contradiction
/// rather than a precedence puzzle, so it is refused.
fn parse_definition(value: &toml::Value) -> Result<MeshData> {
    let source = value.get("source").and_then(toml::Value::as_str);
    let inline = value.get("positions").is_some() || value.get("indices").is_some();
    match (source, inline) {
        (Some(_), true) => bail!(
            "a mesh names both a `source` file and inline `positions`; it can carry one or the other"
        ),
        (Some(source), false) => {
            // The file is read by the caller that has the engine; an asset
            // parser only sees the definition, so the reference is recorded
            // and resolved at load. See `load_from`.
            Ok(MeshData {
                source: Some(source.to_string()),
                ..MeshData::default()
            })
        }
        (None, true) => parse_inline(value),
        (None, false) => bail!("a mesh needs a `source` file or inline `positions` and `indices`"),
    }
}

/// Inline geometry. `positions` are `[x, y, z]` or, for a 2D polygon,
/// `[x, y]`. Faces come from `indices`, or from `polygons` (index loops,
/// each ear-clipped), or — with neither — from the outline itself: the
/// positions minus the trailing `internal` ones, as one loop.
fn parse_inline(value: &toml::Value) -> Result<MeshData> {
    let positions = points(value, "positions")?;
    if positions.is_empty() {
        bail!("a mesh's `positions` is empty, so there is no geometry in it");
    }
    let internal = value
        .get("internal")
        .map(|v| {
            v.as_integer()
                .ok_or_else(|| anyhow!("a mesh's `internal` is {}, not a count", v.type_str()))
        })
        .transpose()?
        .unwrap_or(0);
    if internal < 0 || internal as usize >= positions.len() {
        bail!(
            "a mesh's `internal = {internal}` leaves no outline out of {} positions",
            positions.len()
        );
    }
    let indices = match (value.get("indices"), value.get("polygons")) {
        (Some(_), Some(_)) => {
            bail!("a mesh names both `indices` and `polygons`; it can carry one or the other")
        }
        (Some(_), None) => explicit_faces(value, positions.len())?,
        (None, polygons) => triangulated(&positions, internal as usize, polygons)?,
    };
    if indices.is_empty() {
        bail!("a mesh has `positions` but no faces, so it declares nothing to fill");
    }
    let uvs = pairs(value, "uvs")?;
    if let Some(uvs) = &uvs {
        if uvs.len() != positions.len() {
            bail!(
                "a mesh has {} `uvs` for {} positions; it needs one per vertex",
                uvs.len(),
                positions.len()
            );
        }
    }
    let skin = parse_skin(value, positions.len())?;
    Ok(MeshData {
        positions,
        indices,
        uvs,
        skin,
        ..MeshData::default()
    })
}

/// The `indices` a definition wrote, checked against its vertex count.
fn explicit_faces(value: &toml::Value, vertices: usize) -> Result<Vec<[u32; 3]>> {
    let indices: Vec<[u32; 3]> = triples(value, "indices")?
        .into_iter()
        .map(|[a, b, c]| [a as u32, b as u32, c as u32])
        .collect();
    let limit = vertices as u32;
    for [a, b, c] in &indices {
        if *a >= limit || *b >= limit || *c >= limit {
            bail!(
                "a mesh face names vertex {} but only {limit} were given",
                a.max(b).max(c)
            );
        }
    }
    Ok(indices)
}

/// Faces from index loops, or from the outline when there are none.
///
/// Interior vertices only ever reach a triangle through a loop that names
/// them, which is the Godot rule: automatic triangulation bends badly, so
/// the moment a polygon has interior points its author draws the polygons.
fn triangulated(
    positions: &[[f32; 3]],
    internal: usize,
    polygons: Option<&toml::Value>,
) -> Result<Vec<[u32; 3]>> {
    let flat: Vec<glamx::Vec2> = positions
        .iter()
        .map(|p| glamx::Vec2::new(p[0], p[1]))
        .collect();
    let Some(polygons) = polygons else {
        if internal > 0 {
            tracing::warn!(
                "a mesh has {internal} internal vertices but no `polygons`; \
                 only its outline is filled"
            );
        }
        let outline: Vec<u32> = (0..(positions.len() - internal) as u32).collect();
        return crate::triangulate::triangulate(&flat, &outline);
    };
    let loops = polygons
        .as_array()
        .ok_or_else(|| anyhow!("a mesh's `polygons` must be a list of index loops"))?;
    let mut out = Vec::new();
    for (i, item) in loops.iter().enumerate() {
        let ring: Vec<u32> = item
            .as_array()
            .ok_or_else(|| anyhow!("a mesh's `polygons[{i}]` is not a list of indices"))?
            .iter()
            .map(|v| {
                v.as_integer()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| anyhow!("a mesh's `polygons[{i}]` holds a non-index"))
            })
            .collect::<Result<_>>()?;
        out.extend(
            crate::triangulate::triangulate(&flat, &ring)
                .map_err(|e| anyhow!("a mesh's `polygons[{i}]`: {e}"))?,
        );
    }
    Ok(out)
}

/// `[[skin.bones]]` — `path` and one `weights` entry per vertex — folded to
/// the four heaviest influences per vertex, ties to the earlier bone.
fn parse_skin(value: &toml::Value, vertices: usize) -> Result<Option<MeshSkin>> {
    let Some(skin) = value.get("skin") else {
        return Ok(None);
    };
    let bones = skin
        .get("bones")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow!("a mesh's `skin` needs a `bones` list"))?;
    let mut names = Vec::with_capacity(bones.len());
    let mut per_vertex: Vec<Vec<(u32, f32)>> = vec![Vec::new(); vertices];
    for (index, bone) in bones.iter().enumerate() {
        let path = bone
            .get("path")
            .and_then(toml::Value::as_str)
            .filter(|p| !p.is_empty())
            .ok_or_else(|| anyhow!("a mesh's `skin.bones[{index}]` needs a `path`"))?;
        let weights = bone
            .get("weights")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| anyhow!("a mesh's `skin.bones[{index}]` needs a `weights` list"))?;
        if weights.len() != vertices {
            bail!(
                "a mesh's `skin.bones[{index}]` has {} weights for {vertices} vertices",
                weights.len()
            );
        }
        for (vertex, weight) in weights.iter().enumerate() {
            let weight = crate::components::as_f64(weight).ok_or_else(|| {
                anyhow!("a mesh's `skin.bones[{index}]` weight {vertex} is not a number")
            })? as f32;
            if weight < 0.0 || !weight.is_finite() {
                bail!("a mesh's `skin.bones[{index}]` weight {vertex} is {weight}; weights are 0 or more");
            }
            if weight > 0.0 {
                per_vertex[vertex].push((index as u32, weight));
            }
        }
        names.push(path.to_string());
    }
    let mut joints = Vec::with_capacity(vertices);
    let mut weights = Vec::with_capacity(vertices);
    for influences in &mut per_vertex {
        // Heaviest first, earlier bone on a tie: `total_cmp` keeps the order
        // the same on every platform.
        influences.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        influences.truncate(INFLUENCES_PER_VERTEX);
        let total: f32 = influences.iter().map(|(_, w)| w).sum();
        let mut j = [0u32; INFLUENCES_PER_VERTEX];
        let mut w = [0f32; INFLUENCES_PER_VERTEX];
        for (slot, (bone, weight)) in influences.iter().enumerate() {
            j[slot] = *bone;
            w[slot] = weight / total;
        }
        joints.push(j);
        weights.push(w);
    }
    Ok(Some(MeshSkin {
        bones: names,
        joints,
        weights,
        inverse_bind: None,
    }))
}

/// `[[x, y, z], ...]` from a definition key.
fn triples(value: &toml::Value, key: &str) -> Result<Vec<[f32; 3]>> {
    rows(value, key, &[3], "[x, y, z] triples")
}

/// `[[x, y, z], ...]` or `[[x, y], ...]`; a pair is a 2D vertex on z = 0.
fn points(value: &toml::Value, key: &str) -> Result<Vec<[f32; 3]>> {
    rows(value, key, &[2, 3], "[x, y] or [x, y, z] points")
}

/// `[[u, v], ...]`, when present.
fn pairs(value: &toml::Value, key: &str) -> Result<Option<Vec<[f32; 2]>>> {
    if value.get(key).is_none() {
        return Ok(None);
    }
    Ok(Some(
        rows(value, key, &[2], "[u, v] pairs")?
            .into_iter()
            .map(|[u, v, _]| [u, v])
            .collect(),
    ))
}

/// Rows of numbers from a definition key, each of one of `lengths`, padded
/// with zeros to three. Empty when the key is absent.
fn rows(value: &toml::Value, key: &str, lengths: &[usize], shape: &str) -> Result<Vec<[f32; 3]>> {
    let Some(rows) = value.get(key) else {
        return Ok(Vec::new());
    };
    let rows = rows
        .as_array()
        .ok_or_else(|| anyhow!("a mesh's `{key}` must be a list of {shape}"))?;
    rows.iter()
        .enumerate()
        .map(|(i, row)| {
            let row = row
                .as_array()
                .filter(|r| lengths.contains(&r.len()))
                .ok_or_else(|| anyhow!("a mesh's `{key}[{i}]` is not one of {shape}"))?;
            let mut out = [0.0f32; 3];
            for (slot, number) in out.iter_mut().zip(row) {
                *slot = crate::components::as_f64(number)
                    .ok_or_else(|| anyhow!("a mesh's `{key}[{i}]` holds a non-number"))?
                    as f32;
            }
            Ok(out)
        })
        .collect()
}

/// A mesh asset's geometry, with a `source` reference followed to the file it
/// names. This is the half `parse_definition` cannot do: an asset parser
/// sees only the definition, and reading a file needs the project reader.
///
/// # Errors
/// If the referenced file is missing, or does not parse.
pub fn load_from(eng: &crate::Engine, definition: &MeshData) -> Result<MeshData> {
    let Some(source) = definition.source.as_deref() else {
        return Ok(definition.clone());
    };
    let files = eng.resource::<crate::project::ProjectFiles>();
    let files = files.borrow();
    let bytes = files.read(source)?;
    // A `.gltf` names its buffers and images relative to itself.
    let directory = std::path::Path::new(source)
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let side = |uri: &str| {
        files.read(&if directory.is_empty() {
            uri.to_string()
        } else {
            format!("{directory}/{uri}")
        })
    };
    let mut mesh = parse_with(&bytes, source, &side)?;
    mesh.source = Some(source.to_string());
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Float arrays compared the way the rest of the workspace does it.
    fn close<const N: usize>(got: [f32; N], want: [f32; N]) -> bool {
        got.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-6)
    }

    const TRIANGLE: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

    #[test]
    fn a_triangle_becomes_three_vertices_and_one_face() {
        let mesh = parse_obj(TRIANGLE.as_bytes(), "t.obj").unwrap();
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.indices, vec![[0, 1, 2]]);
        assert_eq!(mesh.triangle_count(), 1);
        // Nothing declared them, so the backend is free to compute them.
        assert!(mesh.normals.is_none() && mesh.uvs.is_none());
    }

    #[test]
    fn a_quad_is_triangulated_as_a_fan() {
        let quad = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n";
        let mesh = parse_obj(quad.as_bytes(), "q.obj").unwrap();
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.indices, vec![[0, 1, 2], [0, 2, 3]]);
    }

    /// OBJ indexes positions, UVs and normals separately; a GPU mesh has one
    /// index stream, so each distinct triple has to become its own vertex.
    #[test]
    fn corners_sharing_a_position_but_not_a_uv_become_separate_vertices() {
        let source = "v 0 0 0\nv 1 0 0\nv 0 1 0\n\
                      vt 0 0\nvt 1 0\nvt 0 1\nvt 1 1\n\
                      f 1/1 2/2 3/3\nf 1/4 2/2 3/3\n";
        let mesh = parse_obj(source.as_bytes(), "s.obj").unwrap();
        assert_eq!(mesh.indices.len(), 2);
        // Four corners, because vertex 1 appears with two different UVs.
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.uvs.as_ref().unwrap().len(), 4);
    }

    #[test]
    fn a_negative_index_counts_back_from_the_newest_vertex() {
        let source = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n";
        let mesh = parse_obj(source.as_bytes(), "n.obj").unwrap();
        assert_eq!(mesh.indices, vec![[0, 1, 2]]);
        assert!(close(mesh.positions[2], [0.0, 1.0, 0.0]));
    }

    #[test]
    fn comments_materials_and_groups_are_read_past() {
        let source = "# a comment\nmtllib thing.mtl\no Thing\ng Part\ns 1\nusemtl red\n";
        let full = format!("{source}{TRIANGLE}");
        assert_eq!(
            parse_obj(full.as_bytes(), "c.obj").unwrap().indices.len(),
            1
        );
    }

    #[test]
    fn normals_and_uvs_survive_when_the_file_declares_them() {
        let source = "v 0 0 0\nv 1 0 0\nv 0 1 0\n\
                      vt 0 0\nvt 1 0\nvt 0 1\n\
                      vn 0 0 1\nf 1/1/1 2/2/1 3/3/1\n";
        let mesh = parse_obj(source.as_bytes(), "f.obj").unwrap();
        assert!(mesh
            .normals
            .unwrap()
            .iter()
            .all(|n| close(*n, [0.0, 0.0, 1.0])));
        let uvs = mesh.uvs.unwrap();
        assert!(
            close(uvs[0], [0.0, 0.0]) && close(uvs[1], [1.0, 0.0]) && close(uvs[2], [0.0, 1.0])
        );
    }

    #[test]
    fn bounds_cover_every_vertex() {
        let source = "v -1 0 2\nv 3 -4 0\nv 0 5 -6\nf 1 2 3\n";
        let mesh = parse_obj(source.as_bytes(), "b.obj").unwrap();
        let (min, max) = mesh.bounds().expect("a mesh with vertices has bounds");
        assert!(close(min, [-1.0, -4.0, -6.0]) && close(max, [3.0, 5.0, 2.0]));
    }

    #[test]
    fn a_face_naming_a_vertex_that_does_not_exist_is_an_error() {
        let err = parse_obj(b"v 0 0 0\nf 1 2 3\n", "bad.obj")
            .unwrap_err()
            .to_string();
        assert!(err.contains("out of range"), "{err}");
        assert!(
            err.contains("bad.obj:2"),
            "the error should name the line: {err}"
        );
    }

    #[test]
    fn a_file_with_no_faces_is_an_error_rather_than_an_empty_mesh() {
        let err = parse_obj(b"v 0 0 0\n", "empty.obj")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no faces"), "{err}");
    }

    #[test]
    fn an_unknown_extension_names_what_is_supported() {
        let err = parse(b"", "model.fbx").unwrap_err().to_string();
        assert!(err.contains(".fbx") && err.contains(".obj"), "{err}");
    }
    #[test]
    fn a_definition_may_name_a_file_to_import() {
        let value: toml::Value = toml::from_str("source = \"models/hero.obj\"").unwrap();
        let mesh = parse_definition(&value).unwrap();
        assert_eq!(mesh.source.as_deref(), Some("models/hero.obj"));
        // Not read yet: the parser has no engine to read a file with.
        assert!(mesh.positions.is_empty());
    }

    #[test]
    fn a_definition_may_carry_its_vertices_instead() {
        let value: toml::Value =
            toml::from_str("positions = [[0,0,0],[1,0,0],[0,1,0]]\nindices = [[0,1,2]]").unwrap();
        let mesh = parse_definition(&value).unwrap();
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.indices, vec![[0, 1, 2]]);
        assert!(mesh.source.is_none());
    }

    /// Naming both is a contradiction, not a precedence question.
    #[test]
    fn naming_a_source_and_vertices_at_once_is_refused() {
        let value: toml::Value =
            toml::from_str("source = \"a.obj\"\npositions = [[0,0,0]]").unwrap();
        let err = parse_definition(&value).unwrap_err().to_string();
        assert!(err.contains("one or the other"), "{err}");
    }

    #[test]
    fn a_face_naming_a_vertex_the_definition_never_gave_is_refused() {
        let value: toml::Value =
            toml::from_str("positions = [[0,0,0],[1,0,0]]\nindices = [[0,1,2]]").unwrap();
        let err = parse_definition(&value).unwrap_err().to_string();
        assert!(err.contains("only 2 were given"), "{err}");
    }

    #[test]
    fn vertices_without_faces_are_filled_as_one_outline() {
        let value: toml::Value = toml::from_str("positions = [[0,0,0],[1,0,0],[0,1,0]]").unwrap();
        let mesh = parse_definition(&value).unwrap();
        assert_eq!(mesh.indices, vec![[0, 1, 2]]);
    }

    #[test]
    fn an_empty_definition_says_what_it_needs() {
        let value: toml::Value = toml::from_str("").unwrap();
        let err = parse_definition(&value).unwrap_err().to_string();
        assert!(
            err.contains("`source`") && err.contains("positions"),
            "{err}"
        );
    }
}
