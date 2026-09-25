//! What a solver hands the renderer each step: vertex positions, and the
//! triangles they make, with a count that changes when the triangles do.

/// The vertex positions a solver produced this step, in the node's own space.
///
/// Written on a node by the physics plugin's soft bodies and read by whatever
/// draws it, so a deformable body is another source of vertex positions for
/// the path a skin already goes down. Positions rather than offsets: a soft
/// body's particles *are* the geometry, and subtracting a rest mesh to add it
/// back would be two passes over the vertices for nothing.
///
/// `topology` changes whenever the triangles or the vertex count do, which is
/// what a tear or a rebuilt body does; a renderer holding an uploaded mesh
/// rebuilds it when the number it last saw is not this one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolvedMesh {
    pub positions: Vec<[f32; 3]>,
    /// Triangles, as indices into `positions`.
    pub indices: Vec<[u32; 3]>,
    pub topology: u32,
    /// What to draw it in when the node has nothing of its own to deform.
    pub color: [f32; 4],
}

/// The same in 2D, whose renderer draws a polygon rather than a mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolvedPolygon {
    pub positions: Vec<[f32; 2]>,
    /// Triangles, as indices into `positions`.
    pub indices: Vec<[u32; 3]>,
    pub topology: u32,
    /// As [`SolvedMesh::color`].
    pub color: [f32; 4],
}

// Process-wide rather than per body: a solver's own count restarts at zero
// for a rebuilt body, and a renderer comparing against it keeps stale faces.
static NEXT_TOPOLOGY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

fn next_topology() -> u32 {
    NEXT_TOPOLOGY.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl SolvedMesh {
    /// Take this step's geometry, with a new `topology` if its triangles or
    /// its vertex count are not the ones held.
    pub fn update(&mut self, positions: Vec<[f32; 3]>, indices: Vec<[u32; 3]>) {
        if self.topology == 0 || positions.len() != self.positions.len() || indices != self.indices
        {
            self.topology = next_topology();
            self.indices = indices;
        }
        self.positions = positions;
    }
}

impl SolvedPolygon {
    /// As [`SolvedMesh::update`].
    pub fn update(&mut self, positions: Vec<[f32; 2]>, indices: Vec<[u32; 3]>) {
        if self.topology == 0 || positions.len() != self.positions.len() || indices != self.indices
        {
            self.topology = next_topology();
            self.indices = indices;
        }
        self.positions = positions;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solved_geometry_changes_topology_only_when_its_triangles_or_vertex_count_do() {
        let mut solved = SolvedMesh::default();
        solved.update(vec![[0.0; 3]; 3], vec![[0, 1, 2]]);
        let first = solved.topology;
        assert_ne!(first, 0, "the first geometry is a topology of its own");
        solved.update(vec![[1.0; 3]; 3], vec![[0, 1, 2]]);
        assert_eq!(
            solved.topology, first,
            "moved vertices keep the uploaded faces"
        );
        solved.update(vec![[0.0; 3]; 3], vec![[0, 2, 1]]);
        let torn = solved.topology;
        assert_ne!(torn, first, "new triangles are a new topology");
        solved.update(vec![[0.0; 3]; 4], vec![[0, 2, 1]]);
        assert_ne!(solved.topology, torn, "so is a rebuilt body's vertex count");
    }
}
