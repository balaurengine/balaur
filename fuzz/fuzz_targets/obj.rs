//! Any bytes as a Wavefront OBJ model: reading refuses or yields a mesh.
#![no_main]

use balaur_geometry::mesh::parse_obj;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse_obj(data, "fuzz.obj");
});
