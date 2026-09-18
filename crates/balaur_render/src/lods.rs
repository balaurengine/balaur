//! A model's levels of detail in the windowed renderer: every level's
//! triangles over the one vertex buffer, swapped in by camera distance.
//! Presentation only, so nothing the simulation reads sees which is drawn.

use kiss3d::scene::SceneNode3d;

/// Where the camera stood when the frame was published; the origin before
/// any has been.
pub(crate) fn eye(eng: &balaur_core::Engine) -> glamx::Vec3 {
    eng.try_resource::<crate::ViewportSnapshot3d>()
        .map_or(glamx::Vec3::ZERO, |vp| {
            glamx::Vec3::from_array(vp.borrow().eye)
        })
}

/// A rigid mesh's levels, the camera distance each simpler one takes over
/// at, and which one the node is drawing.
pub(crate) struct Lods {
    levels: Vec<Vec<[u32; 3]>>,
    distances: Vec<f32>,
    shown: usize,
}

impl Lods {
    /// The levels a model's import settings ask for, over the triangles
    /// `faces` it was uploaded with; `None` for a mesh with none.
    pub(crate) fn of(
        eng: &balaur_core::Engine,
        data: &balaur_core::mesh::MeshData,
        faces: &[[u32; 3]],
    ) -> Option<Self> {
        use balaur_core::import::model::{lod_distances, lod_levels};
        let settings = balaur_core::import::resolved(eng, data.source.as_deref()?);
        let simpler = lod_levels(data, &settings.settings);
        (!simpler.is_empty()).then(|| Self {
            distances: lod_distances(&settings.settings, simpler.len()),
            levels: std::iter::once(faces.to_vec()).chain(simpler).collect(),
            shown: 0,
        })
    }

    /// Draw the level `distance` from the camera calls for.
    pub(crate) fn show(&mut self, node: &mut SceneNode3d, distance: f32) {
        let wanted = self.distances.iter().filter(|d| distance >= **d).count();
        if wanted == self.shown {
            return;
        }
        let level = &self.levels[wanted];
        node.modify_faces(&mut |faces| faces.clone_from(level));
        self.shown = wanted;
    }
}
