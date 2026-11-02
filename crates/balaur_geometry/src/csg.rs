//! Constructive solid geometry: two meshes joined, cut or intersected.
//!
//! A BSP tree over the faces, the algorithm every CSG library is a variant
//! of: each solid's faces are clipped against the other's tree until only
//! the ones the operation keeps are left. Written out because parry offers
//! `intersect_meshes` and nothing else -- no union, no difference -- and
//! because a BSP is dot products, lerps and comparisons with no
//! transcendental anywhere, so the result is the same on every platform.
//!
//! What the tree leaves is sealed before it becomes triangles: corners a
//! rounding apart are merged, and a corner one face has in the middle of
//! another's edge is added to that edge, so every edge of the result is
//! shared by exactly two triangles.

use crate::mesh::MeshData;
use crate::primitive::Build;
use glamx::{Vec2, Vec3};
use rustc_hash::FxHashMap;

/// Which side of a plane a point counts as being on. Below this a vertex is
/// treated as lying in the plane, which is what stops a face that is almost
/// coplanar with a cut from being split into slivers. Sealing merges corners
/// this close, scaled up for a mesh whose coordinates pass one.
const ON_PLANE: f32 = 1e-5;

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

    /// A piece of `parent` cut off by a plane. It keeps the parent's plane:
    /// three of its corners may be collinear, and a plane from those would
    /// be noise or nothing.
    fn piece(corners: Vec<Corner>, parent: &Self) -> Option<Self> {
        (corners.len() >= 3).then_some(Self {
            corners,
            normal: parent.normal,
            offset: parent.offset,
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
fn split(normal: Vec3, offset: f32, face: Face, out: &mut Pieces) {
    let side = |corner: &Corner| {
        let distance = normal.dot(corner.position) - offset;
        if distance < -ON_PLANE {
            BACK
        } else if distance > ON_PLANE {
            FRONT
        } else {
            COPLANAR
        }
    };
    match face
        .corners
        .iter()
        .fold(COPLANAR, |all, corner| all | side(corner))
    {
        COPLANAR => {
            if normal.dot(face.normal) > 0.0 {
                out.coplanar_front.push(face);
            } else {
                out.coplanar_back.push(face);
            }
        }
        FRONT => out.front.push(face),
        BACK => out.back.push(face),
        _ => {
            let sides: Vec<u8> = face.corners.iter().map(side).collect();
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
            out.front.extend(Face::piece(ahead, &face));
            out.back.extend(Face::piece(behind, &face));
        }
    }
}

/// One node of the tree: a dividing plane, the faces lying in it, and the
/// two halves it separates, as indices into [`Tree::nodes`].
#[derive(Default)]
struct Node {
    divider: Option<(Vec3, f32)>,
    faces: Vec<Face>,
    front: Option<usize>,
    back: Option<usize>,
}

/// A solid as a BSP tree. A convex solid divides into a chain as deep as it
/// has faces, so every walk keeps its own stack rather than recursing, and
/// none stops early: a node past a cut-off would not divide what it holds.
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn build(faces: Vec<Face>) -> Self {
        let mut tree = Self {
            nodes: vec![Node::default()],
        };
        tree.add(faces);
        tree
    }

    /// Put more faces into the tree, each node dividing on the plane of the
    /// first face to reach it.
    fn add(&mut self, faces: Vec<Face>) {
        let mut work = vec![(0, faces)];
        while let Some((at, faces)) = work.pop() {
            let Some(first) = faces.first() else {
                continue;
            };
            let (normal, offset) = *self.nodes[at]
                .divider
                .get_or_insert((first.normal, first.offset));
            let mut pieces = Pieces::default();
            for face in faces {
                split(normal, offset, face, &mut pieces);
            }
            // Anything lying in this node's plane belongs to the node itself,
            // whichever way round it faces.
            let node = &mut self.nodes[at];
            node.faces.append(&mut pieces.coplanar_front);
            node.faces.append(&mut pieces.coplanar_back);
            if !pieces.front.is_empty() {
                work.push((self.child(at, true), pieces.front));
            }
            if !pieces.back.is_empty() {
                work.push((self.child(at, false), pieces.back));
            }
        }
    }

    /// The node on one side of `at`, made empty if there is none yet.
    fn child(&mut self, at: usize, front: bool) -> usize {
        let fresh = self.nodes.len();
        let node = &mut self.nodes[at];
        let slot = if front {
            &mut node.front
        } else {
            &mut node.back
        };
        let index = *slot.get_or_insert(fresh);
        if index == fresh {
            self.nodes.push(Node::default());
        }
        index
    }

    /// The faces of `faces` that lie outside this solid.
    fn clip(&self, faces: Vec<Face>) -> Vec<Face> {
        let mut kept = Vec::new();
        let mut work = vec![(0, faces)];
        while let Some((at, faces)) = work.pop() {
            let node = &self.nodes[at];
            let Some((normal, offset)) = node.divider else {
                kept.extend(faces);
                continue;
            };
            let mut pieces = Pieces::default();
            for face in faces {
                split(normal, offset, face, &mut pieces);
            }
            // A coplanar face goes with the half the plane faces, so a wall
            // shared with the clipping solid is kept exactly once.
            let mut ahead = pieces.front;
            ahead.append(&mut pieces.coplanar_front);
            let mut behind = pieces.back;
            behind.append(&mut pieces.coplanar_back);
            // With nothing behind the plane, everything behind it is inside.
            if let Some(back) = node.back
                && !behind.is_empty()
            {
                work.push((back, behind));
            }
            match node.front {
                Some(front) => work.push((front, ahead)),
                None => kept.extend(ahead),
            }
        }
        kept
    }

    /// Drop everything of this solid that lies inside `other`.
    fn clip_to(&mut self, other: &Self) {
        for node in &mut self.nodes {
            if !node.faces.is_empty() {
                node.faces = other.clip(std::mem::take(&mut node.faces));
            }
        }
    }

    /// Turn the solid inside out: every face reversed and the halves swapped.
    fn invert(&mut self) {
        for node in &mut self.nodes {
            for face in &mut node.faces {
                face.flip();
            }
            node.divider = node.divider.map(|(normal, offset)| (-normal, -offset));
            std::mem::swap(&mut node.front, &mut node.back);
        }
    }

    fn faces(&self) -> Vec<Face> {
        self.nodes
            .iter()
            .flat_map(|node| node.faces.iter().cloned())
            .collect()
    }
}

/// Two meshes combined. The result carries normals and texture coordinates
/// interpolated across every cut, so a face that was split still shades and
/// textures as the face it came from.
#[must_use]
pub fn combine(a: &MeshData, b: &MeshData, op: Op) -> MeshData {
    let (mut left, mut right) = (Tree::build(faces_of(a)), Tree::build(faces_of(b)));
    // The textbook ends by building `right` into `left` and reading the tree
    // back; that only cuts the kept faces finer, so the two lists are joined.
    let inverted = match op {
        Op::Union => {
            left.clip_to(&right);
            right.clip_to(&left);
            right.invert();
            right.clip_to(&left);
            right.invert();
            false
        }
        Op::Difference => {
            left.invert();
            left.clip_to(&right);
            right.clip_to(&left);
            right.invert();
            right.clip_to(&left);
            right.invert();
            true
        }
        Op::Intersection => {
            left.invert();
            right.clip_to(&left);
            right.invert();
            left.clip_to(&right);
            right.clip_to(&left);
            true
        }
    };
    let mut faces = left.faces();
    faces.extend(right.faces());
    if inverted {
        faces.iter_mut().for_each(Face::flip);
    }
    mesh_of(&faces)
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

/// The faces sealed and cut into triangles. A face keeps its own corners, so
/// its normals and texture coordinates stay its own; only positions are
/// shared.
fn mesh_of(faces: &[Face]) -> MeshData {
    let mut points = Points::new(faces);
    let rings: Vec<Vec<(u32, Corner)>> = faces.iter().map(|face| points.ring(face)).collect();
    let mut junctions = FxHashMap::default();
    let mut build = Build::default();
    for (face, ring) in faces.iter().zip(&rings) {
        if ring.len() >= 3 {
            let ring = points.with_junctions(ring, &mut junctions);
            triangulate(&ring, face.normal, points.tolerance, &mut build);
        }
    }
    build.finish()
}

/// Every distinct corner position of a result, merged where two lie within
/// `tolerance` of each other, and a grid to find them by.
struct Points {
    at: Vec<Vec3>,
    cells: FxHashMap<[i32; 3], Vec<u32>>,
    cell: f32,
    tolerance: f32,
}

/// The points strictly inside an edge, keyed by its ends lowest first, with
/// how far from the lower end each lies.
type Junctions = FxHashMap<(u32, u32), Vec<(f32, u32)>>;

impl Points {
    fn new(faces: &[Face]) -> Self {
        let (mut reach, mut length, mut edges) = (1.0_f32, 0.0_f32, 0_u32);
        for face in faces {
            for (i, corner) in face.corners.iter().enumerate() {
                let next = face.corners[(i + 1) % face.corners.len()].position;
                reach = reach.max(corner.position.abs().max_element());
                length += corner.position.distance(next);
                edges += 1;
            }
        }
        let tolerance = ON_PLANE * reach;
        // A cell about an edge long keeps a lookup to a handful of points.
        let cell = (length / edges.max(1) as f32).max(tolerance);
        Self {
            at: Vec::new(),
            cells: FxHashMap::default(),
            cell,
            tolerance,
        }
    }

    fn key(&self, p: Vec3) -> [i32; 3] {
        let p = p / self.cell;
        [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32]
    }

    /// Every cell the box from `low` to `high` widened by a tolerance touches.
    fn cells_over(&self, low: Vec3, high: Vec3, keys: &mut Vec<[i32; 3]>) {
        let pad = Vec3::splat(self.tolerance);
        let (low, high) = (self.key(low - pad), self.key(high + pad));
        for x in low[0]..=high[0] {
            for y in low[1]..=high[1] {
                keys.extend((low[2]..=high[2]).map(|z| [x, y, z]));
            }
        }
    }

    fn points_in(&self, keys: &[[i32; 3]]) -> impl Iterator<Item = u32> {
        keys.iter()
            .filter_map(|key| self.cells.get(key))
            .flatten()
            .copied()
    }

    /// The point `p` merges into, added if it is the first there.
    fn id(&mut self, p: Vec3) -> u32 {
        let mut keys = Vec::new();
        self.cells_over(p, p, &mut keys);
        if let Some(found) = self
            .points_in(&keys)
            .find(|&i| self.at[i as usize].distance(p) <= self.tolerance)
        {
            return found;
        }
        let id = self.at.len() as u32;
        self.at.push(p);
        self.cells.entry(self.key(p)).or_default().push(id);
        id
    }

    /// A face's corners as merged points, with the runs merging left behind
    /// collapsed to one.
    fn ring(&mut self, face: &Face) -> Vec<(u32, Corner)> {
        let mut ring: Vec<(u32, Corner)> = Vec::with_capacity(face.corners.len());
        for corner in &face.corners {
            let id = self.id(corner.position);
            if ring.last().is_none_or(|(last, _)| *last != id) {
                ring.push((id, *corner));
            }
        }
        while ring.len() > 1 && ring.first().map(|c| c.0) == ring.last().map(|c| c.0) {
            ring.pop();
        }
        ring
    }

    /// The points strictly inside the segment, with how far along it each
    /// lies. The segment is walked a cell at a time, so a long diagonal one
    /// looks in the cells beside it and not the whole box it spans.
    fn on_segment(&self, from: Vec3, to: Vec3) -> Vec<(f32, u32)> {
        let span = to - from;
        let length_squared = span.length_squared();
        if length_squared <= 0.0 {
            return Vec::new();
        }
        let pieces = (span.length() / self.cell).ceil().max(1.0) as u32;
        let along = |s: u32| from + span * (s as f32 / pieces as f32);
        let mut keys = Vec::new();
        for s in 0..pieces {
            let (a, b) = (along(s), along(s + 1));
            self.cells_over(a.min(b), a.max(b), &mut keys);
        }
        keys.sort_unstable();
        keys.dedup();
        self.points_in(&keys)
            .filter_map(|i| {
                let p = self.at[i as usize];
                let t = (p - from).dot(span) / length_squared;
                let inside = t > 0.0 && t < 1.0;
                (inside && (from + span * t).distance(p) <= self.tolerance).then_some((t, i))
            })
            .collect()
    }

    /// The ring at merged positions, with every point another face has in
    /// the middle of one of its edges added there, in order along the edge.
    fn with_junctions(&self, ring: &[(u32, Corner)], junctions: &mut Junctions) -> Vec<Corner> {
        let mut out = Vec::with_capacity(ring.len());
        for (i, &(id, corner)) in ring.iter().enumerate() {
            let (next_id, next) = ring[(i + 1) % ring.len()];
            out.push(Corner {
                position: self.at[id as usize],
                ..corner
            });
            let (low, high) = (id.min(next_id), id.max(next_id));
            let found = junctions
                .entry((low, high))
                .or_insert_with(|| self.on_segment(self.at[low as usize], self.at[high as usize]));
            let mut between: Vec<(f32, u32)> = found
                .iter()
                .filter(|(_, point)| ring.iter().all(|(own, _)| own != point))
                .map(|&(t, point)| (if id == low { t } else { 1.0 - t }, point))
                .collect();
            between.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            for (t, point) in between {
                out.push(Corner {
                    position: self.at[point as usize],
                    ..corner.mix(next, t)
                });
            }
        }
        out
    }
}

/// A convex ring cut into triangles, none of them flat. Only a real corner
/// is clipped, and one beside a point on a straight side goes first: the
/// other order can leave a side's points in a line with nothing to close.
fn triangulate(ring: &[Corner], normal: Vec3, tolerance: f32, build: &mut Build) {
    let first = build.vertex_count();
    for corner in ring {
        build.vertex(corner.position, corner.normal, Vec2::from_array(corner.uv));
    }
    let at = |i: usize| ring[i].position;
    let mut left: Vec<usize> = (0..ring.len()).collect();
    while left.len() >= 3 {
        let count = left.len();
        let around = |k: usize| {
            (
                left[(k + count - 1) % count],
                left[k],
                left[(k + 1) % count],
            )
        };
        let corners: Vec<bool> = (0..count)
            .map(|k| {
                let (prev, here, next) = around(k);
                let base = at(next) - at(prev);
                (at(here) - at(prev)).cross(base).dot(normal) > tolerance * base.length()
            })
            .collect();
        let Some(k) = (0..count)
            .filter(|&k| corners[k])
            .min_by_key(|&k| corners[(k + count - 1) % count] && corners[(k + 1) % count])
        else {
            break;
        };
        let (prev, here, next) = around(k);
        build.triangle(
            first + prev as u32,
            first + here as u32,
            first + next as u32,
        );
        left.remove(k);
    }
}
