//! `collider2d`: shapes, their material, and the overlap and contact
//! queries that read them back.

use crate::rapier2d::math::Vector;
use anyhow::{anyhow, Result};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine};

use crate::rapier2d::prelude::{
    ActiveCollisionTypes, ActiveEvents, ActiveHooks, CoefficientCombineRule, Collider,
    ColliderBuilder as ColliderBuilder2, Group, InteractionGroups, InteractionTestMode,
    RigidBodyHandle,
};
use crate::scalar::{self, Pose2, Real, Rotation2};

use crate::dim2::{node_pose_2d, PhysicsState2d};
use crate::vocabulary as v;

/// The nearest 2D body at or above `entity`, and the node that owns it: the
/// 2D half of `crate::collider::nearest_body`, and the reason a child node's
/// collider belongs to its parent's body.
fn nearest_body(eng: &Engine, entity: Entity) -> Option<(Entity, RigidBodyHandle)> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let world = eng.world();
    let mut current = entity;
    loop {
        if let Some(handle) = state.bodies.get(&current) {
            return Some((current, *handle));
        }
        current = world.get::<&balaur_core::scene::Parent>(current).ok()?.0;
    }
}

fn pose_relative_to(eng: &Engine, entity: Entity, body_node: Entity) -> Result<Pose2> {
    let here = node_pose_2d(eng, entity)?;
    if entity == body_node {
        return Ok(Pose2::IDENTITY);
    }
    let there = node_pose_2d(eng, body_node)?;
    let inverse = there.rotation.inverse();
    Ok(Pose2::from_parts(
        inverse * (here.translation - there.translation),
        inverse * here.rotation,
    ))
}

pub(crate) fn add_collider_at(
    eng: &Engine,
    entity: Entity,
    builder: ColliderBuilder2,
    offset: Pose2,
) -> Result<()> {
    let handle = match nearest_body(eng, entity) {
        Some((body_node, body)) => {
            let local = pose_relative_to(eng, entity, body_node)?;
            let state = eng.resource::<PhysicsState2d>();
            let mut state = state.borrow_mut();
            state
                .world
                .insert_collider(builder.position(local * offset), Some(body))
        }
        None => {
            let pose = node_pose_2d(eng, entity)?;
            let state = eng.resource::<PhysicsState2d>();
            let mut state = state.borrow_mut();
            state
                .world
                .insert_collider(builder.position(pose * offset), None)
        }
    };
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    state.world.colliders[handle].user_data = u128::from(entity.to_bits().get());
    state.colliders.entry(entity).or_default().push(handle);
    state.queries_ready = false;
    Ok(())
}

pub(crate) fn add_collider(eng: &Engine, entity: Entity, builder: ColliderBuilder2) -> Result<()> {
    add_collider_at(eng, entity, builder, Pose2::IDENTITY)
}

pub(crate) fn remove_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    if let Some(handles) = state.colliders.swap_remove(&entity) {
        for handle in handles {
            state.world.remove_collider(handle);
        }
        state.queries_ready = false;
    }
}

/// The collider described by `params`, in the `collider2d` schema's own
/// vocabulary — so a script table and a scene-file entry build the same thing.
pub(crate) fn collider_builder(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder2> {
    let kind = v::text(params, "kind", "rect");
    let radius = scalar::real(v::f(params, "radius", 0.5)).max(0.01);
    // `height` is the straight part, caps excluded, as it is in 3D.
    let half_height = scalar::real(v::f(params, "height", 1.0).max(0.01)) / 2.0;
    let he = |i: usize| scalar::real(v::axis(params, "half_extents", i, 0.5)).max(0.01);
    let point = |key: &str, fallback: [f32; 2]| scalar::v2a(v::vec2(params, key, fallback));
    let border = scalar::real(v::f(params, "border", 0.0)).max(0.0);
    let rounded = border > 0.0;
    let builder = match kind {
        "circle" => ColliderBuilder2::ball(radius),
        "rect" if rounded => ColliderBuilder2::round_cuboid(he(0), he(1), border),
        "rect" => ColliderBuilder2::cuboid(he(0), he(1)),
        "capsule" => ColliderBuilder2::capsule_y(half_height, radius),
        "triangle" if rounded => ColliderBuilder2::round_triangle(
            point("a", [0.0, 0.0]),
            point("b", [1.0, 0.0]),
            point("c", [0.0, 1.0]),
            border,
        ),
        "triangle" => ColliderBuilder2::triangle(
            point("a", [0.0, 0.0]),
            point("b", [1.0, 0.0]),
            point("c", [0.0, 1.0]),
        ),
        "segment" => ColliderBuilder2::segment(point("a", [0.0, 0.0]), point("b", [1.0, 0.0])),
        "halfspace" => {
            let n = point("normal", [0.0, 1.0]);
            if n.length_squared() < 1.0e-12 {
                return Err(anyhow!("a halfspace collider needs a non-zero `normal`"));
            }
            ColliderBuilder2::new(crate::rapier2d::prelude::SharedShape::halfspace(
                n.normalize(),
            ))
        }
        "trimesh" | "convex_hull" | "polyline" => mesh_collider(eng, params, kind)?,
        "heightfield" => heightfield_collider(eng, params)?,
        other => return Err(anyhow!("unknown collider2d kind '{other}'")),
    };
    Ok(with_material(builder, params))
}

/// A 2D shape from a `mesh` asset, reading the x and y of its points: the
/// same asset a `polygon` draws, so the outline a player sees and the one they
/// collide with are one authored thing.
fn mesh_collider(eng: &Engine, params: &toml::Value, kind: &str) -> Result<ColliderBuilder2> {
    let reference = params
        .get("mesh")
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a {kind} collider2d needs a `mesh` asset"))?;
    let definition =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, reference)?;
    let mesh = balaur_core::mesh::load_from(eng, &definition)?;
    let points: Vec<Vector> = mesh
        .positions
        .iter()
        .map(|p| scalar::v2(p[0], p[1]))
        .collect();
    match kind {
        "trimesh" => ColliderBuilder2::trimesh(points, mesh.indices.clone())
            .map_err(|e| anyhow!("that mesh cannot be a trimesh collider: {e}")),
        "convex_hull" => ColliderBuilder2::convex_hull(&points)
            .ok_or_else(|| anyhow!("those {} points have no hull", points.len())),
        _ => {
            if points.len() < 2 {
                return Err(anyhow!(
                    "a polyline collider needs at least two points, not {}",
                    points.len()
                ));
            }
            Ok(ColliderBuilder2::polyline(points, None))
        }
    }
}

/// A 2D heightfield is one row of heights: a side-scroller's ground.
fn heightfield_collider(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder2> {
    let reference = params
        .get("heightfield")
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a heightfield collider2d needs a `heightfield` asset"))?;
    let field = balaur_core::assets::load_typed::<balaur_core::heightfield::HeightfieldData>(
        eng, reference,
    )?;
    Ok(ColliderBuilder2::heightfield(
        field.heights.iter().map(|h| scalar::real(*h)).collect(),
        scalar::v2a(v::vec2(params, "scale", [1.0, 1.0])),
    ))
}

/// The 2D half of `crate::collider::with_material`. The flag tables are
/// shared (`crate::vocabulary::flags`); only the types they are poured into
/// are per-dimension.
fn with_material(builder: ColliderBuilder2, params: &toml::Value) -> ColliderBuilder2 {
    let mut builder = builder
        .restitution(scalar::real(v::f(params, "restitution", 0.0)))
        .friction(scalar::real(v::f(params, "friction", 0.5)))
        .density(scalar::real(v::f(params, "density", 1.0).max(0.001)))
        .friction_combine_rule(combine_rule(v::text(params, "friction_combine", "average")))
        .restitution_combine_rule(combine_rule(v::text(
            params,
            "restitution_combine",
            "average",
        )))
        .contact_skin(scalar::real(v::f(params, "contact_skin", 0.0).max(0.0)))
        .contact_force_event_threshold(scalar::real(v::f(params, "contact_force_threshold", 0.0)))
        .collision_groups(InteractionGroups::new(
            Group::from_bits_truncate(v::layer_bits(params, "layers", false)),
            Group::from_bits_truncate(v::layer_bits(params, "mask", true)),
            InteractionTestMode::And,
        ))
        .solver_groups(InteractionGroups::new(
            Group::from_bits_truncate(v::layer_bits(params, "solver_layers", false)),
            Group::from_bits_truncate(v::layer_bits(params, "solver_mask", true)),
            InteractionTestMode::And,
        ))
        .active_collision_types(ActiveCollisionTypes::from_bits_truncate(v::bits(
            params,
            "active_collisions",
            &v::flags::collision_types(),
        )))
        .active_events(ActiveEvents::from_bits_truncate(v::bits(
            params,
            "events",
            &v::flags::events(),
        )))
        .active_hooks(active_hooks(params))
        .enabled(v::boolean(params, "enabled", true))
        .sensor(v::boolean(params, "sensor", false));
    let mass = scalar::real(v::f(params, "mass", 0.0));
    if mass > 0.0 {
        builder = builder.mass(mass);
    }
    builder
}

fn active_hooks(params: &toml::Value) -> ActiveHooks {
    let mut hooks = ActiveHooks::from_bits_truncate(v::bits(params, "hooks", &v::flags::hooks()));
    if v::boolean(params, "one_way", false) {
        hooks |= ActiveHooks::MODIFY_SOLVER_CONTACTS;
    }
    hooks
}

fn combine_rule(name: &str) -> CoefficientCombineRule {
    match name {
        "min" => CoefficientCombineRule::Min,
        "multiply" => CoefficientCombineRule::Multiply,
        "max" => CoefficientCombineRule::Max,
        "clamped_sum" => CoefficientCombineRule::ClampedSum,
        "geometric_mean" => CoefficientCombineRule::GeometricMean,
        _ => CoefficientCombineRule::Average,
    }
}

fn combine_name(rule: CoefficientCombineRule) -> &'static str {
    match rule {
        CoefficientCombineRule::Min => "min",
        CoefficientCombineRule::Multiply => "multiply",
        CoefficientCombineRule::Max => "max",
        CoefficientCombineRule::ClampedSum => "clamped_sum",
        CoefficientCombineRule::GeometricMean => "geometric_mean",
        CoefficientCombineRule::Average => "average",
    }
}

/// Build and insert the collider described by `params`, replacing any
/// existing one.
pub(crate) fn apply_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let builder = collider_builder(eng, params)?;
    let offset = Pose2::from_parts(
        scalar::v2a(v::vec2(params, "offset", [0.0; 2])),
        Rotation2::from_angle(scalar::real(v::f(params, "offset_rotation", 0.0))),
    );
    remove_colliders(eng, entity);
    add_collider_at(eng, entity, builder, offset)
}

pub(crate) fn get_collider_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handle = state.colliders.get(&entity)?.first()?;
    let collider = state.world.colliders.get(*handle)?;
    let mut map = shape_params(collider)?;
    read_material(collider, &mut map);
    Some(toml::Value::Table(map))
}

/// The shape half of a `collider2d`'s params. `None` for the asset-backed
/// kinds: rapier keeps the geometry, not the file it came from.
fn shape_params(collider: &Collider) -> Option<toml::map::Map<String, toml::Value>> {
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let vec2 = |x: Real, y: Real| toml::Value::Array(vec![f(x), f(y)]);
    let shape = collider.shape();
    let mut map = toml::map::Map::new();
    if let Some(ball) = shape.as_ball() {
        map.insert("kind".into(), "circle".into());
        map.insert("radius".into(), f(ball.radius));
        return Some(map);
    }
    if let Some(capsule) = shape.as_capsule() {
        map.insert("kind".into(), "capsule".into());
        map.insert("radius".into(), f(capsule.radius));
        let straight = (capsule.segment.b - capsule.segment.a).length();
        map.insert("height".into(), f(straight));
        return Some(map);
    }
    if let Some(cuboid) = shape.as_cuboid() {
        map.insert("kind".into(), "rect".into());
        let he = cuboid.half_extents;
        map.insert("half_extents".into(), vec2(he.x, he.y));
        return Some(map);
    }
    if let Some(round) = shape.as_round_cuboid() {
        map.insert("kind".into(), "rect".into());
        let he = round.inner_shape.half_extents;
        map.insert("half_extents".into(), vec2(he.x, he.y));
        map.insert("border".into(), f(round.border_radius));
        return Some(map);
    }
    if let Some(tri) = shape.as_triangle() {
        map.insert("kind".into(), "triangle".into());
        map.insert("a".into(), vec2(tri.a.x, tri.a.y));
        map.insert("b".into(), vec2(tri.b.x, tri.b.y));
        map.insert("c".into(), vec2(tri.c.x, tri.c.y));
        return Some(map);
    }
    if let Some(segment) = shape.as_segment() {
        map.insert("kind".into(), "segment".into());
        map.insert("a".into(), vec2(segment.a.x, segment.a.y));
        map.insert("b".into(), vec2(segment.b.x, segment.b.y));
        return Some(map);
    }
    if let Some(halfspace) = shape.as_halfspace() {
        map.insert("kind".into(), "halfspace".into());
        map.insert(
            "normal".into(),
            vec2(halfspace.normal.x, halfspace.normal.y),
        );
        return Some(map);
    }
    None
}

/// The 2D twin of `crate::collider::read_material`, property for property.
fn read_material(collider: &Collider, map: &mut toml::map::Map<String, toml::Value>) {
    let f = |value: Real| toml::Value::Float(f64::from(value));
    map.insert("restitution".into(), f(collider.restitution()));
    map.insert("friction".into(), f(collider.friction()));
    map.insert("density".into(), f(collider.density()));
    map.insert("mass".into(), f(collider.mass()));
    map.insert("contact_skin".into(), f(collider.contact_skin()));
    map.insert(
        "contact_force_threshold".into(),
        f(collider.contact_force_event_threshold()),
    );
    map.insert("sensor".into(), collider.is_sensor().into());
    map.insert("enabled".into(), collider.is_enabled().into());
    map.insert(
        "friction_combine".into(),
        combine_name(collider.friction_combine_rule()).into(),
    );
    map.insert(
        "restitution_combine".into(),
        combine_name(collider.restitution_combine_rule()).into(),
    );
    let groups = collider.collision_groups();
    map.insert("layers".into(), v::layer_names(groups.memberships.bits()));
    map.insert("mask".into(), v::layer_names(groups.filter.bits()));
    let solver = collider.solver_groups();
    map.insert(
        "solver_layers".into(),
        v::layer_names(solver.memberships.bits()),
    );
    map.insert("solver_mask".into(), v::layer_names(solver.filter.bits()));
    map.insert(
        "events".into(),
        v::names(collider.active_events().bits(), &v::flags::events()),
    );
    map.insert(
        "hooks".into(),
        v::names(collider.active_hooks().bits(), &v::flags::hooks()),
    );
    map.insert(
        "active_collisions".into(),
        v::names(
            collider.active_collision_types().bits(),
            &v::flags::collision_types(),
        ),
    );
}

/// Largest contact normal impulse currently applied to the node's colliders
/// (0 when untouched). Gameplay uses this for impact damage.
pub(crate) fn max_contact_impulse(eng: &Engine, entity: Entity) -> Real {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let Some(handles) = state.colliders.get(&entity) else {
        return 0.0;
    };
    let mut max: Real = 0.0;
    for &handle in handles {
        for pair in state.world.narrow_phase.contact_pairs_with(handle) {
            for manifold in &pair.manifolds {
                for point in &manifold.points {
                    max = max.max(point.data.impulse.abs());
                }
            }
        }
    }
    max
}

/// The `collider2d` key, backed by no component type: it writes into
/// [`crate::PhysicsState2d`].
pub(crate) fn register_collider2d_component(app: &mut App) {
    let schema = format!(
        r#"kind = {{ type = "enum", default = "rect", options = ["circle", "rect", "capsule", "triangle", "segment", "halfspace", "trimesh", "convex_hull", "polyline", "heightfield"], description = "Collision shape" }}
radius = {{ type = "float", default = 0.5, min = 0.01, description = "Circle radius, when kind is circle or capsule" }}
height = {{ type = "float", default = 1.0, min = 0.01, description = "Length along y of the straight part, when kind is capsule" }}
half_extents = {{ type = "vec2", default = [0.5, 0.5], description = "Half-sizes of the rect, when kind is rect" }}
border = {{ type = "float", default = 0.0, min = 0.0, description = "Rounds a rect or triangle by this radius, so it slides over seams instead of catching on them" }}
a = {{ type = "vec2", default = [0.0, 0.0], description = "First corner, when kind is triangle or segment" }}
b = {{ type = "vec2", default = [1.0, 0.0], description = "Second corner, when kind is triangle or segment" }}
c = {{ type = "vec2", default = [0.0, 1.0], description = "Third corner, when kind is triangle" }}
normal = {{ type = "vec2", default = [0.0, 1.0], description = "Which way the infinite line faces, when kind is halfspace" }}
mesh = {{ type = "asset", asset = "mesh", default = "", description = "Points and triangles for a trimesh, convex_hull or polyline collider: the same asset a polygon draws" }}
heightfield = {{ type = "asset", asset = "heightfield", default = "", description = "A row of heights, when kind is heightfield: a side-scroller's ground" }}
scale = {{ type = "vec2", default = [1.0, 1.0], description = "Width and height scale of a heightfield" }}
offset = {{ type = "vec2", default = [0.0, 0.0], description = "Where the shape sits relative to the node" }}
offset_rotation = {{ type = "float", default = 0.0, description = "How the shape is turned relative to the node, in radians" }}
one_way_axis = {{ type = "vec2", default = [0.0, 1.0], description = "The direction a one-way platform lets bodies through from" }}
{}"#,
        crate::collider::shared_collider_schema()
    );
    app.register_component(
        "collider2d",
        ComponentDef {
            doc: "The shape the node collides with in 2D. On a node with a `body2d` it is that body's shape; on a node without one it is immovable world geometry. A collider on a child node belongs to the nearest body above it, which is how one body carries several shapes.",
            schema: ComponentDef::parse_schema("collider2d", &schema),
            tags: &["2d", "physics"],
            expects: &[],
            apply: Box::new(apply_collider),
            remove: Box::new(|eng, entity| {
                remove_colliders(eng, entity);
                Ok(())
            }),
            get: Box::new(get_collider_params),
        },
    );
}
