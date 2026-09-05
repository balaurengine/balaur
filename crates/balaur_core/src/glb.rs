//! A glTF model — a `.glb`, or a `.gltf` with its side files — as a `mesh`
//! asset and as a scene to import.
//!
//! Two readers over one file. [`parse_gltf`] is what `mesh::parse` calls: the
//! file's triangles, with rigid primitives baked into the file's own frame
//! and skinned ones left in bind space, plus the first skin's joints,
//! weights and inverse bind matrices. [`import`] is what `balaur import`
//! calls: the node hierarchy as scene nodes — joints carrying `bone3d` rest
//! poses — one mesh node at the model's root, and every animation as a clip
//! that keys those nodes by path. Both name nodes the same way, which is
//! what lets a skin in the mesh find its bones in the scene.
//!
//! Buffers come from the binary chunk, from a `data:` URI decoded here, or
//! from a file beside the model through a [`SideReader`] the caller supplies
//! — the project reader at load, the file system at import. Of a material,
//! only the base colour texture is kept, as the mesh node's `texture`;
//! cameras, lights and morph targets are read past.

use anyhow::{Context, Result, anyhow, bail};
use glamx::{Mat4, Quat, Vec3};

use crate::collections::DetHashMap;
use crate::mesh::{INFLUENCES_PER_VERTEX, MeshData, MeshSkin};
use crate::skeleton::euler_from_quat;

/// Reads a file the model names by relative URI — a `.bin` beside a
/// `.gltf`, an image — or says why it cannot.
pub type SideReader<'a> = &'a dyn Fn(&str) -> Result<Vec<u8>>;

/// The reader for a model that must stand alone: a `.glb`, or a `.gltf`
/// whose buffers are all `data:` URIs.
///
/// # Errors
/// Always: the point is the message a side file gets.
pub fn no_side_files(uri: &str) -> Result<Vec<u8>> {
    Err(anyhow!(
        "'{uri}' is a side file, and this reader has no directory to find it in"
    ))
}

/// A loaded file with the node facts every reader needs.
struct Model {
    document: gltf::Document,
    /// One per glTF buffer, in index order.
    buffers: Vec<Vec<u8>>,
    /// The side files the buffers came from, to carry along on import.
    side_files: Vec<(String, Vec<u8>)>,
    /// Unique per node, so a path of names resolves to one node.
    names: Vec<String>,
    parent: Vec<Option<usize>>,
}

impl Model {
    fn load(bytes: &[u8], name: &str, side: SideReader<'_>) -> Result<Self> {
        let file = gltf::Gltf::from_slice(bytes).with_context(|| format!("reading {name}"))?;
        let mut blob = file.blob;
        let document = file.document;
        let mut buffers = Vec::new();
        let mut side_files = Vec::new();
        for buffer in document.buffers() {
            let data = match buffer.source() {
                gltf::buffer::Source::Bin => blob
                    .take()
                    .ok_or_else(|| anyhow!("{name} names its binary chunk and has none"))?,
                gltf::buffer::Source::Uri(uri) => {
                    let data =
                        uri_bytes(uri, side).with_context(|| format!("{name}: buffer '{uri}'"))?;
                    if !uri.starts_with("data:") {
                        side_files.push((percent_decoded(uri), data.clone()));
                    }
                    data
                }
            };
            if data.len() < buffer.length() {
                bail!(
                    "{name}: buffer {} holds {} bytes, fewer than the {} it declares",
                    buffer.index(),
                    data.len(),
                    buffer.length()
                );
            }
            buffers.push(data);
        }
        let count = document.nodes().count();
        let mut parent = vec![None; count];
        for node in document.nodes() {
            for child in node.children() {
                parent[child.index()] = Some(node.index());
            }
        }
        let mut names = Vec::with_capacity(count);
        let mut taken: DetHashMap<String, u32> = DetHashMap::default();
        for node in document.nodes() {
            let base = node
                .name()
                .filter(|n| !n.is_empty())
                .map_or_else(|| format!("Node{}", node.index()), |n| n.replace('/', "_"));
            let seen = taken.entry(base.clone()).or_insert(0);
            *seen += 1;
            names.push(if *seen == 1 {
                base
            } else {
                format!("{base}_{seen}")
            });
        }
        Ok(Self {
            document,
            buffers,
            side_files,
            names,
            parent,
        })
    }

    fn buffer(&self, buffer: &gltf::Buffer<'_>) -> Option<&[u8]> {
        self.buffers.get(buffer.index()).map(Vec::as_slice)
    }

    /// The bytes of a buffer view, for an image the file embeds. The end is
    /// checked rather than added: both numbers come off the file.
    fn view_bytes(&self, view: &gltf::buffer::View<'_>) -> Option<&[u8]> {
        let data = self.buffer(&view.buffer())?;
        let end = view.offset().checked_add(view.length())?;
        data.get(view.offset()..end)
    }

    fn local(&self, node: usize) -> Mat4 {
        let (t, r, s) = self
            .document
            .nodes()
            .nth(node)
            .map_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]), |n| {
                n.transform().decomposed()
            });
        Mat4::from_scale_rotation_translation(
            Vec3::from(s),
            Quat::from_xyzw(r[0], r[1], r[2], r[3]),
            Vec3::from(t),
        )
    }

    /// The node's transform composed from the scene root.
    fn global(&self, node: usize) -> Mat4 {
        let mut matrix = self.local(node);
        for p in self.ancestors(node) {
            matrix = self.local(p) * matrix;
        }
        matrix
    }

    /// Root-first chain of ancestors, the node itself last.
    fn lineage(&self, node: usize) -> Vec<usize> {
        let mut chain = vec![node];
        chain.extend(self.ancestors(node));
        chain.reverse();
        chain
    }

    /// The node's ancestors, nearest first. Bounded because a file's node
    /// graph is whatever the file says and a cycle would walk forever.
    fn ancestors(&self, node: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut current = self.parent[node];
        while let Some(p) = current {
            if out.len() >= self.parent.len() {
                tracing::warn!(node, "the model's node parents form a cycle");
                break;
            }
            out.push(p);
            current = self.parent[p];
        }
        out
    }

    /// The deepest node that is every joint's ancestor or self: the rig
    /// root the skin's bone paths are relative to.
    fn rig_root(&self, skin: &gltf::Skin<'_>) -> Option<usize> {
        if let Some(root) = skin.skeleton() {
            return Some(root.index());
        }
        let mut joints = skin.joints().map(|j| j.index());
        let first = self.lineage(joints.next()?);
        let mut depth = first.len();
        for joint in joints {
            let other = self.lineage(joint);
            let shared = first.iter().zip(&other).take_while(|(a, b)| a == b).count();
            depth = depth.min(shared);
        }
        (depth > 0).then(|| first[depth - 1])
    }

    /// `A/B/C` from `from` down to `node`, empty when they are the same.
    fn path(&self, from: usize, node: usize) -> String {
        let chain = self.lineage(node);
        let start = chain.iter().position(|&n| n == from).map_or(0, |i| i + 1);
        chain[start..]
            .iter()
            .map(|&n| self.names[n].as_str())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn scene_roots(&self) -> Vec<usize> {
        self.document
            .default_scene()
            .or_else(|| self.document.scenes().next())
            .map(|scene| scene.nodes().map(|n| n.index()).collect())
            .unwrap_or_default()
    }

    /// Every node of the scene, parents before children, siblings in order.
    fn scene_nodes(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack: Vec<usize> = self.scene_roots().into_iter().rev().collect();
        while let Some(node) = stack.pop() {
            out.push(node);
            if let Some(n) = self.document.nodes().nth(node) {
                let children: Vec<usize> = n.children().map(|c| c.index()).collect();
                stack.extend(children.into_iter().rev());
            }
        }
        out
    }
}

/// The skin the mesh binds to: the first one in the file.
struct Rig {
    skin_index: usize,
    rig_root: usize,
    joints: Vec<usize>,
    bone_paths: Vec<String>,
    inverse_bind: Vec<Mat4>,
}

impl Rig {
    fn first(model: &Model) -> Result<Option<Self>> {
        let Some(skin) = model.document.skins().next() else {
            return Ok(None);
        };
        let rig_root = model
            .rig_root(&skin)
            .ok_or_else(|| anyhow!("the skin's joints share no ancestor"))?;
        let joints: Vec<usize> = skin.joints().map(|j| j.index()).collect();
        let bone_paths = joints.iter().map(|&j| model.path(rig_root, j)).collect();
        // The file's inverse bind matrices are relative to its scene root;
        // the palette wants them relative to the rig root.
        let rig_global = model.global(rig_root);
        let inverse_bind: Vec<Mat4> = match skin
            .reader(|b| model.buffer(&b))
            .read_inverse_bind_matrices()
        {
            Some(matrices) => matrices
                .map(|m| Mat4::from_cols_array_2d(&m) * rig_global)
                .collect(),
            None => joints
                .iter()
                .map(|&j| model.global(j).inverse() * rig_global)
                .collect(),
        };
        if inverse_bind.len() != joints.len() {
            bail!(
                "the skin has {} inverse bind matrices for {} joints",
                inverse_bind.len(),
                joints.len()
            );
        }
        Ok(Some(Self {
            skin_index: skin.index(),
            rig_root,
            joints,
            bone_paths,
            inverse_bind,
        }))
    }
}

/// A URI's bytes: a `data:` URI is decoded here, anything else is read
/// beside the model.
fn uri_bytes(uri: &str, side: SideReader<'_>) -> Result<Vec<u8>> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (_, payload) = rest
            .split_once(',')
            .ok_or_else(|| anyhow!("a data URI with no payload"))?;
        return base64_decode(payload);
    }
    side(&percent_decoded(uri))
}

/// `%20` and friends back to the characters a file name has.
fn percent_decoded(uri: &str) -> String {
    let bytes = uri.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = bytes
            .get(i + 1..i + 3)
            .filter(|_| bytes[i] == b'%')
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = escaped {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Standard base64 with optional padding, the alphabet a `data:` URI uses.
fn base64_decode(text: &str) -> Result<Vec<u8>> {
    let value = |c: u8| -> Result<u32> {
        Ok(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            other => bail!("'{}' is not a base64 digit", char::from(other)),
        })
    };
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in text.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Vertex streams gathered across primitives, every one per vertex.
#[derive(Default)]
struct Streams {
    mesh: MeshData,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    joints: Vec<[u32; 4]>,
    weights: Vec<[f32; 4]>,
    had_normals: bool,
    had_uvs: bool,
}

impl Streams {
    /// One primitive's vertices, placed by `frame`, with its joints when it
    /// is skinned and empty influences when it is rigid.
    fn add<'a, 's, F>(
        &mut self,
        reader: &gltf::mesh::Reader<'a, 's, F>,
        frame: Mat4,
        skinned: bool,
        name: &str,
    ) -> Result<()>
    where
        F: Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>,
    {
        let base = self.mesh.positions.len() as u32;
        let positions: Vec<[f32; 3]> = reader
            .read_positions()
            .ok_or_else(|| anyhow!("{name}: a primitive has no positions"))?
            .map(|p| frame.transform_point3(Vec3::from(p)).to_array())
            .collect();
        let count = positions.len();
        match reader.read_indices() {
            Some(indices) => {
                let flat: Vec<u32> = indices.into_u32().collect();
                for tri in flat.as_chunks::<3>().0 {
                    self.mesh
                        .indices
                        .push([base + tri[0], base + tri[1], base + tri[2]]);
                }
            }
            None => {
                for i in (0..count as u32).step_by(3) {
                    if i + 2 < count as u32 {
                        self.mesh
                            .indices
                            .push([base + i, base + i + 1, base + i + 2]);
                    }
                }
            }
        }
        match reader.read_normals() {
            Some(ns) => {
                self.had_normals = true;
                self.normals.extend(ns.map(|n| {
                    frame
                        .transform_vector3(Vec3::from(n))
                        .normalize_or_zero()
                        .to_array()
                }));
            }
            None => self
                .normals
                .extend(std::iter::repeat_n([0.0, 1.0, 0.0], count)),
        }
        match reader.read_tex_coords(0) {
            Some(ts) => {
                self.had_uvs = true;
                self.uvs.extend(ts.into_f32());
            }
            None => self.uvs.extend(std::iter::repeat_n([0.0, 0.0], count)),
        }
        if skinned {
            let js = reader
                .read_joints(0)
                .ok_or_else(|| anyhow!("{name}: a skinned primitive has no joints"))?
                .into_u16();
            let ws = reader
                .read_weights(0)
                .ok_or_else(|| anyhow!("{name}: a skinned primitive has no weights"))?
                .into_f32();
            for (j, w) in js.zip(ws) {
                self.joints.push(j.map(u32::from));
                self.weights.push(w);
            }
        } else {
            self.joints
                .extend(std::iter::repeat_n([0; INFLUENCES_PER_VERTEX], count));
            self.weights
                .extend(std::iter::repeat_n([0.0; INFLUENCES_PER_VERTEX], count));
        }
        self.mesh.positions.extend(positions);
        Ok(())
    }
}

/// The file's geometry as one mesh. `side` reads what a `.gltf` names
/// beside itself; a `.glb` never asks it anything.
///
/// # Errors
/// If the file does not read, a buffer is missing, or a primitive is not
/// triangles.
pub fn parse_gltf(bytes: &[u8], name: &str, side: SideReader<'_>) -> Result<MeshData> {
    let model = Model::load(bytes, name, side)?;
    let rig = Rig::first(&model)?;
    let mut streams = Streams::default();
    for node_index in model.scene_nodes() {
        let Some(node) = model.document.nodes().nth(node_index) else {
            continue;
        };
        let Some(mesh) = node.mesh() else {
            continue;
        };
        let skinned = matches!((&rig, node.skin()), (Some(rig), Some(skin)) if skin.index() == rig.skin_index);
        // A skinned primitive is authored in bind space and its node's
        // transform is ignored (the glTF rule); a rigid one bakes its place.
        let frame = if skinned {
            Mat4::IDENTITY
        } else {
            model.global(node_index)
        };
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                bail!(
                    "{name}: mesh '{}' is not triangles",
                    model.names[node_index]
                );
            }
            let reader = primitive.reader(|b| model.buffer(&b));
            streams.add(&reader, frame, skinned, name)?;
        }
    }
    let Streams {
        mut mesh,
        normals,
        uvs,
        joints,
        weights,
        had_normals,
        had_uvs,
    } = streams;
    if mesh.positions.is_empty() {
        bail!("{name} draws no triangles");
    }
    if had_normals {
        mesh.normals = Some(normals);
    }
    if had_uvs {
        mesh.uvs = Some(uvs);
    }
    if let Some(rig) = rig {
        mesh.skin = Some(MeshSkin {
            bones: rig.bone_paths,
            joints,
            weights,
            inverse_bind: Some(rig.inverse_bind),
        });
    }
    Ok(mesh)
}

/// What `balaur import` writes for a model: a scene and, when the file has
/// animations, a clip library.
pub struct GlbImport {
    /// A scene document: the model's root node, its hierarchy with `bone3d`
    /// on every joint, and one `mesh` node.
    pub scene: toml::Value,
    /// An `animation_clip` library with one entry per animation, or `None`.
    pub clips: Option<toml::Value>,
    /// Files to write beside the model under `models/`: the `.bin` a
    /// `.gltf` names, and the base colour texture when there is one.
    pub files: Vec<(String, Vec<u8>)>,
}

impl GlbImport {
    /// The scene as the text `scenes/<stem>.toml` holds.
    pub fn scene_toml(&self) -> Result<String> {
        Ok(toml::to_string(&self.scene)?)
    }

    /// The clip library as the text `animations/<stem>.toml` holds.
    pub fn clips_toml(&self) -> Result<Option<String>> {
        self.clips
            .as_ref()
            .map(|clips| toml::to_string(clips).map_err(Into::into))
            .transpose()
    }
}

fn floats(values: impl IntoIterator<Item = f32>) -> toml::Value {
    toml::Value::Array(
        values
            .into_iter()
            .map(|v| toml::Value::Float(f64::from(v)))
            .collect(),
    )
}

fn slug(name: &str) -> String {
    let mut out = String::from("n_");
    let mut gap = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            gap = false;
        } else if !gap {
            out.push('_');
            gap = true;
        }
    }
    out.trim_end_matches('_').to_string()
}

/// The scene and clips a model becomes. `model_file` is the name the model
/// will have under `models/` (`hero.glb`, `hero.gltf`); its stem names the
/// clip library, `animations/<stem>.toml`, and the scene's root node.
///
/// # Errors
/// If the file does not read.
pub fn import(bytes: &[u8], model_file: &str, side: SideReader<'_>) -> Result<GlbImport> {
    let stem = std::path::Path::new(model_file)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("'{model_file}' has no file name"))?;
    let model = Model::load(bytes, model_file, side)?;
    let rig = Rig::first(&model)?;
    let clips = clips_of(&model);
    let mut files = model.side_files.clone();
    let texture = texture_file(&model, stem, side)?;
    if let Some(file) = &texture {
        files.push(file.clone());
    }
    let root_name = {
        let mut chars = stem.chars();
        chars.next().map_or_else(String::new, |c| {
            c.to_uppercase().collect::<String>() + chars.as_str()
        })
    };
    let root_id = slug(&root_name);
    let mut nodes: Vec<toml::Value> = Vec::new();
    let mut root = toml::map::Map::new();
    root.insert("id".into(), toml::Value::String(root_id.clone()));
    root.insert("name".into(), toml::Value::String(root_name.clone()));
    root.insert("parent".into(), toml::Value::String(String::new()));
    if let Some(first) = clips.as_ref().and_then(first_clip_name) {
        let mut animation = toml::map::Map::new();
        animation.insert(
            "library".into(),
            toml::Value::String(format!("animations/{stem}.toml")),
        );
        animation.insert("autoplay".into(), toml::Value::String(first));
        root.insert("animation".into(), toml::Value::Table(animation));
    }
    nodes.push(toml::Value::Table(root));

    let joints: DetHashMap<usize, ()> = rig
        .as_ref()
        .map(|r| r.joints.iter().map(|&j| (j, ())).collect())
        .unwrap_or_default();
    let mut ids: DetHashMap<usize, String> = DetHashMap::default();
    for node_index in model.scene_nodes() {
        // A leaf that only carried geometry has nothing left to say: its
        // triangles are in the mesh node below.
        let carries_only_a_mesh = model.document.nodes().nth(node_index).is_some_and(|n| {
            n.mesh().is_some() && n.children().len() == 0 && !joints.contains_key(&node_index)
        });
        if carries_only_a_mesh {
            continue;
        }
        let name = model.names[node_index].clone();
        let id = format!("{root_id}_{}", slug(&name).trim_start_matches("n_"));
        ids.insert(node_index, id.clone());
        let parent_id = model.parent[node_index]
            .and_then(|p| ids.get(&p).cloned())
            .unwrap_or_else(|| root_id.clone());
        let (t, r, s) = model
            .document
            .nodes()
            .nth(node_index)
            .map_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]), |n| {
                n.transform().decomposed()
            });
        let euler = euler_from_quat(Quat::from_xyzw(r[0], r[1], r[2], r[3]));
        let mut entry = toml::map::Map::new();
        entry.insert("id".into(), toml::Value::String(id));
        entry.insert("name".into(), toml::Value::String(name));
        entry.insert("parent".into(), toml::Value::String(parent_id));
        entry.insert("position".into(), floats(t));
        entry.insert("rotation_euler".into(), floats(euler.to_array()));
        entry.insert("scale".into(), floats(s));
        if joints.contains_key(&node_index) {
            let mut bone = toml::map::Map::new();
            bone.insert("rest_position".into(), floats(t));
            bone.insert("rest_rotation".into(), floats(euler.to_array()));
            bone.insert("rest_scale".into(), floats(s));
            entry.insert("bone3d".into(), toml::Value::Table(bone));
        }
        nodes.push(toml::Value::Table(entry));
    }

    nodes.push(mesh_node(
        &model,
        rig.as_ref(),
        &root_id,
        &root_name,
        model_file,
        texture.as_ref().map(|(name, _)| name.as_str()),
    ));

    let mut scene = toml::map::Map::new();
    scene.insert("nodes".into(), toml::Value::Array(nodes));
    Ok(GlbImport {
        scene: toml::Value::Table(scene),
        clips,
        files,
    })
}

/// One mesh node for the whole file, at the root: rigid primitives were
/// baked into the root's frame and skinned ones live in bind space.
fn mesh_node(
    model: &Model,
    rig: Option<&Rig>,
    root_id: &str,
    root_name: &str,
    model_file: &str,
    texture: Option<&str>,
) -> toml::Value {
    let mut mesh_node = toml::map::Map::new();
    mesh_node.insert("id".into(), toml::Value::String(format!("{root_id}_mesh")));
    mesh_node.insert(
        "name".into(),
        toml::Value::String(format!("{root_name}Mesh")),
    );
    mesh_node.insert("parent".into(), toml::Value::String(root_id.to_string()));
    // An asset reference names a definition, so the file goes inside one:
    // a table is a definition, and this one just points at the model.
    let mut definition = toml::map::Map::new();
    definition.insert(
        "source".into(),
        toml::Value::String(format!("models/{model_file}")),
    );
    let mut mesh = toml::map::Map::new();
    mesh.insert("source".into(), toml::Value::Table(definition));
    if let Some(texture) = texture {
        mesh.insert(
            "texture".into(),
            toml::Value::String(format!("models/{texture}")),
        );
    }
    if let Some(rig) = rig {
        let scene_roots = model.scene_roots();
        let rig_path = if scene_roots.contains(&rig.rig_root) {
            model.names[rig.rig_root].clone()
        } else {
            let top = model.lineage(rig.rig_root)[0];
            format!("{}/{}", model.names[top], model.path(top, rig.rig_root))
        };
        mesh.insert(
            "skeleton".into(),
            toml::Value::String(format!("../{rig_path}")),
        );
    }
    mesh_node.insert("mesh".into(), toml::Value::Table(mesh));
    toml::Value::Table(mesh_node)
}

/// The first base colour texture the file's materials name, as a file to
/// write under `models/`: a side image as it is, an embedded one as
/// `<stem>_texture.<ext>`.
fn texture_file(
    model: &Model,
    stem: &str,
    side: SideReader<'_>,
) -> Result<Option<(String, Vec<u8>)>> {
    let Some(image) = model.document.materials().find_map(|material| {
        material
            .pbr_metallic_roughness()
            .base_color_texture()
            .map(|info| info.texture().source())
    }) else {
        return Ok(None);
    };
    match image.source() {
        gltf::image::Source::Uri { uri, .. } => {
            if uri.starts_with("data:") {
                let extension = if uri.starts_with("data:image/jpeg") {
                    "jpg"
                } else {
                    "png"
                };
                return Ok(Some((
                    format!("{stem}_texture.{extension}"),
                    uri_bytes(uri, side)?,
                )));
            }
            let name = percent_decoded(uri);
            let bytes = side(&name).with_context(|| format!("texture '{name}'"))?;
            Ok(Some((name, bytes)))
        }
        gltf::image::Source::View { view, mime_type } => {
            let bytes = model
                .view_bytes(&view)
                .ok_or_else(|| anyhow!("the embedded texture's buffer view is out of range"))?;
            let extension = match mime_type {
                "image/jpeg" => "jpg",
                _ => "png",
            };
            Ok(Some((
                format!("{stem}_texture.{extension}"),
                bytes.to_vec(),
            )))
        }
    }
}

fn first_clip_name(clips: &toml::Value) -> Option<String> {
    clips
        .get("clips")
        .and_then(toml::Value::as_table)
        .and_then(|t| t.keys().next().cloned())
}

/// Every animation as a clip keyed by node path from the model's root.
/// Cubic-spline samplers keep their values and drop their tangents, which
/// the clip format has no slot for; morph-target channels are skipped.
fn clips_of(model: &Model) -> Option<toml::Value> {
    let mut clips = toml::map::Map::new();
    for (index, animation) in model.document.animations().enumerate() {
        let name = animation
            .name()
            .filter(|n| !n.is_empty())
            .map_or_else(|| format!("clip{index}"), |n| n.replace(['/', ' '], "_"));
        let mut tracks: Vec<toml::Value> = Vec::new();
        let mut length: f32 = 0.0;
        for channel in animation.channels() {
            let reader = channel.reader(|b| model.buffer(&b));
            let Some(times) = reader.read_inputs() else {
                continue;
            };
            let Some(outputs) = reader.read_outputs() else {
                continue;
            };
            let times: Vec<f32> = times.collect();
            let cubic =
                channel.sampler().interpolation() == gltf::animation::Interpolation::CubicSpline;
            let (property, values): (&str, Vec<Vec<f32>>) = match outputs {
                gltf::animation::util::ReadOutputs::Translations(it) => {
                    ("position", it.map(|v| v.to_vec()).collect())
                }
                gltf::animation::util::ReadOutputs::Scales(it) => {
                    ("scale", it.map(|v| v.to_vec()).collect())
                }
                // Kept as the quaternion the file holds, so a re-export gives
                // the same numbers back.
                gltf::animation::util::ReadOutputs::Rotations(rotations) => (
                    "rotation",
                    rotations.into_f32().map(|q| q.to_vec()).collect(),
                ),
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => continue,
            };
            // A cubic key is (in tangent, value, out tangent); the value is
            // the middle one.
            let values: Vec<Vec<f32>> = if cubic {
                values.chunks(3).filter_map(|c| c.get(1).cloned()).collect()
            } else {
                values
            };
            let keys: Vec<toml::Value> = times
                .iter()
                .zip(&values)
                .map(|(t, v)| {
                    let mut key = toml::map::Map::new();
                    key.insert("t".into(), toml::Value::Float(f64::from(*t)));
                    key.insert("value".into(), floats(v.iter().copied()));
                    toml::Value::Table(key)
                })
                .collect();
            if keys.is_empty() {
                continue;
            }
            length = length.max(times.iter().copied().fold(0.0, f32::max));
            let mut track = toml::map::Map::new();
            let target = model.path(usize::MAX, channel.target().node().index());
            track.insert("target".into(), toml::Value::String(target));
            track.insert("property".into(), toml::Value::String(property.into()));
            let interp = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Step => "step",
                _ => "linear",
            };
            track.insert("interp".into(), toml::Value::String(interp.into()));
            track.insert("keys".into(), toml::Value::Array(keys));
            tracks.push(toml::Value::Table(track));
        }
        if tracks.is_empty() {
            continue;
        }
        let mut clip = toml::map::Map::new();
        clip.insert(
            "length".into(),
            toml::Value::Float(f64::from(length.max(0.001))),
        );
        clip.insert("loop".into(), toml::Value::String("loop".into()));
        clip.insert("tracks".into(), toml::Value::Array(tracks));
        clips.insert(name, toml::Value::Table(clip));
    }
    if clips.is_empty() {
        return None;
    }
    let mut document = toml::map::Map::new();
    document.insert("type".into(), toml::Value::String("animation_clip".into()));
    document.insert("clips".into(), toml::Value::Table(clips));
    Some(toml::Value::Table(document))
}
