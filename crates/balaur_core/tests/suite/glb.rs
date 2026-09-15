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
    // The image, and the sidecar carrying the sampler the file asked for.
    assert_eq!(
        names,
        vec!["column.bin", "column_0.png", "column_0.png.toml"]
    );
    // The `.bin` is named rather than carried: importing a model never holds
    // the files it only copies.
    assert!(
        imported.files[0].1.bytes().is_none(),
        "the side buffer should be named, not held"
    );
    assert_eq!(imported.files[1].1.bytes().unwrap(), PIXEL_PNG);
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
    // The base colour reaches the surface through the material's `albedo`
    // slot now, rather than through the node's one texture key.
    let material = imported.scene.get("assets").unwrap().as_array().unwrap();
    assert_eq!(material.len(), 1);
    assert_eq!(
        mesh.get("material").unwrap().as_str(),
        Some(format!("#{}", material[0].get("id").unwrap().as_str().unwrap()).as_str())
    );
    assert_eq!(
        material[0]
            .get("params")
            .unwrap()
            .get("albedo")
            .unwrap()
            .as_str(),
        Some("models/column_0.png")
    );
    assert!(mesh.get("texture").is_none());
    // The shader the material draws with goes into the project beside it.
    let documents: Vec<&str> = imported.documents.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(documents, vec![glb::MATERIAL_SHADER_PATH]);
}

/// Two triangles, one per material, with the factors and maps a real export
/// carries: enough to say what `balaur import` keeps of a glTF material.
///
/// Written by hand rather than exported so the numbers under test are the
/// ones written here.
fn two_materials() -> String {
    let positions = f32s(&[
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 0.0, 0.0, 3.0, 0.0, 0.0, 2.0, 1.0, 0.0,
    ]);
    let indices = u16s(&[0, 1, 2, 3, 4, 5]);
    let (bin, views, descs) = pack(&[
        Accessor {
            bytes: positions,
            component_type: 5126,
            kind: "VEC3",
            count: 6,
            bounds: Some(([0.0, 0.0, 0.0], [3.0, 1.0, 0.0])),
        },
        Accessor {
            bytes: indices,
            component_type: 5123,
            kind: "SCALAR",
            count: 6,
            bounds: None,
        },
        Accessor {
            bytes: PIXEL_PNG.to_vec(),
            component_type: 5121,
            kind: "SCALAR",
            count: PIXEL_PNG.len(),
            bounds: None,
        },
    ]);
    format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
"nodes":[{{"name":"Body","mesh":0}}],
"meshes":[{{"primitives":[
  {{"attributes":{{"POSITION":0}},"indices":1,"material":0}},
  {{"attributes":{{"POSITION":0}},"indices":1,"material":1}}
]}}],
"images":[{{"bufferView":2,"mimeType":"image/png"}},
          {{"bufferView":2,"mimeType":"image/png"}}],
"textures":[{{"source":0}},{{"source":1}}],
"materials":[
  {{"name":"Stone","doubleSided":true,"alphaMode":"MASK","alphaCutoff":0.25,
    "emissiveFactor":[0.1,0.2,0.3],
    "pbrMetallicRoughness":{{"baseColorFactor":[0.8,0.6,0.4,1.0],
      "metallicFactor":0.25,"roughnessFactor":0.75,
      "baseColorTexture":{{"index":0}},"metallicRoughnessTexture":{{"index":0}}}},
    "normalTexture":{{"index":1}},"occlusionTexture":{{"index":0}}}},
  {{"name":"Pane","alphaMode":"OPAQUE",
    "pbrMetallicRoughness":{{"baseColorFactor":[1.0,1.0,1.0,1.0],
      "metallicFactor":0.0,"roughnessFactor":0.05}},
    "extensions":{{"KHR_materials_transmission":{{"transmissionFactor":0.9}},
      "KHR_materials_ior":{{"ior":1.52}},
      "KHR_materials_volume":{{"thicknessFactor":0.2,
        "attenuationColor":[0.9,0.97,0.94],"attenuationDistance":3.0}}}}}}
],
"buffers":[{{"byteLength":{},"uri":"data:application/octet-stream;base64,{}"}}],
"bufferViews":[{views}],
"accessors":[{descs}]}}"#,
        bin.len(),
        base64(&bin)
    )
}

fn imported_two_materials() -> glb::GlbImport {
    glb::import(two_materials().as_bytes(), "hall.gltf", &glb::no_side_files).unwrap()
}

#[test]
fn a_material_keeps_its_factors_and_every_map_it_names() {
    let imported = imported_two_materials();
    let assets = imported.scene.get("assets").unwrap().as_array().unwrap();
    let stone = assets
        .iter()
        .find(|a| a.get("id").unwrap().as_str() == Some("hall_stone"))
        .expect("the file names a Stone material");
    let params = stone.get("params").unwrap();
    let base = params.get("base_color").unwrap().as_array().unwrap();
    assert!(close(base[0].as_float().unwrap() as f32, 0.8));
    assert!(close(base[2].as_float().unwrap() as f32, 0.4));
    assert!(close(
        params.get("metallic").unwrap().as_float().unwrap() as f32,
        0.25
    ));
    assert!(close(
        params.get("roughness").unwrap().as_float().unwrap() as f32,
        0.75
    ));
    let emissive = params.get("emissive").unwrap().as_array().unwrap();
    assert!(close(emissive[1].as_float().unwrap() as f32, 0.2));
    // The four slots this material named. The normal map is the file's second
    // image, the rest its first.
    for slot in ["albedo", "metallic_roughness", "occlusion"] {
        assert_eq!(
            params.get(slot).unwrap().as_str(),
            Some("models/hall_0.png"),
            "{slot}"
        );
    }
    assert_eq!(
        params.get("normal").unwrap().as_str(),
        Some("models/hall_1.png")
    );
    assert!(params.get("emissive_map").is_none());
    // A map the file did not name turns its feature off, so the factor
    // stands where glTF says a missing map is one.
    let features = stone.get("features").unwrap();
    assert_eq!(
        features.get("metallic_roughness_map").unwrap().as_bool(),
        Some(true)
    );
    assert_eq!(features.get("emissive_map").unwrap().as_bool(), Some(false));
    // An image is named once, however many slots point at it, and each gets a
    // sidecar of its own.
    let files: Vec<&str> = imported.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        files,
        vec![
            "hall_0.png",
            "hall_0.png.toml",
            "hall_1.png",
            "hall_1.png.toml"
        ]
    );
}

#[test]
fn a_masked_material_carries_its_cutoff_and_draws_both_sides() {
    let imported = imported_two_materials();
    let assets = imported.scene.get("assets").unwrap().as_array().unwrap();
    let stone = assets
        .iter()
        .find(|a| a.get("id").unwrap().as_str() == Some("hall_stone"))
        .unwrap();
    let surface = stone.get("surface").unwrap();
    assert_eq!(surface.get("alpha").unwrap().as_str(), Some("mask"));
    assert!(close(
        surface.get("alpha_cutoff").unwrap().as_float().unwrap() as f32,
        0.25
    ));
    assert_eq!(surface.get("double_sided").unwrap().as_bool(), Some(true));
}

#[test]
fn a_transmissive_material_becomes_glass_with_its_volume() {
    let imported = imported_two_materials();
    let assets = imported.scene.get("assets").unwrap().as_array().unwrap();
    let pane = assets
        .iter()
        .find(|a| a.get("id").unwrap().as_str() == Some("hall_pane"))
        .expect("the file names a Pane material");
    let surface = pane.get("surface").unwrap();
    assert!(close(
        surface.get("transmission").unwrap().as_float().unwrap() as f32,
        0.9
    ));
    assert!(close(
        surface.get("ior").unwrap().as_float().unwrap() as f32,
        1.52
    ));
    assert!(close(
        surface.get("thickness").unwrap().as_float().unwrap() as f32,
        0.2
    ));
    assert!(close(
        surface
            .get("attenuation_distance")
            .unwrap()
            .as_float()
            .unwrap() as f32,
        3.0
    ));
    // The shader reads the same numbers, so the node that draws as glass
    // shades as glass too.
    let params = pane.get("params").unwrap();
    assert!(close(
        params.get("transmission").unwrap().as_float().unwrap() as f32,
        0.9
    ));
    let attenuation = params.get("attenuation").unwrap().as_array().unwrap();
    assert!(close(attenuation[1].as_float().unwrap() as f32, 0.97));
    assert!(close(attenuation[3].as_float().unwrap() as f32, 3.0));
    // An index of refraction of 1.52 is a touch above the 0.5 that spells the
    // 4% of common glass.
    let reflectance = params.get("reflectance").unwrap().as_float().unwrap() as f32;
    assert!((0.5..0.53).contains(&reflectance), "{reflectance}");
}

#[test]
fn a_model_with_two_materials_becomes_one_mesh_node_for_each() {
    let imported = imported_two_materials();
    let nodes = imported.scene.get("nodes").unwrap().as_array().unwrap();
    let names: Vec<&str> = nodes
        .iter()
        .map(|n| n.get("name").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Hall", "HallMesh_stone", "HallMesh_pane"]);
    let mesh_of = |name: &str| {
        nodes
            .iter()
            .find(|n| n.get("name").unwrap().as_str() == Some(name))
            .unwrap()
            .get("mesh")
            .unwrap()
            .clone()
    };
    let stone = mesh_of("HallMesh_stone");
    assert_eq!(
        stone.get("source").unwrap().get("part").unwrap().as_str(),
        Some("Stone")
    );
    assert_eq!(stone.get("material").unwrap().as_str(), Some("#hall_stone"));
    assert_eq!(
        mesh_of("HallMesh_pane")
            .get("source")
            .unwrap()
            .get("part")
            .unwrap()
            .as_str(),
        Some("Pane")
    );
}

#[test]
fn a_part_takes_only_that_materials_triangles() {
    let json = two_materials();
    let whole = mesh::parse(json.as_bytes(), "hall.gltf").unwrap();
    assert_eq!(whole.indices.len(), 4, "both primitives, drawn together");
    let stone = mesh::parse_part(
        json.as_bytes(),
        "hall.gltf",
        &glb::no_side_files,
        Some("Stone"),
    )
    .unwrap();
    assert_eq!(stone.indices.len(), 2);
    assert_eq!(stone.part.as_deref(), None, "the parser does not record it");
}

#[test]
fn a_part_the_file_does_not_name_says_what_it_does_name() {
    let json = two_materials();
    let err = format!(
        "{:#}",
        mesh::parse_part(
            json.as_bytes(),
            "hall.gltf",
            &glb::no_side_files,
            Some("Marble"),
        )
        .unwrap_err()
    );
    assert!(
        err.contains("Marble") && err.contains("Stone") && err.contains("Pane"),
        "{err}"
    );
}

/// A texture is copied, never decoded, so the import does not read one.
///
/// Sponza's 69 images are 41 MB of its 50, and holding them to write them is
/// what made importing it cost the whole model in memory.
#[test]
fn an_image_the_file_names_is_not_read_while_importing() {
    let (plain, bin) = column_parts(Buffer::Side, false);
    let json = plain
        .replace(r#""indices":1}]}]"#, r#""indices":1,"material":0}]}]"#)
        .replace(
            r#""buffers":["#,
            r#""images":[{"uri":"stone.png"}],"textures":[{"source":0}],
"materials":[{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}],
"buffers":["#,
        );
    let asked = std::cell::RefCell::new(Vec::new());
    let reader = |uri: &str| {
        asked.borrow_mut().push(uri.to_string());
        if uri == "column.bin" {
            Ok(bin.clone())
        } else {
            Err(anyhow::anyhow!("'{uri}' should not be read here"))
        }
    };
    let imported = glb::import(json.as_bytes(), "column.gltf", &reader).unwrap();
    assert_eq!(
        *asked.borrow(),
        vec!["column.bin".to_string()],
        "only the buffer the geometry needs is read"
    );
    let stone = imported
        .files
        .iter()
        .find(|(name, _)| name == "stone.png")
        .expect("the named image is carried along");
    assert!(
        stone.1.bytes().is_none(),
        "a named image is a name, not bytes"
    );
    // Its sampler is written here, so that one is bytes.
    let sidecar = imported
        .files
        .iter()
        .find(|(name, _)| name == "stone.png.toml")
        .expect("a sidecar beside it");
    assert!(sidecar.1.bytes().is_some());
}

/// A map is only half of what a file says about a texture; the other half is
/// how to sample it, and Balaur's own defaults are not glTF's.
///
/// A floor whose UVs run past one and whose sampler was dropped clamps to a
/// single texel and draws flat, which is what the sidecar exists to stop.
#[test]
fn a_texture_keeps_the_sampler_the_file_gave_it() {
    let imported = imported_two_materials();
    let sidecar: Vec<&(String, glb::Beside)> = imported
        .files
        .iter()
        .filter(|(name, _)| name.ends_with(".toml"))
        .collect();
    assert_eq!(sidecar.len(), 2, "one sidecar beside each image");
    assert_eq!(sidecar[0].0, "hall_0.png.toml");
    let text = String::from_utf8(sidecar[0].1.bytes().unwrap().to_vec()).unwrap();
    let settings: toml::Value = toml::from_str(&text).unwrap();
    // The fixture names no sampler, so glTF's own defaults apply: repeat, and
    // a mip chain, both of which Balaur would otherwise have turned off.
    assert_eq!(settings.get("repeat_u").unwrap().as_str(), Some("repeat"));
    assert_eq!(settings.get("repeat_v").unwrap().as_str(), Some("repeat"));
    assert_eq!(settings.get("mipmaps").unwrap().as_bool(), Some(true));
    assert_eq!(settings.get("filter").unwrap().as_str(), Some("linear"));
    // A model is walked at a glancing angle, where a mip chain on its own
    // blurs a floor into bands.
    assert_eq!(settings.get("anisotropy").unwrap().as_integer(), Some(16));
}

/// sRGB describes colour, and a normal or a roughness is not colour: read back
/// through that curve a stored roughness of 0.19 becomes 0.03, and the scene
/// of mirrors that makes turns every glancing surface white.
#[test]
fn only_the_colour_maps_are_marked_srgb() {
    let imported = imported_two_materials();
    let of = |name: &str| -> toml::Value {
        let bytes = imported
            .files
            .iter()
            .find(|(f, _)| f == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1
            .bytes()
            .unwrap_or_else(|| panic!("{name} is named rather than carried"))
            .to_vec();
        toml::from_str(&String::from_utf8(bytes).unwrap()).unwrap()
    };
    assert_eq!(
        of("hall_0.png.toml").get("srgb").unwrap().as_bool(),
        Some(true),
        "the base colour is colour"
    );
    assert_eq!(
        of("hall_1.png.toml").get("srgb").unwrap().as_bool(),
        Some(false),
        "the normal map is not"
    );
}
