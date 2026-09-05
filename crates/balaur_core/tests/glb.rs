//! A `.glb` read two ways: as the mesh a node draws, and as the scene and
//! clips `balaur import` writes. The file is built by hand here — two
//! joints, one skinned quad, one animation — so nothing binary ships with
//! the tests.

use balaur_core::glb;
use balaur_core::mesh::{self, MeshData};
use glamx::{Mat4, Vec3};

/// One accessor's worth of data and the JSON that describes it.
struct Accessor {
    bytes: Vec<u8>,
    component_type: u32,
    kind: &'static str,
    count: usize,
    bounds: Option<([f32; 3], [f32; 3])>,
}

fn f32s(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u16s(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Pack accessors into one buffer, four-byte aligned, and emit the JSON
/// `bufferViews` and `accessors` arrays for them.
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
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// The smallest PNG there is: one white pixel.
const PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xFF, 0xFF, 0x3F,
    0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x35, 0x81, 0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// How the file carries its buffer: inside the `.glb`, beside a `.gltf`, or
/// inline as a `data:` URI.
#[derive(Clone, Copy)]
enum Buffer {
    Bin,
    Side,
    DataUri,
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A column: root joint `Rig` at the origin, child joint `Tip` one unit up,
/// a quad from y = 0 to y = 2 whose bottom row follows `Rig` and top row
/// follows `Tip`, and a one-second clip turning `Tip` a quarter turn about z.
fn column() -> Vec<u8> {
    let (json, bin) = column_parts(Buffer::Bin, false);
    glb(&json, &bin)
}

/// The JSON and the buffer of the column, the buffer carried as `how`, with
/// a one-pixel base colour texture when `textured`.
fn column_parts(how: Buffer, textured: bool) -> (String, Vec<u8>) {
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let mut accessors = vec![
        // 0 positions
        Accessor {
            bytes: f32s(&[-0.5, 0.0, 0.0, 0.5, 0.0, 0.0, -0.5, 2.0, 0.0, 0.5, 2.0, 0.0]),
            component_type: 5126,
            kind: "VEC3",
            count: 4,
            bounds: Some(([-0.5, 0.0, 0.0], [0.5, 2.0, 0.0])),
        },
        // 1 indices
        Accessor {
            bytes: u16s(&[0, 1, 3, 0, 3, 2]),
            component_type: 5123,
            kind: "SCALAR",
            count: 6,
            bounds: None,
        },
        // 2 joints
        Accessor {
            bytes: u16s(&[0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]),
            component_type: 5123,
            kind: "VEC4",
            count: 4,
            bounds: None,
        },
        // 3 weights
        Accessor {
            bytes: f32s(&[
                1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
            ]),
            component_type: 5126,
            kind: "VEC4",
            count: 4,
            bounds: None,
        },
        // 4 inverse bind matrices: identity, and translate(0, -1, 0)
        Accessor {
            bytes: f32s(&[
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 1.0,
            ]),
            component_type: 5126,
            kind: "MAT4",
            count: 2,
            bounds: None,
        },
        // 5 animation times
        Accessor {
            bytes: f32s(&[0.0, 1.0]),
            component_type: 5126,
            kind: "SCALAR",
            count: 2,
            bounds: None,
        },
        // 6 animation rotations: identity, then 90 degrees about z
        Accessor {
            bytes: f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, half, half]),
            component_type: 5126,
            kind: "VEC4",
            count: 2,
            bounds: None,
        },
    ];
    if textured {
        // 7 the image, as a plain buffer view (an accessor entry is emitted
        // too, which glTF allows to go unused).
        accessors.push(Accessor {
            bytes: PIXEL_PNG.to_vec(),
            component_type: 5121,
            kind: "SCALAR",
            count: PIXEL_PNG.len(),
            bounds: None,
        });
    }
    let (bin, views, descs) = pack(&accessors);
    let buffer = match how {
        Buffer::Bin => format!(r#"{{"byteLength":{}}}"#, bin.len()),
        Buffer::Side => format!(r#"{{"byteLength":{},"uri":"column.bin"}}"#, bin.len()),
        Buffer::DataUri => format!(
            r#"{{"byteLength":{},"uri":"data:application/octet-stream;base64,{}"}}"#,
            bin.len(),
            base64(&bin)
        ),
    };
    let material = if textured {
        r#","images":[{"bufferView":7,"mimeType":"image/png"}],"textures":[{"source":0}],
"materials":[{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}]"#
    } else {
        ""
    };
    let primitive_material = if textured { r#","material":0"# } else { "" };
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0,2]}}],
"nodes":[
  {{"name":"Rig","children":[1]}},
  {{"name":"Tip","translation":[0,1,0]}},
  {{"name":"Body","mesh":0,"skin":0}}
],
"skins":[{{"joints":[0,1],"inverseBindMatrices":4}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"JOINTS_0":2,"WEIGHTS_0":3}},"indices":1{primitive_material}}}]}}],
"animations":[{{"name":"wave","channels":[{{"sampler":0,"target":{{"node":1,"path":"rotation"}}}}],
  "samplers":[{{"input":5,"output":6,"interpolation":"LINEAR"}}]}}]{material},
"buffers":[{buffer}],
"bufferViews":[{views}],
"accessors":[{descs}]}}"#
    );
    (json, bin)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn a_glb_becomes_a_mesh_with_its_skin() {
    let data: MeshData = mesh::parse(&column(), "column.glb").unwrap();
    assert_eq!(data.positions.len(), 4);
    assert_eq!(data.indices, vec![[0, 1, 3], [0, 3, 2]]);
    let skin = data.skin.expect("the file has a skin");
    assert_eq!(skin.bones, vec![String::new(), "Tip".to_string()]);
    assert_eq!(skin.joints[2], [1, 0, 0, 0]);
    assert_eq!(
        skin.weights[2].map(f32::to_bits),
        [1.0f32, 0.0, 0.0, 0.0].map(f32::to_bits)
    );
    let bind = skin
        .inverse_bind
        .expect("the file has inverse bind matrices");
    // The rig root sits at the origin, so the file's matrices are already in
    // rig space: the tip's maps its origin (0, 1, 0) back to zero.
    let origin = bind[1].transform_point3(Vec3::new(0.0, 1.0, 0.0));
    assert!(origin.length() < 1e-5, "{origin:?}");
    assert!(bind[0].abs_diff_eq(Mat4::IDENTITY, 1e-6));
}

#[test]
fn a_file_with_no_binary_chunk_is_refused() {
    let bytes = glb(r#"{"asset":{"version":"2.0"}}"#, &[]);
    // A zero-length BIN chunk is no chunk at all to the reader.
    let err = format!("{:#}", mesh::parse(&bytes, "empty.glb").unwrap_err());
    assert!(
        err.contains("binary chunk") || err.contains("no triangles") || err.contains("reading"),
        "{err}"
    );
}

#[test]
fn an_import_writes_bones_a_mesh_node_and_a_clip_keyed_by_path() {
    let imported = glb::import(&column(), "column.glb", &glb::no_side_files).unwrap();
    let nodes = imported.scene.get("nodes").unwrap().as_array().unwrap();
    let names: Vec<&str> = nodes
        .iter()
        .map(|n| n.get("name").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Column", "Rig", "Tip", "ColumnMesh"]);
    let by_name = |name: &str| {
        nodes
            .iter()
            .find(|n| n.get("name").unwrap().as_str() == Some(name))
            .unwrap()
    };

    let tip = by_name("Tip");
    let rest = tip.get("bone3d").unwrap().get("rest_position").unwrap();
    assert!(close(
        rest.as_array().unwrap()[1].as_float().unwrap() as f32,
        1.0
    ));
    assert_eq!(
        tip.get("parent").unwrap().as_str(),
        by_name("Rig").get("id").unwrap().as_str()
    );

    let mesh = by_name("ColumnMesh").get("mesh").unwrap();
    // The file sits inside an inline definition: a reference names a
    // definition, never a binary file.
    assert_eq!(
        mesh.get("source").unwrap().get("source").unwrap().as_str(),
        Some("models/column.glb")
    );
    assert_eq!(mesh.get("skeleton").unwrap().as_str(), Some("../Rig"));

    let root = by_name("Column");
    let animation = root.get("animation").unwrap();
    assert_eq!(
        animation.get("library").unwrap().as_str(),
        Some("animations/column.toml")
    );
    assert_eq!(animation.get("autoplay").unwrap().as_str(), Some("wave"));

    let clips = imported.clips.expect("the file has an animation");
    let wave = clips.get("clips").unwrap().get("wave").unwrap();
    let tracks = wave.get("tracks").unwrap().as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].get("target").unwrap().as_str(), Some("Rig/Tip"));
    assert_eq!(
        tracks[0].get("property").unwrap().as_str(),
        Some("rotation")
    );
    let keys = tracks[0].get("keys").unwrap().as_array().unwrap();
    let last = keys[1].get("value").unwrap().as_array().unwrap();
    // The quaternion the file held, as it was: [0, 0, sin 45°, cos 45°].
    assert_eq!(last.len(), 4);
    assert!(close(
        last[2].as_float().unwrap() as f32,
        std::f32::consts::FRAC_1_SQRT_2
    ));
    assert!(close(
        last[3].as_float().unwrap() as f32,
        std::f32::consts::FRAC_1_SQRT_2
    ));
    toml::to_string(&imported.scene).unwrap();
    toml::to_string(&clips).unwrap();
}

#[test]
fn a_gltf_reads_its_buffer_beside_itself_through_the_reader() {
    let (json, bin) = column_parts(Buffer::Side, false);
    let reader = |uri: &str| {
        if uri == "column.bin" {
            Ok(bin.clone())
        } else {
            Err(anyhow::anyhow!("no such side file '{uri}'"))
        }
    };
    let data = mesh::parse_with(json.as_bytes(), "column.gltf", &reader).unwrap();
    assert_eq!(data.positions.len(), 4);
    assert!(data.skin.is_some());
    // Without a reader the same file says what it is missing.
    let err = format!(
        "{:#}",
        mesh::parse(json.as_bytes(), "column.gltf").unwrap_err()
    );
    assert!(
        err.contains("column.bin") && err.contains("side file"),
        "{err}"
    );
}

#[test]
fn a_data_uri_buffer_needs_no_reader() {
    let (json, _) = column_parts(Buffer::DataUri, false);
    let data = mesh::parse(json.as_bytes(), "column.gltf").unwrap();
    assert_eq!(data.indices.len(), 2);
}

#[test]
fn an_import_carries_the_side_buffer_and_the_texture_along() {
    let (json, bin) = column_parts(Buffer::Side, true);
    let reader = |uri: &str| {
        if uri == "column.bin" {
            Ok(bin.clone())
        } else {
            Err(anyhow::anyhow!("no such side file '{uri}'"))
        }
    };
    let imported = glb::import(json.as_bytes(), "column.gltf", &reader).unwrap();
    let names: Vec<&str> = imported.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["column.bin", "column_texture.png"]);
    assert_eq!(imported.files[1].1, PIXEL_PNG);
    let nodes = imported.scene.get("nodes").unwrap().as_array().unwrap();
    let mesh = nodes
        .iter()
        .find(|n| n.get("name").unwrap().as_str() == Some("ColumnMesh"))
        .unwrap()
        .get("mesh")
        .unwrap();
    assert_eq!(
        mesh.get("source").unwrap().get("source").unwrap().as_str(),
        Some("models/column.gltf")
    );
    assert_eq!(
        mesh.get("texture").unwrap().as_str(),
        Some("models/column_texture.png")
    );
}
