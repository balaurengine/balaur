//! The parsers that read bytes somebody else wrote, against bytes nobody
//! wrote: each answers a corrupted input with an error rather than a panic.

#[path = "../../../balaur_core/tests/support/corrupt.rs"]
mod corrupt;
use corrupt::corruptions;

/// A `.glb` is a container with its own lengths, which is where a reader
/// trusting the file walks off the end of it.
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
