//! Morph targets on the GPU: a mesh's named shapes as the buffers the
//! blend reads.
//!
//! The weights that drive them are a component property (`mesh::MorphWeights`),
//! so a clip animates a smile with the tracks it already has; what lives
//! here is only the once-per-upload conversion.

/// A mesh's shapes as the buffers the GPU blends: every target's deltas one
/// after another, padded to four floats because a storage buffer aligns that
/// way. `None` for a mesh with no shapes.
pub(crate) fn targets_of(
    data: &balaur_core::mesh::MeshData,
) -> Option<kiss3d::resource::MorphTargets> {
    if data.morphs.is_empty() || data.positions.is_empty() {
        return None;
    }
    let vertices = data.positions.len();
    let pad = |rows: &[[f32; 3]]| -> Vec<[f32; 4]> {
        (0..vertices)
            .map(|i| {
                let d = rows.get(i).copied().unwrap_or([0.0; 3]);
                [d[0], d[1], d[2], 0.0]
            })
            .collect()
    };
    let positions: Vec<[f32; 4]> = data
        .morphs
        .iter()
        .flat_map(|target| pad(&target.positions))
        .collect();
    // Normals go together or not at all: a partly-normalled set would blend
    // some shapes' lighting and not others.
    let normals: Option<Vec<[f32; 4]>> = data
        .morphs
        .iter()
        .all(|target| target.normals.is_some())
        .then(|| {
            data.morphs
                .iter()
                .flat_map(|target| pad(target.normals.as_deref().unwrap_or_default()))
                .collect()
        });
    Some(kiss3d::resource::MorphTargets::new(
        data.morphs.len(),
        vertices,
        positions,
        normals,
    ))
}
