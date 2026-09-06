//! Constructive solid geometry: two meshes joined, cut or intersected.
//!
//! A BSP tree over the faces, the algorithm every CSG library is a variant
//! of: each solid's faces are clipped against the other's tree until only
//! the ones the operation keeps are left. Written out because parry offers
//! `intersect_meshes` and nothing else -- no union, no difference -- and
//! because a BSP is dot products, lerps and comparisons with no
//! transcendental anywhere, so the result is the same on every platform.

use crate::mesh::MeshData;
use glamx::Vec3;

/// Which side of a plane a point counts as being on. Below this a vertex is
/// treated as lying in the plane, which is what stops a face that is almost
/// coplanar with a cut from being split into slivers.
const ON_PLANE: f32 = 1e-5;

/// How deep the tree may go before it stops dividing. A mesh with thousands
/// of distinct planes would otherwise recurse as far as it has faces.
const MAX_DEPTH: u32 = 64;

/// What to keep of two solids.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// Everything inside either.
    Union,
    /// Everything inside the first and outside the second.
    Difference,
    /// Everything inside both.
    Intersection,
}

impl Op {
    /// The word this operation answers to.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Union => words::UNION,
            Self::Difference => words::DIFFERENCE,
            Self::Intersection => words::INTERSECTION,
        }
    }

    /// The operation a scene names, or `None` for a word that is not one.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            words::UNION => Some(Self::Union),
            words::DIFFERENCE => Some(Self::Difference),
            words::INTERSECTION => Some(Self::Intersection),
            _ => None,
        }
    }
}

/// The three operations, spelled once for a schema, a scene and a script.
pub mod words {
    pub const UNION: &str = "union";
    pub const DIFFERENCE: &str = "difference";
    pub const INTERSECTION: &str = "intersection";
    /// In the order an inspector offers them.
    pub const OPS: &[&str] = &[UNION, DIFFERENCE, INTERSECTION];
}

/// One vertex of a face, carrying what a mesh needs to keep across a cut.
#[derive(Clone, Copy, Debug)]
struct Corner {
    position: Vec3,
    normal: Vec3,
    uv: [f32; 2],
}

impl Corner {
    /// The corner `t` of the way to another: what a split writes where the
    /// cut crosses an edge.
    fn mix(self, other: Self, t: f32) -> Self {
        Self {
            position: self.position + (other.position - self.position) * t,
            normal: (self.normal + (other.normal - self.normal) * t).normalize_or_zero(),
            uv: [
                self.uv[0] + (other.uv[0] - self.uv[0]) * t,
                self.uv[1] + (other.uv[1] - self.uv[1]) * t,
            ],
        }
    }

    fn flipped(self) -> Self {
        Self {
            normal: -self.normal,
            ..self
        }
    }
}

/// A convex face and the plane it lies in.
#[derive(Clone, Debug)]
struct Face {
    corners: Vec<Corner>,
    normal: Vec3,
    offset: f32,
}

impl Face {
    /// A face from its corners, or `None` when they are collinear and so
    /// enclose nothing to cut against.
    fn new(corners: Vec<Corner>) -> Option<Self> {
        if corners.len() < 3 {
            return None;
        }
        let a = corners[0].position;
        let normal = (corners[1].position - a).cross(corners[2].position - a);
        if normal.length_squared() <= f32::MIN_POSITIVE {
            return None;
        }
        let normal = normal.normalize_or_zero();
        Some(Self {
            offset: normal.dot(a),
            corners,
            normal,
        })
    }

    fn flip(&mut self) {
        self.corners.reverse();
        for corner in &mut self.corners {
            *corner = corner.flipped();
        }
        self.normal = -self.normal;
        self.offset = -self.offset;
    }
}

/// Where a face sits relative to a plane. The values are bits so a face that
/// straddles one comes out as `FRONT | BACK`.
const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;

/// What cutting a face by a plane leaves, in the four places a piece can
/// land. A caller merges them the way its own operation needs.
#[derive(Default)]
struct Pieces {
    coplanar_front: Vec<Face>,
    coplanar_back: Vec<Face>,
    front: Vec<Face>,
    back: Vec<Face>,
}

/// Cut `face` by the plane, adding each piece to the list it belongs in.
///
/// A face lying in the plane goes to the side the plane faces, which is what
/// keeps a shared wall from being counted twice.
fn split(normal: Vec3, offset: f32, face: &Face, out: &mut Pieces) {
    let sides: Vec<u8> = face
        .corners
        .iter()
        .map(|corner| {
            let distance = normal.dot(corner.position) - offset;
            if distance < -ON_PLANE {
                BACK
            } else if distance > ON_PLANE {
                FRONT
            } else {
                COPLANAR
            }
        })
        .collect();
    let combined = sides.iter().fold(0, |all, side| all | side);
    match combined {
        COPLANAR => {
            if normal.dot(face.normal) > 0.0 {
                out.coplanar_front.push(face.clone());
            } else {
                out.coplanar_back.push(face.clone());
            }
        }
        FRONT => out.front.push(face.clone()),
        BACK => out.back.push(face.clone()),
        _ => {
            let (mut ahead, mut behind) = (Vec::new(), Vec::new());
            let count = face.corners.len();
            for i in 0..count {
                let j = (i + 1) % count;
                let (here, next) = (face.corners[i], face.corners[j]);
                if sides[i] != BACK {
                    ahead.push(here);
                }
                if sides[i] != FRONT {
                    behind.push(here);
                }
                if sides[i] | sides[j] == FRONT | BACK {
                    let from = normal.dot(here.position) - offset;
                    let to = normal.dot(next.position) - offset;
                    let t = from / (from - to);
                    let cut = here.mix(next, t);
                    ahead.push(cut);
                    behind.push(cut);
                }
            }
            if let Some(piece) = Face::new(ahead) {
                out.front.push(piece);
            }
            if let Some(piece) = Face::new(behind) {
                out.back.push(piece);
            }
        }
    }
}

/// One node of the tree: a dividing plane, the faces lying in it, and the
/// two halves it separates.
#[derive(Default)]
struct Node {
    divider: Option<(Vec3, f32)>,
    faces: Vec<Face>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
}

impl Node {
    fn build(faces: &[Face], depth: u32) -> Self {
        let mut node = Self::default();
        node.add(faces, depth);
        node
    }

    /// Put more faces into the tree, dividing on the first one's plane when
    /// this node has none yet.
    fn add(&mut self, faces: &[Face], depth: u32) {
        if faces.is_empty() {
            return;
        }
        if self.divider.is_none() {
            self.divider = Some((faces[0].normal, faces[0].offset));
        }
        let Some((normal, offset)) = self.divider else {
            return;
        };
        let mut pieces = Pieces::default();
        for face in faces {
            split(normal, offset, face, &mut pieces);
        }
        // Anything lying in this node's plane belongs to the node itself,
        // whichever way round it faces.
        self.faces.append(&mut pieces.coplanar_front);
        self.faces.append(&mut pieces.coplanar_back);
        let (ahead, behind) = (pieces.front, pieces.back);
        // Past the cap the tree stops dividing and keeps what is left where
        // it stands: a wrong face beats a blown stack.
        if depth >= MAX_DEPTH {
            self.faces.extend(ahead);
            self.faces.extend(behind);
            return;
        }
        if !ahead.is_empty() {
            self.front
                .get_or_insert_with(Box::default)
                .add(&ahead, depth + 1);
        }
        if !behind.is_empty() {
            self.back
                .get_or_insert_with(Box::default)
                .add(&behind, depth + 1);
        }
    }

    /// The faces of `faces` that lie outside this solid.
    fn clip(&self, faces: Vec<Face>) -> Vec<Face> {
        let Some((normal, offset)) = self.divider else {
            return faces;
        };
        let mut pieces = Pieces::default();
        for face in &faces {
            split(normal, offset, face, &mut pieces);
        }
        // A coplanar face goes with the half the plane faces, so a wall
        // shared with the clipping solid is kept exactly once.
        let mut ahead = pieces.front;
        ahead.append(&mut pieces.coplanar_front);
        let mut behind = pieces.back;
        behind.append(&mut pieces.coplanar_back);
        let mut kept = match &self.front {
            Some(front) => front.clip(ahead),
            None => ahead,
        };
        // With nothing behind the plane, everything behind it is inside.
        if let Some(back) = &self.back {
            kept.extend(back.clip(behind));
        }
        kept
    }

    /// Drop everything of this solid that lies inside `other`.
    fn clip_to(&mut self, other: &Self) {
        self.faces = other.clip(std::mem::take(&mut self.faces));
        if let Some(front) = &mut self.front {
            front.clip_to(other);
        }
        if let Some(back) = &mut self.back {
            back.clip_to(other);
        }
    }

    /// Turn the solid inside out: every face reversed and the halves swapped.
    fn invert(&mut self) {
        for face in &mut self.faces {
            face.flip();
        }
        if let Some((normal, offset)) = self.divider {
            self.divider = Some((-normal, -offset));
        }
        if let Some(front) = &mut self.front {
            front.invert();
        }
        if let Some(back) = &mut self.back {
            back.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    fn faces(&self) -> Vec<Face> {
        let mut out = self.faces.clone();
        if let Some(front) = &self.front {
            out.extend(front.faces());
        }
        if let Some(back) = &self.back {
            out.extend(back.faces());
        }
        out
    }
}

/// Two meshes combined. The result carries normals and texture coordinates
/// interpolated across every cut, so a face that was split still shades and
/// textures as the face it came from.
#[must_use]
pub fn combine(a: &MeshData, b: &MeshData, op: Op) -> MeshData {
    let (mut left, mut right) = (Node::build(&faces_of(a), 0), Node::build(&faces_of(b), 0));
    match op {
        Op::Union => {
            left.clip_to(&right);
            right.clip_to(&left);
            right.invert();
            right.clip_to(&left);
            right.invert();
            left.add(&right.faces(), 0);
        }
        Op::Difference => {
            left.invert();
            left.clip_to(&right);
            right.clip_to(&left);
            right.invert();
            right.clip_to(&left);
            right.invert();
            left.add(&right.faces(), 0);
            left.invert();
        }
        Op::Intersection => {
            left.invert();
            right.clip_to(&left);
            right.invert();
            left.clip_to(&right);
            right.clip_to(&left);
            left.add(&right.faces(), 0);
            left.invert();
        }
    }
    mesh_of(&left.faces())
}

/// Every triangle of a mesh as a face, with the normals it carries or the
/// ones its faces imply.
fn faces_of(mesh: &MeshData) -> Vec<Face> {
    let normal_at = |i: u32| {
        mesh.normals
            .as_ref()
            .and_then(|ns| ns.get(i as usize))
            .map_or(Vec3::ZERO, |n| Vec3::from_array(*n))
    };
    let uv_at = |i: u32| {
        mesh.uvs
            .as_ref()
            .and_then(|us| us.get(i as usize))
            .copied()
            .unwrap_or([0.0, 0.0])
    };
    mesh.indices
        .iter()
        .filter_map(|&[a, b, c]| {
            let corner = |i: u32| Corner {
                position: Vec3::from_array(mesh.positions[i as usize]),
                normal: normal_at(i),
                uv: uv_at(i),
            };
            let mut face = Face::new(vec![corner(a), corner(b), corner(c)])?;
            // A mesh with no normals of its own takes the face's, so a cut
            // through it still shades flat rather than black.
            for slot in &mut face.corners {
                if slot.normal == Vec3::ZERO {
                    slot.normal = face.normal;
                }
            }
            Some(face)
        })
        .collect()
}

/// The faces fanned back into triangles. Every face out of the tree is
/// convex, because a split of a convex face is two convex faces.
fn mesh_of(faces: &[Face]) -> MeshData {
    let mut build = crate::primitive::Build::default();
    for face in faces {
        let first = build.vertex_count();
        for corner in &face.corners {
            build.vertex(
                corner.position,
                corner.normal,
                glamx::Vec2::from_array(corner.uv),
            );
        }
        for i in 1..face.corners.len() as u32 - 1 {
            build.triangle(first, first + i, first + i + 1);
        }
    }
    build.finish()
}
