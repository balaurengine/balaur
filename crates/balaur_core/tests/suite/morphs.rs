//! Vertex colours and morph targets: what a glTF that carries them used to
//! lose, and what an inline definition can say.

use balaur_core::mesh::{self, MeshData};

/// Four bytes each, little-endian, as a glTF buffer holds them.
fn f32s(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u16s(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// One accessor of the fixture, and where its bytes go.
struct Accessor {
    bytes: Vec<u8>,
    component_type: u32,
    kind: &'static str,
    count: usize,
    /// glTF requires them on a position accessor, morph targets included.
    bounds: Option<([f32; 3], [f32; 3])>,
}

fn pack(accessors: &[Accessor]) -> (Vec<u8>, String, String) {
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut descs = Vec::new();
    for (i, a) in accessors.iter().enumerate() {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(format!(
            r#"{{"buffer":0,"byteOffset":{},"byteLength":{}}}"#,
            bin.len(),
            a.bytes.len()
        ));
        let bounds = a.bounds.map_or(String::new(), |(min, max)| {
            format!(
                r#","min":[{},{},{}],"max":[{},{},{}]"#,
                min[0], min[1], min[2], max[0], max[1], max[2]
            )
        });
        descs.push(format!(
            r#"{{"bufferView":{i},"componentType":{},"count":{},"type":"{}"{bounds}}}"#,
            a.component_type, a.count, a.kind
        ));
        bin.extend_from_slice(&a.bytes);
    }
    (bin, views.join(","), descs.join(","))
}

fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json = json.as_bytes().to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut out = Vec::new();
    let total = 12 + 8 + json.len() + 8 + bin.len();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(bin);
    out
}

/// A quad with per-vertex colours and two named shapes: one that lifts it and
/// one that widens it.
fn painted_quad() -> Vec<u8> {
    let accessors = vec![
        Accessor {
            bytes: f32s(&[-1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 1.0, 0.0, 1.0, 1.0, 0.0]),
            component_type: 5126,
            kind: "VEC3",
            count: 4,
            bounds: Some(([-1.0, 0.0, 0.0], [1.0, 1.0, 0.0])),
        },
        Accessor {
            bytes: u16s(&[0, 1, 3, 0, 3, 2]),
            component_type: 5123,
            kind: "SCALAR",
            count: 6,
            bounds: None,
        },
        // Colours: red, green, blue, white.
        Accessor {
            bytes: f32s(&[
                1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
            ]),
            component_type: 5126,
            kind: "VEC4",
            count: 4,
            bounds: None,
        },
        // "lift": every vertex two units up.
        Accessor {
            bytes: f32s(&[0.0, 2.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 0.0]),
            component_type: 5126,
            kind: "VEC3",
            count: 4,
            bounds: Some(([0.0, 2.0, 0.0], [0.0, 2.0, 0.0])),
        },
        // "widen": the right-hand pair one unit further out.
        Accessor {
            bytes: f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            component_type: 5126,
            kind: "VEC3",
            count: 4,
            bounds: Some(([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])),
        },
    ];
    let (bin, views, descs) = pack(&accessors);
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,
"scenes":[{{"nodes":[0]}}],
"nodes":[{{"mesh":0,"name":"Quad"}}],
"meshes":[{{"name":"Quad","extras":{{"targetNames":["lift","widen"]}},"primitives":[{{"attributes":{{"POSITION":0,"COLOR_0":2}},"indices":1,"targets":[{{"POSITION":3}},{{"POSITION":4}}]}}]}}],
"buffers":[{{"byteLength":{}}}],
"bufferViews":[{views}],
"accessors":[{descs}]}}"#,
        bin.len()
    );
    glb(&json, &bin)
}

fn quad() -> MeshData {
    mesh::parse(&painted_quad(), "quad.glb").expect("the fixture reads")
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "these numbers are read from the file, not computed from it"
)]
fn a_glb_keeps_the_colours_it_carries() {
    let data = quad();
    let colors = data.colors.as_ref().expect("colours survived the import");
    assert_eq!(colors.len(), data.positions.len(), "one per vertex");
    assert_eq!(colors[0], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(colors[1], [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(colors[3], [1.0, 1.0, 1.0, 1.0]);
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "these numbers are read from the file, not computed from it"
)]
fn a_glb_keeps_the_shapes_it_can_blend_towards() {
    let data = quad();
    assert_eq!(data.morphs.len(), 2, "two targets");
    assert_eq!(
        data.morphs[0].name, "lift",
        "named by the file, not numbered"
    );
    assert_eq!(data.morphs[1].name, "widen");
    for target in &data.morphs {
        assert_eq!(
            target.positions.len(),
            data.positions.len(),
            "one delta per vertex"
        );
    }
    assert_eq!(data.morphs[0].positions[0], [0.0, 2.0, 0.0]);
    assert_eq!(
        data.morphs[1].positions[0],
        [0.0, 0.0, 0.0],
        "the left edge stays"
    );
    assert_eq!(
        data.morphs[1].positions[1],
        [1.0, 0.0, 0.0],
        "the right edge moves"
    );
}

/// A mesh with nothing to blend carries no targets, rather than an empty one.
#[test]
fn a_mesh_with_no_shapes_carries_none() {
    let value: toml::Value =
        toml::from_str("positions = [[0, 0, 0], [1, 0, 0], [0, 1, 0]]\nindices = [[0, 1, 2]]")
            .unwrap();
    let data = mesh::parse_definition(&value).expect("it is geometry");
    assert!(data.morphs.is_empty());
    assert!(data.colors.is_none());
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "these numbers are read from the file, not computed from it"
)]
fn an_inline_mesh_can_carry_its_own_colours() {
    let value: toml::Value = toml::from_str(
        r"
positions = [[0, 0, 0], [1, 0, 0], [0, 1, 0]]
indices = [[0, 1, 2]]
colors = [[1, 0, 0], [0, 1, 0, 0.5], [0, 0, 1]]
",
    )
    .unwrap();
    let data = mesh::parse_definition(&value).expect("it parses");
    let colors = data.colors.expect("inline colours");
    assert_eq!(colors[0], [1.0, 0.0, 0.0, 1.0], "no alpha means opaque");
    assert_eq!(colors[1], [0.0, 1.0, 0.0, 0.5]);
}

#[test]
fn a_colour_for_every_vertex_or_none_at_all() {
    let value: toml::Value = toml::from_str(
        "positions = [[0, 0, 0], [1, 0, 0], [0, 1, 0]]\nindices = [[0, 1, 2]]\ncolors = [[1, 0, 0]]",
    )
    .unwrap();
    let refused = mesh::parse_definition(&value);
    assert!(
        refused.is_err(),
        "one colour for three vertices is a mistake"
    );
}
