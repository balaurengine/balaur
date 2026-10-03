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

/// The morph bindings of group 1, for a material drawing on a device that
/// morphs: one-element stand-ins for a mesh with no targets, which a zero
/// target count keeps the shader from reading.
pub(crate) struct MorphFallback {
    storage: kiss3d::wgpu::Buffer,
    control: kiss3d::wgpu::Buffer,
}

impl MorphFallback {
    pub(crate) fn new() -> Self {
        use kiss3d::wgpu::BufferUsages;
        let ctxt = kiss3d::context::Context::get();
        Self {
            storage: ctxt.create_buffer_init(
                Some("mesh_morph_fallback"),
                &[0u8; 16],
                BufferUsages::STORAGE,
            ),
            control: ctxt.create_buffer_init(
                Some("mesh_morph_control_fallback"),
                bytemuck::bytes_of(&kiss3d::builtin::deform::DeformControl::default()),
                BufferUsages::UNIFORM,
            ),
        }
    }
}

/// One node's morph state: the weights it uploads and which buffers its
/// group was last built over.
#[derive(Default)]
pub(crate) struct NodeMorph {
    control: Option<kiss3d::wgpu::Buffer>,
    /// The deltas the group binds, by address, so a mesh that uploads them
    /// again rebuilds the group.
    bound: Option<[usize; 2]>,
}

/// What group 1's morph bindings point at for one draw.
pub(crate) struct MorphBinding<'a> {
    pub(crate) positions: &'a kiss3d::wgpu::Buffer,
    pub(crate) normals: &'a kiss3d::wgpu::Buffer,
    pub(crate) control: &'a kiss3d::wgpu::Buffer,
}

impl NodeMorph {
    /// Upload this frame's weights and answer what to bind, and whether the
    /// group needs building again because the buffers under it moved.
    ///
    /// A mesh with no targets, or weights that are all missing, binds the
    /// stand-ins: the rest shape.
    pub(crate) fn prepare<'a>(
        &'a mut self,
        mesh: &'a kiss3d::resource::GpuMesh3d,
        weights: &[f32],
        fallback: &'a MorphFallback,
    ) -> (MorphBinding<'a>, bool) {
        use kiss3d::builtin::deform::DeformControl;
        let ctxt = kiss3d::context::Context::get();
        let live = mesh.has_morph() && !weights.is_empty();
        let deltas = live.then(|| mesh.ensure_morph_on_gpu()).flatten();
        let Some((positions, normals)) = deltas else {
            let rebuild = self.bound.take().is_some();
            return (
                MorphBinding {
                    positions: &fallback.storage,
                    normals: &fallback.storage,
                    control: &fallback.control,
                },
                rebuild,
            );
        };
        let mut control = DeformControl::default();
        control.set_weights(weights);
        control.num_vertices = mesh.morph_vertex_count() as u32;
        control.has_morph_normals = u32::from(normals.is_some());
        let buffer = self.control.get_or_insert_with(|| {
            ctxt.create_buffer(&kiss3d::wgpu::BufferDescriptor {
                label: Some("mesh_morph_control"),
                size: std::mem::size_of::<DeformControl>() as u64,
                usage: kiss3d::wgpu::BufferUsages::UNIFORM | kiss3d::wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        ctxt.write_buffer(buffer, 0, bytemuck::bytes_of(&control));
        let normals = normals.unwrap_or(&fallback.storage);
        let key = [
            std::ptr::from_ref(positions) as usize,
            std::ptr::from_ref(normals) as usize,
        ];
        let rebuild = self.bound != Some(key);
        self.bound = Some(key);
        (
            MorphBinding {
                positions,
                normals,
                control: buffer,
            },
            rebuild,
        )
    }
}
