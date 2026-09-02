// Helpers every Balaur shader may import, mounted as `package::common`.

// Unpacks the three vec4 columns a mat3 is padded to in a uniform block.
fn unpack_mat3(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>) -> mat3x3<f32> {
    return mat3x3<f32>(a.xyz, b.xyz, c.xyz);
}
