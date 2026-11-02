//! A model's import settings applied to its mesh: the simpler copies its
//! `lods` ask for, and its `scale` and `offset`.

use crate::mesh::MeshData;
use balaur_core::import::{keys, number};

/// The most simpler copies a model may ask for.
pub const MAX_LODS: usize = 6;
/// Where the first simpler copy takes over when the model does not say.
pub const DEFAULT_LOD_DISTANCE: f32 = 20.0;

/// The triangles of each simpler copy a model's `lods` asks for, level
/// one first; empty for none. A skinned or morphing mesh gets none: its
/// vertices move, and a copy simplified at rest would tear.
#[must_use]
pub fn lod_levels(mesh: &MeshData, settings: &toml::Table) -> Vec<Vec<[u32; 3]>> {
    let wanted = number(settings, keys::LODS, 0.0).clamp(0.0, MAX_LODS as f64) as usize;
    if wanted == 0 || mesh.skin.is_some() || !mesh.morphs.is_empty() {
        return Vec::new();
    }
    let source: Vec<u32> = mesh.indices.iter().flatten().copied().collect();
    let mut levels = Vec::new();
    let mut held = source.len();
    // A farther copy may stray further from the shape: 2% of the model's
    // size at the first, doubling from there.
    let mut error = 0.02f32;
    for _ in 0..wanted {
        let target = (held / 2) / 3 * 3;
        if target < 3 {
            break;
        }
        let mut out = vec![0u32; source.len()];
        let kept =
            meshopt_rs::simplify::simplify(&mut out, &source, &mesh.positions, target, error);
        if kept == 0 || kept >= held {
            break;
        }
        out.truncate(kept);
        held = kept;
        levels.push(out.as_chunks::<3>().0.to_vec());
        error = (error * 2.0).min(0.5);
    }
    levels
}

/// The camera distance each simpler copy takes over at, level one first.
#[must_use]
pub fn lod_distances(settings: &toml::Table, levels: usize) -> Vec<f32> {
    let first = number(
        settings,
        keys::LOD_DISTANCE,
        f64::from(DEFAULT_LOD_DISTANCE),
    );
    let first = if first > 0.0 {
        first as f32
    } else {
        DEFAULT_LOD_DISTANCE
    };
    std::iter::successors(Some(first), |d| Some(d * 2.0))
        .take(levels)
        .collect()
}

/// A model file's `scale` and `offset`, applied to what a mesh reads from it:
/// a file modelled in centimetres, or around a corner rather than its centre.
/// A rig's bones are nodes a scene places, so a skinned mesh is left as is.
pub fn place(mesh: &mut MeshData, settings: &toml::Table, source: &str) {
    let triple = |key: &str, fallback: f32| -> [f32; 3] {
        match settings.get(key) {
            Some(toml::Value::Array(items)) if items.len() == 3 => {
                let at = |i: usize| match &items[i] {
                    toml::Value::Float(f) => *f as f32,
                    toml::Value::Integer(n) => *n as f32,
                    _ => fallback,
                };
                [at(0), at(1), at(2)]
            }
            _ => [number(settings, key, f64::from(fallback)) as f32; 3],
        }
    };
    let scale = triple(keys::SCALE, 1.0).map(|s| if s.abs() < 1e-6 { 1.0 } else { s });
    let offset = triple(keys::OFFSET, 0.0);
    let unit = scale.iter().all(|s| (s - 1.0).abs() < f32::EPSILON);
    if unit && offset.iter().all(|o| o.abs() < f32::EPSILON) {
        return;
    }
    if mesh.skin.is_some() {
        tracing::warn!(
            "{source}: `scale` and `offset` move no rig, so a skinned mesh is left as authored"
        );
        return;
    }
    for p in &mut mesh.positions {
        *p = [0, 1, 2].map(|i| p[i] * scale[i] + offset[i]);
    }
    for morph in &mut mesh.morphs {
        for d in &mut morph.positions {
            *d = [0, 1, 2].map(|i| d[i] * scale[i]);
        }
    }
    if let Some(normals) = &mut mesh.normals {
        for n in normals.iter_mut() {
            let bent = [0, 1, 2].map(|i| n[i] / scale[i]);
            let length = bent.iter().map(|c| c * c).sum::<f32>().sqrt().max(1e-12);
            *n = bent.map(|c| c / length);
        }
    }
    // A mirror turns every triangle inside out; swapping two corners puts it back.
    if scale.iter().filter(|s| **s < 0.0).count() % 2 == 1 {
        for triangle in &mut mesh.indices {
            triangle.swap(1, 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::place;

    const TRIANGLE: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

    fn close(got: [f32; 3], want: [f32; 3]) -> bool {
        got.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-6)
    }

    /// A grid of `side` by `side` quads in the z = 0 plane.
    fn grid(side: u32) -> crate::mesh::MeshData {
        let mut mesh = crate::mesh::MeshData::default();
        for y in 0..=side {
            for x in 0..=side {
                let bump = ((x * 7 + y * 3) % 5) as f32 * 0.01;
                mesh.positions.push([x as f32, y as f32, bump]);
            }
        }
        let at = |x: u32, y: u32| y * (side + 1) + x;
        for y in 0..side {
            for x in 0..side {
                mesh.indices
                    .push([at(x, y), at(x + 1, y), at(x + 1, y + 1)]);
                mesh.indices
                    .push([at(x, y), at(x + 1, y + 1), at(x, y + 1)]);
            }
        }
        mesh
    }

    /// Each simpler copy has at most half the triangles of the one before.
    #[test]
    fn a_model_asking_for_lods_gets_simpler_copies() {
        let mesh = grid(16);
        let settings: toml::Table = toml::from_str("lods = 2\nlod_distance = 8").unwrap();
        let levels = super::lod_levels(&mesh, &settings);
        assert_eq!(levels.len(), 2);
        assert!(
            levels[0].len() * 2 <= mesh.indices.len() + 2,
            "{}",
            levels[0].len()
        );
        assert!(levels[1].len() < levels[0].len());
        assert_eq!(super::lod_distances(&settings, 2), vec![8.0, 16.0]);
        assert!(super::lod_levels(&mesh, &toml::Table::new()).is_empty());
    }

    /// A model file's own `scale` and `offset` place its geometry.
    #[test]
    fn a_model_is_scaled_and_moved_by_its_settings() {
        let mut mesh = crate::mesh::parse_obj(TRIANGLE.as_bytes(), "t.obj").unwrap();
        let settings: toml::Table = toml::from_str("scale = 0.01\noffset = [0, 1, 0]").unwrap();
        place(&mut mesh, &settings, "t.obj");
        assert!(close(mesh.positions[1], [0.01, 1.0, 0.0]));
        let mirrored: toml::Table = toml::from_str("scale = [-1, 1, 1]").unwrap();
        let before = mesh.indices[0];
        place(&mut mesh, &mirrored, "t.obj");
        assert_eq!(
            mesh.indices[0],
            [before[0], before[2], before[1]],
            "a mirror keeps its facing"
        );
    }
}
