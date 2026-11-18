//! The parsers that read bytes somebody else wrote, against bytes nobody
//! wrote: each answers a corrupted input with an error rather than a panic.

#[path = "../../../balaur_core/tests/support/corrupt.rs"]
mod corrupt;
use corrupt::corruptions;

// A `.glb` is a container with its own lengths, which is where a reader
// trusting the file walks off the end of it.
#[test]
fn a_corrupted_glb_is_an_error_and_never_a_panic() {
    // 12-byte header, then one JSON chunk holding the smallest legal document.
    let json = br#"{"asset":{"version":"2.0"},"meshes":[],"nodes":[],"scenes":[]}"#;
    let padded = json.len().next_multiple_of(4);
    let mut chunk = json.to_vec();
    chunk.resize(padded, b' ');
    let mut valid = Vec::new();
    valid.extend_from_slice(b"glTF");
    valid.extend_from_slice(&2u32.to_le_bytes());
    valid.extend_from_slice(&((12 + 8 + padded) as u32).to_le_bytes());
    valid.extend_from_slice(&(padded as u32).to_le_bytes());
    valid.extend_from_slice(b"JSON");
    valid.extend_from_slice(&chunk);

    for (case, bytes) in corruptions(&valid, 2000).into_iter().enumerate() {
        let read = std::panic::catch_unwind(|| {
            balaur_asset::glb::parse_gltf(&bytes, "corrupt.glb", &|_| {
                Err(anyhow::anyhow!("no side files"))
            })
        });
        assert!(
            read.is_ok(),
            "case {case}: parse_gltf panicked on {} bytes",
            bytes.len()
        );
    }
}

#[test]
fn a_glb_declaring_less_than_its_header_is_an_error() {
    let mut bytes = b"glTF".to_vec();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    let err = balaur_asset::glb::parse_gltf(&bytes, "short.glb", &|_| {
        Err(anyhow::anyhow!("no side files"))
    })
    .unwrap_err();
    assert!(
        format!("{err:#}").contains("shorter than its own header"),
        "{err:#}"
    );
}

#[test]
fn a_mesh_naming_a_position_accessor_it_has_not_got_is_an_error() {
    let json =
        br#"{"asset":{"version":"2.0"},"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}]}"#;
    let err = balaur_asset::glb::parse_gltf(json, "bare.gltf", &|_| {
        Err(anyhow::anyhow!("no side files"))
    })
    .unwrap_err();
    assert!(
        format!("{err:#}").contains("position accessor it has not got"),
        "{err:#}"
    );
}

#[test]
fn nodes_that_are_each_others_children_are_read_once() {
    let json = br#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
"nodes":[{"name":"A","children":[1]},{"name":"B","children":[0]}]}"#;
    let imported = balaur_asset::glb::import(json, "loop.gltf", &|_| {
        Err(anyhow::anyhow!("no side files"))
    })
    .unwrap();
    let names: Vec<&str> = imported.scene["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n.get("name")?.as_str())
        .collect();
    for name in ["A", "B"] {
        assert_eq!(names.iter().filter(|&&n| n == name).count(), 1, "{names:?}");
    }
}
