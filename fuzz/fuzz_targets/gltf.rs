//! Any bytes as a glTF or GLB model, with no file beside it: reading the mesh
//! and importing the scene and clips refuse or answer.
#![no_main]

use balaur_asset::glb::{import, parse_gltf};
use libfuzzer_sys::fuzz_target;

fn no_side_file(name: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::bail!("{name}: no file beside the model")
}

fuzz_target!(|data: &[u8]| {
    let _ = parse_gltf(data, "fuzz.glb", &no_side_file);
    let _ = import(data, "fuzz.glb", &no_side_file);
});
