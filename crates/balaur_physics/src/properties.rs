//! The words and values more than one physics component's properties hold:
//! each a type the component structs name, so a struct is its schema.
//!
//! A word set here is the options list a schema offers and the value a
//! scene file spells; the vocabulary's `words` are the same words, for the
//! script calls and readers that take them as text.

use balaur_core::Engine;
use balaur_core::components::derived::{Enum, Property, Record, Result, Variant, toml};
use balaur_core::hecs::Entity;
use balaur_core::layers::Layer;

/// The node a `node` property names, resolved from the node that holds it as
/// `balaur_core::components::as_node` does: `None` for the empty default and
/// for a path that names nothing yet.
pub(crate) fn node_at(eng: &Engine, from: Entity, path: &str) -> Option<Entity> {
    if path.trim().is_empty() {
        return None;
    }
    balaur_core::scene::find_node(&eng.world(), from, path)
}

/// How physics drives a body's node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum BodyKind {
    Dynamic,
    Static,
    Kinematic,
    KinematicVelocity,
}

/// A world axis in 2D.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Axis2d {
    X,
    Y,
}

/// A world axis in 3D.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Axis3d {
    X,
    Y,
    Z,
}

/// The one way a 2D body turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Turn {
    Rotation,
}

/// Whether a character's lengths are world units or a fraction of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum LengthMode {
    Absolute,
    Relative,
}

/// What a character's or a vehicle's sweep passes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum SweepIgnore {
    Static,
    Kinematic,
    Dynamic,
    Sensors,
    Solids,
}

/// Whether a pair needs both colliders' layers to accept it, or either's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum LayerTest {
    Both,
    Either,
}

/// The pairs of body kinds a collider is tested against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum ContactPair {
    DynamicDynamic,
    DynamicKinematic,
    DynamicStatic,
    KinematicKinematic,
    KinematicStatic,
    StaticStatic,
}

/// The shapes a 2D mesh can be fitted to: rapier2d's converter has no
/// decomposition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Fit2d {
    ConvexHull,
    Aabb,
    Obb,
}

/// The shapes a 3D mesh can be fitted to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Fit3d {
    ConvexHull,
    Aabb,
    Obb,
    ConvexDecomposition,
}

/// Which edges a polyline takes from its mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Edges {
    Chain,
    Mesh,
}

/// Which edges a 2D polyline takes from its mesh, its outline among them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Edges2d {
    Chain,
    Outline,
    Mesh,
}

/// Whether voxelizing a mesh fills its inside or only its shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Fill {
    Solid,
    Surface,
}

/// How a concave 2D polygon is cut into convex pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Decomposition2d {
    Exact,
    Vhacd,
    Voxels,
}

/// How a 3D convex decomposition is cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Decomposition3d {
    Vhacd,
    Voxels,
}

/// Rapier's two follow controllers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum FollowKind {
    Pd,
    Pid,
}

/// The 2D joints. `groove` is Godot's, and 2D-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum JointKind2d {
    Fixed,
    Hinge,
    Slider,
    Rope,
    Spring,
    Groove,
    Generic,
}

/// The 3D joints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum JointKind3d {
    Fixed,
    Hinge,
    Slider,
    BallSocket,
    Rope,
    Spring,
    Generic,
}

/// A 2D joint's degrees of freedom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum JointAxis2d {
    X,
    Y,
    Rotation,
}

/// A 3D joint's degrees of freedom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum JointAxis3d {
    X,
    Y,
    Z,
    RotationX,
    RotationY,
    RotationZ,
}

/// What a joint's motor drives towards, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Motor {
    Off,
    Velocity,
    Position,
}

/// Whether a motor's strength ignores mass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum MotorStrength {
    Auto,
    Acceleration,
    Force,
}

/// How a 2D soft body's particles are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum SoftKind2d {
    Grid,
    Circle,
    Polygon,
    Outline,
    Rope,
    Volumetric,
    TriangleMesh,
    Polyline,
    Custom,
}

/// How a 3D soft body's particles are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum SoftKind3d {
    Box,
    Sphere,
    Cloth,
    ClothTube,
    Rope,
    Volumetric,
    TriangleMesh,
    Custom,
}

/// Which way a soft body's surface faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Orientation {
    Auto,
    Solid,
    Shell,
}

/// How a soft body's collision mesh follows the body.
#[derive(Clone, Debug, PartialEq, Variant)]
pub enum Binding {
    /// Each vertex on the nearest particle within its distance
    #[option(default)]
    Nearest(NearestBinding),
    /// Vertex i on particle i
    Particles,
    /// Each vertex riding the cell that holds it
    Cells,
}

/// How near a particle a vertex binds to.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct NearestBinding {
    /// How far a vertex may sit from the particle it binds to
    #[property(default = 0.01, min = 0.0)]
    pub distance: f32,
}

/// Which of a soft body's edges only pull.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum TensionOnly {
    None,
    All,
    Listed,
}

/// How a soft body is solved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum SoftSolver {
    Constraints,
    Fem,
}

/// A chassis's own axes, either way along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum ChassisAxis {
    X,
    Y,
    Z,
    #[option(name = "-x")]
    NegativeX,
    #[option(name = "-y")]
    NegativeY,
    #[option(name = "-z")]
    NegativeZ,
}

/// A path to a node, held in a list: what a `node` property is on its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodePath(pub String);

impl Property for NodePath {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        spec.entry("type")
            .or_insert_with(|| toml::Value::String("node".into()));
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::String(self.0.clone())
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        String::from_value(value).map(Self)
    }
}

/// Particle indices, read as leniently as scenes wrote them: a number saved
/// as text still counts, and an entry that is no index is skipped, so a
/// half-typed row does not throw the others away.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Indices(pub Vec<u32>);

impl Property for Indices {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        spec.entry("type")
            .or_insert_with(|| toml::Value::String("list".into()));
        let mut of = toml::map::Map::new();
        of.insert("type".into(), toml::Value::String("int".into()));
        spec.entry("of").or_insert(toml::Value::Table(of));
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::Array(
            self.0
                .iter()
                .map(|index| toml::Value::Integer(i64::from(*index)))
                .collect(),
        )
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        let items = value
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("is {}, where a list was expected", value.type_str()))?;
        Ok(Self(
            items
                .iter()
                .filter_map(|item| match item {
                    toml::Value::Integer(number) => u32::try_from(*number).ok(),
                    toml::Value::String(text) => text.trim().parse::<u32>().ok(),
                    _ => None,
                })
                .collect(),
        ))
    }
}

/// Two particles of a soft body, as a seam or a tension-only edge names them.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct ParticlePair {
    #[property(name = "a")]
    pub first: i64,
    #[property(name = "b", default = 1)]
    pub second: i64,
}

/// Part of a soft body with a material of its own.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct SoftRegion {
    pub particles: Vec<i64>,
    #[property(default = 1.0)]
    pub stiffness_scale: f32,
    #[property(default = 1.0)]
    pub tear_resistance: f32,
    pub shape_matching: bool,
    pub pinned: bool,
    pub hz: f32,
    #[property(default = 1.0)]
    pub damping: f32,
}

/// An edge of a soft body with a spring of its own.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct EdgeSpring {
    #[property(name = "a")]
    pub first: i64,
    #[property(name = "b", default = 1)]
    pub second: i64,
    #[property(default = 30.0)]
    pub hz: f32,
    #[property(default = 1.0)]
    pub damping: f32,
}

/// An edge of a soft body that tears sooner or later than the rest.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct EdgeTearResistance {
    #[property(name = "a")]
    pub first: i64,
    #[property(name = "b", default = 1)]
    pub second: i64,
    #[property(default = 1.0)]
    pub resistance: f32,
}

/// The rows every collider carries beside its shape and its mass, the same
/// in both dimensions and on a tile map: what it is, what it meets, what it
/// reports. Read from the typed component, so a builder never reads a table.
pub(crate) trait ColliderRows {
    fn enabled(&self) -> bool;
    fn sensor(&self) -> bool;
    /// The layers it is on, the layers it meets, and how the two are tested.
    fn collision(&self) -> (&[Layer], &[Layer], LayerTest);
    /// The same three for whether a contact pushes.
    fn solver(&self) -> (&[Layer], &[Layer], LayerTest);
    fn contact_pairs(&self) -> &[ContactPair];
    fn contact_force_threshold(&self) -> f32;
    fn collision_margin(&self) -> f32;
    fn density(&self) -> f32;
    fn mass(&self) -> f32;
    /// Whether it asks for the contact hook: a one-way surface, or one that
    /// carries what touches it along.
    fn hooked(&self) -> bool;
}

/// [`ColliderRows`] for a component whose rows have the names a collider's
/// have.
macro_rules! collider_rows {
    ($($component:ty),*) => {$(
        impl crate::properties::ColliderRows for $component {
            fn enabled(&self) -> bool {
                self.enabled
            }
            fn sensor(&self) -> bool {
                self.sensor
            }
            fn collision(
                &self,
            ) -> (&[balaur_core::layers::Layer], &[balaur_core::layers::Layer], crate::properties::LayerTest) {
                (&self.collision_layer, &self.collision_mask, self.collision_test)
            }
            fn solver(
                &self,
            ) -> (&[balaur_core::layers::Layer], &[balaur_core::layers::Layer], crate::properties::LayerTest) {
                (&self.solver_layer, &self.solver_mask, self.solver_test)
            }
            fn contact_pairs(&self) -> &[crate::properties::ContactPair] {
                &self.contact_pairs
            }
            fn contact_force_threshold(&self) -> f32 {
                self.contact_force_threshold
            }
            fn collision_margin(&self) -> f32 {
                self.collision_margin
            }
            fn density(&self) -> f32 {
                self.density
            }
            fn mass(&self) -> f32 {
                self.mass
            }
            fn hooked(&self) -> bool {
                self.one_way || self.surface_velocity.iter().any(|axis| *axis != 0.0)
            }
        }
    )*};
}
pub(crate) use collider_rows;
