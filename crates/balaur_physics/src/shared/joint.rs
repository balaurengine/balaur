//! The joint functions both dimensions share: dropping and checking a joint in
//! either of rapier's two sets, the retry list, the break check, the `axes`
//! records, and the reader every joint call goes through.
//!
//! Each dimension defines `AXIS_WORDS` (the `axes` words and the degree of
//! freedom each names) and `free_axes` (what a kind leaves free) before
//! expanding this.

use crate::scalar::Real;

mod reaction;
mod schema;

pub(crate) use reaction::{Metric, Row, applied};
pub(crate) use schema::{DEFAULT_LINK_DAMPING, schema, softness};

/// One `axes` record, read with the schema's defaults.
pub(crate) struct AxisRecord<'a> {
    pub(crate) axis: &'a str,
    pub(crate) limits: [f32; 2],
    pub(crate) motor: &'a str,
    pub(crate) target: f32,
    pub(crate) target_velocity: f32,
    pub(crate) max_force: f32,
    pub(crate) model: &'a str,
    pub(crate) stiffness: f32,
    pub(crate) damping: f32,
    /// On an articulation; below zero is rapier's own for the axis.
    pub(crate) link_damping: f32,
    pub(crate) armature: f32,
    pub(crate) friction: f32,
}

impl<'a> AxisRecord<'a> {
    pub(crate) fn of(record: &'a toml::Value) -> Self {
        use crate::vocabulary::{self as v, keys as k, words as w};
        Self {
            axis: v::text(record, k::AXIS, ""),
            limits: v::vec2(record, k::LIMITS, [0.0; 2]),
            motor: v::text(record, k::MOTOR, w::OFF),
            target: v::f(record, k::MOTOR_TARGET, 0.0),
            target_velocity: v::f(record, k::MOTOR_TARGET_VELOCITY, 0.0),
            max_force: v::f(record, k::MOTOR_MAX_FORCE, 0.0),
            model: v::text(record, k::MOTOR_MODEL, w::AUTO),
            stiffness: v::f(record, k::STIFFNESS, 0.0),
            damping: v::f(record, k::DAMPING, DEFAULT_DAMPING),
            link_damping: v::f(record, k::LINK_DAMPING, -1.0),
            armature: v::f(record, k::ARMATURE, 0.0).max(0.0),
            friction: v::f(record, k::FRICTION, 0.0).max(0.0),
        }
    }

    /// The damping an articulation's chain takes on this axis: the record's,
    /// or rapier's own, which damps a turn and leaves a slide free.
    pub(crate) fn chain_damping(&self, angular: bool) -> f32 {
        if self.link_damping >= 0.0 {
            self.link_damping
        } else if angular {
            DEFAULT_LINK_DAMPING
        } else {
            0.0
        }
    }

    /// `limits = [0, 0]` is how a record says *no limit*: a range whose ends
    /// meet is a locked axis, and locking is what the `lock_*` flags are for.
    pub(crate) fn limited(&self) -> bool {
        self.limits[0] < self.limits[1]
    }

    pub(crate) fn motorised(&self) -> bool {
        self.motor != crate::vocabulary::words::OFF
    }
}

/// A motor's and a spring's damping when a record names none.
pub(crate) const DEFAULT_DAMPING: f32 = 1.0;

/// A record for `axis` with every other field at its default, for a script
/// call that names an axis no record holds yet.
pub(crate) fn default_record(axis: &str) -> toml::map::Map<String, toml::Value> {
    use crate::vocabulary::{keys as k, words as w};
    let mut out = toml::map::Map::new();
    let zero = || toml::Value::Float(0.0);
    out.insert(k::AXIS.into(), axis.into());
    out.insert(k::LIMITS.into(), toml::Value::Array(vec![zero(), zero()]));
    out.insert(k::MOTOR.into(), w::OFF.into());
    out.insert(k::MOTOR_TARGET.into(), zero());
    out.insert(k::MOTOR_TARGET_VELOCITY.into(), zero());
    out.insert(k::MOTOR_MAX_FORCE.into(), zero());
    out.insert(k::MOTOR_MODEL.into(), w::AUTO.into());
    out.insert(k::STIFFNESS.into(), zero());
    out.insert(
        k::DAMPING.into(),
        toml::Value::Float(f64::from(DEFAULT_DAMPING)),
    );
    out.insert(k::LINK_DAMPING.into(), toml::Value::Float(-1.0));
    out.insert(k::ARMATURE.into(), zero());
    out.insert(k::FRICTION.into(), zero());
    out
}

/// The `axes` records a joint's params hold.
pub(crate) fn records(params: &toml::Value) -> &[toml::Value] {
    params
        .get(crate::vocabulary::keys::AXES)
        .and_then(toml::Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// What a rope's or a spring's distance axis has another key for, refused so
/// the value has one spelling.
pub(crate) fn check_distance_axis(
    kind: &str,
    distance: bool,
    record: &AxisRecord<'_>,
) -> anyhow::Result<()> {
    use crate::vocabulary::words as w;
    if !distance {
        return Ok(());
    }
    if kind == w::ROPE && record.limited() {
        return Err(anyhow::anyhow!(
            "a rope's length limit is `max_length`; its `x` record takes no `limits`"
        ));
    }
    if kind == w::SPRING && record.motorised() {
        return Err(anyhow::anyhow!(
            "a spring's `x` axis is its spring: `rest_length` is where it pulls to and the \
             record's `stiffness`, `damping` and `motor_model` shape the pull, so its `motor` stays off"
        ));
    }
    Ok(())
}

/// `motor_model` read back as the model rapier runs, which is what `auto`
/// resolves to for this axis.
pub(crate) fn resolved_model(word: &str, spring: bool) -> &str {
    use crate::vocabulary::words as w;
    match word {
        w::AUTO if spring => w::FORCE,
        w::AUTO => w::ACCELERATION,
        other => other,
    }
}

/// The `axes` records with every `auto` model spelled as the model it runs.
pub(crate) fn resolved_axes(params: &toml::Value, spring: impl Fn(&str) -> bool) -> toml::Value {
    use crate::vocabulary::{keys as k, words as w};
    let rows = records(params)
        .iter()
        .map(|record| {
            let mut row = record.clone();
            let axis = crate::vocabulary::text(record, k::AXIS, "").to_string();
            if let Some(table) = row.as_table_mut() {
                let word = crate::vocabulary::text(record, k::MOTOR_MODEL, w::AUTO);
                let model = resolved_model(word, spring(&axis)).to_string();
                table.insert(k::MOTOR_MODEL.into(), model.into());
            }
            row
        })
        .collect();
    toml::Value::Array(rows)
}

/// `break_force` and `break_torque`, refused on an articulation: rapier
/// writes back no reaction for a multibody joint, so neither could fire.
pub(crate) fn thresholds(params: &toml::Value, reduced: bool) -> anyhow::Result<(Real, Real)> {
    use crate::vocabulary::{self as v, keys as k};
    let force = v::f(params, k::BREAK_FORCE, 0.0).max(0.0);
    let torque = v::f(params, k::BREAK_TORQUE, 0.0).max(0.0);
    if reduced && (force > 0.0 || torque > 0.0) {
        return Err(anyhow::anyhow!(
            "`break_force` and `break_torque` do not apply to an articulation: its constraint \
             leaves no reaction to measure"
        ));
    }
    Ok((crate::scalar::real(force), crate::scalar::real(torque)))
}

fn number(value: f32) -> toml::Value {
    toml::Value::Float(f64::from(value))
}

/// What `set_motor_velocity` writes into an axis's record.
pub(crate) fn velocity_motor(
    record: &mut toml::map::Map<String, toml::Value>,
    velocity: f32,
    damping: f32,
) {
    use crate::vocabulary::{keys as k, words as w};
    record.insert(k::MOTOR.into(), w::VELOCITY.into());
    record.insert(k::MOTOR_TARGET.into(), number(velocity));
    record.insert(k::MOTOR_TARGET_VELOCITY.into(), number(0.0));
    record.insert(k::DAMPING.into(), number(damping));
}

/// What `set_motor_position` writes, which as rapier's own call leaves no
/// target velocity.
pub(crate) fn position_motor(
    record: &mut toml::map::Map<String, toml::Value>,
    target: f32,
    stiffness: f32,
    damping: f32,
) {
    use crate::vocabulary::{keys as k, words as w};
    record.insert(k::MOTOR.into(), w::POSITION.into());
    record.insert(k::MOTOR_TARGET.into(), number(target));
    record.insert(k::MOTOR_TARGET_VELOCITY.into(), number(0.0));
    record.insert(k::STIFFNESS.into(), number(stiffness));
    record.insert(k::DAMPING.into(), number(damping));
}

/// What `set_joint_limits` writes.
pub(crate) fn limits(record: &mut toml::map::Map<String, toml::Value>, min: f32, max: f32) {
    use crate::vocabulary::keys as k;
    record.insert(
        k::LIMITS.into(),
        toml::Value::Array(vec![number(min), number(max)]),
    );
}

macro_rules! functions {
    (state = $State:ty, world = $World:ty, reference = $Ref:ty, handle = $Handle:ident, component = $component:expr, body = $body:expr, impulse_parts = $parts:ident) => {
        pub(crate) fn remove_joint(eng: &Engine, entity: Entity) {
            let chain = {
                let state = eng.resource::<$State>();
                let mut state = state.borrow_mut();
                state.joint_params.swap_remove(&entity);
                let reference = state.joints.swap_remove(&entity);
                if let Some(reference) = &reference {
                    drop_joint(&mut state.world, reference);
                }
                reference.is_some_and(|r| matches!(r.handle, $Handle::Multibody(_)))
            };
            if chain {
                rewrite_articulations(eng);
            }
        }

        /// Switch a made impulse joint on or off where it stands, when that is
        /// all a write changes; `false` when it has to be made again.
        fn toggled_in_place(eng: &Engine, entity: Entity, params: &toml::Value) -> bool {
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            let state = &mut *state;
            let Some(built) = state.joint_params.get(&entity) else {
                return false;
            };
            // Compared as made, not as written: a patch arrives with the read-back's
            // numbers and words, which spell the same joint another way.
            let (Ok(mut was), Ok(mut asked)) = (joint_of(built), joint_of(params)) else {
                return false;
            };
            was.set_enabled(true);
            asked.set_enabled(true);
            let same_text = |key: &str| v::text(built, key, "") == v::text(params, key, "");
            let same_number =
                |key: &str| v::f(built, key, 0.0).to_bits() == v::f(params, key, 0.0).to_bits();
            let same_switch =
                |key: &str| v::boolean(built, key, false) == v::boolean(params, key, false);
            if was != asked
                || !same_text(k::CONNECTED_BODY)
                || !same_number(k::BREAK_FORCE)
                || !same_number(k::BREAK_TORQUE)
                || !same_switch(k::ARTICULATION)
            {
                return false;
            }
            let Some($Handle::Impulse(handle)) = state.joints.get(&entity).map(|r| r.handle) else {
                return false;
            };
            let Some(joint) = state.world.impulse_joints.get_mut(handle, true) else {
                return false;
            };
            joint.data.set_enabled(v::boolean(params, k::ENABLED, true));
            state.joint_params.insert(entity, params.clone());
            true
        }

        /// Every articulation's per-axis numbers and gears, written again:
        /// rapier indexes them by degree of freedom across the whole chain, and
        /// a link joining or leaving moves the indices.
        fn rewrite_articulations(eng: &Engine) {
            let gears: Vec<(Entity, Entity)> = {
                let state = eng.resource::<$State>();
                let state = state.borrow();
                state
                    .joints
                    .iter()
                    .filter(|(_, r)| matches!(r.handle, $Handle::Multibody(_)))
                    .filter_map(|(entity, _)| {
                        let params = state.joint_params.get(entity)?;
                        let other = balaur_core::components::as_node(
                            eng,
                            *entity,
                            params.get(k::GEAR_WITH),
                        )?;
                        Some((*entity, other))
                    })
                    .collect()
            };
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            write_links(&mut state);
            write_gears(&mut state, gears);
        }

        /// Each articulation's damping, armature, friction and passive spring
        /// onto its link, and its chain's self-collision.
        fn write_links(state: &mut $State) {
            let chains: Vec<(Entity, MultibodyJointHandle)> = state
                .joints
                .iter()
                .filter_map(|(entity, r)| match r.handle {
                    $Handle::Multibody(handle) => Some((*entity, handle)),
                    $Handle::Impulse(_) => None,
                })
                .collect();
            for (_, handle) in &chains {
                if let Some((multibody, _)) = state.world.multibody_joints.get_mut(*handle) {
                    multibody.clear_dof_couplings();
                    multibody.set_self_contacts_enabled(true);
                }
            }
            for (entity, handle) in &chains {
                let Some(params) = state.joint_params.get(entity) else {
                    continue;
                };
                let real = |key: &str, default: f32| scalar::real(v::f(params, key, default));
                let Some((multibody, id)) = state.world.multibody_joints.get_mut(*handle) else {
                    continue;
                };
                if !v::boolean(params, k::SELF_COLLISION, true) {
                    multibody.set_self_contacts_enabled(false);
                }
                let Some(link) = multibody.link(id) else {
                    continue;
                };
                let start = link.assembly_id();
                let locked = link.joint().data.locked_axes.bits();
                // rapier numbers a link's degrees of freedom by its free axes, in order.
                let free = (0..AXIS_WORDS.len()).filter(|i| locked & (1 << i) == 0);
                let unwritten = toml::Value::Table(toml::map::Map::new());
                for (dof, i) in free.enumerate() {
                    let (word, axis) = AXIS_WORDS[i];
                    let row = crate::shared::joint::records(params)
                        .iter()
                        .find(|row| v::text(row, k::AXIS, "") == word);
                    let record = crate::shared::joint::AxisRecord::of(row.unwrap_or(&unwritten));
                    let angular = JointAxesMask::ANG_AXES.contains(axis.into());
                    multibody.damping_mut()[start + dof] =
                        scalar::real(record.chain_damping(angular));
                    multibody.armature_mut()[start + dof] = scalar::real(record.armature);
                    multibody.frictions_mut()[start + dof] = scalar::real(record.friction);
                }
                let Some(link) = multibody.link_mut(id) else {
                    continue;
                };
                let stiffness = real(k::PASSIVE_STIFFNESS, 0.0);
                // The chain holds the joint flipped, so its coordinates run the other way.
                let rest = -real(k::PASSIVE_REST, 0.0);
                let locked = link.joint.data.locked_axes.bits();
                for axis in (0..AXIS_WORDS.len()).filter(|i| locked & (1 << i) == 0) {
                    link.joint.set_spring(axis, stiffness, rest);
                }
            }
        }

        /// Each `gear_with` as a coupling on the chain both joints are in.
        fn write_gears(state: &mut $State, gears: Vec<(Entity, Entity)>) {
            for (entity, other) in gears {
                let handle = |e: &Entity| match state.joints.get(e).map(|r| r.handle) {
                    Some($Handle::Multibody(handle)) => Some(handle),
                    _ => None,
                };
                let (Some(mine), Some(theirs), Some(params)) = (
                    handle(&entity),
                    handle(&other),
                    state.joint_params.get(&entity),
                ) else {
                    continue;
                };
                let set = &mut state.world.multibody_joints;
                let first_free = |data: &GenericJoint| {
                    (0..AXIS_WORDS.len()).find(|i| data.locked_axes.bits() & (1 << i) == 0)
                };
                let (Some((chain1, link1)), Some((chain2, link2))) =
                    (set.get(theirs), set.get(mine))
                else {
                    continue;
                };
                // Both joints in one chain, each with an axis to turn.
                if !std::ptr::eq(chain1, chain2) {
                    continue;
                }
                let axes = (
                    chain1.link(link1).and_then(|l| first_free(&l.joint.data)),
                    chain2.link(link2).and_then(|l| first_free(&l.joint.data)),
                );
                let (Some(axis1), Some(axis2)) = axes else {
                    continue;
                };
                let coupling = MultibodyDofCoupling {
                    link1,
                    dof1: 0,
                    axis1,
                    link2,
                    dof2: 0,
                    axis2,
                    coeff: scalar::real(v::f(params, k::GEAR_RATIO, 1.0)),
                    // Both joints run flipped, which turns the offset's sign.
                    offset: -scalar::real(v::f(params, k::GEAR_OFFSET, 0.0)),
                };
                if let Some((chain, _)) = set.get_mut(mine) {
                    chain.add_dof_coupling(coupling);
                }
            }
        }

        /// What an articulation's chain holds for this joint, over the authored
        /// values: each `axes` record's damping, armature and friction, its
        /// first free axis's spring, whether it is kinematic, and the chain's
        /// self-collision.
        fn read_chain(
            map: &mut toml::map::Map<String, toml::Value>,
            multibody: &Multibody,
            id: usize,
        ) {
            let Some(link) = multibody.link(id) else {
                return;
            };
            let f = |value: Real| toml::Value::Float(f64::from(scalar::f32_of(value)));
            map.insert(k::KINEMATIC_LINK.into(), link.joint.kinematic.into());
            map.insert(
                k::SELF_COLLISION.into(),
                multibody.self_contacts_enabled().into(),
            );
            let locked = link.joint.data.locked_axes.bits();
            let free: Vec<usize> = (0..AXIS_WORDS.len())
                .filter(|i| locked & (1 << i) == 0)
                .collect();
            let start = link.assembly_id();
            if let Some(rows) = map.get_mut(k::AXES).and_then(toml::Value::as_array_mut) {
                for row in rows.iter_mut() {
                    let word = v::text(row, k::AXIS, "").to_string();
                    let dof = free.iter().position(|i| AXIS_WORDS[*i].0 == word);
                    let (Some(dof), Some(table)) = (dof, row.as_table_mut()) else {
                        continue;
                    };
                    table.insert(k::LINK_DAMPING.into(), f(multibody.damping()[start + dof]));
                    table.insert(k::ARMATURE.into(), f(multibody.armature()[start + dof]));
                    table.insert(k::FRICTION.into(), f(multibody.frictions()[start + dof]));
                }
            }
            let Some(&axis) = free.first() else {
                return;
            };
            let (stiffness, rest) = link.joint.spring(axis);
            map.insert(k::PASSIVE_STIFFNESS.into(), f(stiffness));
            map.insert(k::PASSIVE_REST.into(), f(-rest));
        }

        /// A made joint as `joint_state` reads it: its data with this node's end
        /// first, the bodies at each end, and on an articulation its link's
        /// coordinates and joint velocity.
        #[allow(clippy::type_complexity, reason = "one private reader's tuple")]
        fn joint_parts(
            world: &$World,
            handle: $Handle,
        ) -> Result<(
            GenericJoint,
            (RigidBodyHandle, Option<RigidBodyHandle>),
            Option<(Vec<Real>, Vec<Real>)>,
        )> {
            let gone = || anyhow!("this node's joint is gone");
            match handle {
                $Handle::Impulse(handle) => {
                    let joint = world.impulse_joints.get(handle).ok_or_else(gone)?;
                    Ok((joint.data, (joint.body1(), Some(joint.body2())), None))
                }
                $Handle::Multibody(handle) => {
                    let (multibody, id) = world.multibody_joints.get(handle).ok_or_else(gone)?;
                    let link = multibody.link(id).ok_or_else(gone)?;
                    let mut data = link.joint.data;
                    data.flip();
                    let parent = link
                        .parent_id()
                        .and_then(|p| multibody.link(p))
                        .map(|l| l.rigid_body_handle());
                    let coords = link.joint.coords();
                    let coords: Vec<Real> = (0..AXIS_WORDS.len()).map(|i| coords[i]).collect();
                    let velocity: Vec<Real> =
                        multibody.joint_velocity(link).iter().copied().collect();
                    Ok((
                        data,
                        (link.rigid_body_handle(), parent),
                        Some((coords, velocity)),
                    ))
                }
            }
        }

        /// What `joint_state` answers: whether the joint is solved, a hinge's
        /// angle, each free axis's limit and motor impulse, and on an
        /// articulation each free axis's coordinate and velocity.
        fn joint_state(eng: &Engine, node: NodeId) -> Result<balaur_script::Value> {
            use balaur_script::Value;
            let entity = entity_of(node)?;
            let state = eng.resource::<$State>();
            let state = state.borrow();
            let params = state
                .joint_params
                .get(&entity)
                .ok_or_else(|| anyhow!("node has no {}", $component))?;
            let kind = v::text(params, k::KIND, w::FIXED);
            let number = |x: Real| Value::Num(f64::from(scalar::f32_of(x)));
            let Some(reference) = state.joints.get(&entity) else {
                let status = if v::boolean(params, k::ENABLED, true) {
                    w::WAITING
                } else {
                    w::DISABLED
                };
                return Ok(crate::vocabulary::map([(
                    k::STATUS,
                    Value::Str(status.into()),
                )]));
            };
            let (data, bodies, chain) = joint_parts(&state.world, reference.handle)?;
            let status = match data.enabled {
                JointEnabled::Enabled => w::ENABLED,
                JointEnabled::Disabled => w::DISABLED,
                JointEnabled::DisabledByAttachedBody => w::BODY_DISABLED,
            };
            let mut out: Vec<(String, Value)> = vec![(k::STATUS.into(), Value::Str(status.into()))];
            let rotation =
                |body: RigidBodyHandle| state.world.bodies.get(body).map(|b| *b.rotation());
            if kind == w::HINGE
                && let (Some(rot1), Some(rot2)) = (rotation(bodies.0), bodies.1.and_then(rotation))
            {
                let angle = RevoluteJoint { data }.angle(&rot1, &rot2);
                out.push((k::ANGLE.into(), number(angle)));
            }
            let free = free_axes(kind, params);
            let per_axis = |pick: &dyn Fn(usize) -> Option<Real>| -> Value {
                Value::Map(
                    AXIS_WORDS
                        .iter()
                        .filter(|(_, axis)| free.contains((*axis).into()))
                        .filter_map(|(word, axis)| {
                            Some(((*word).to_string(), number(pick(*axis as usize)?)))
                        })
                        .collect(),
                )
            };
            let has = |mask: JointAxesMask, i: usize| mask.bits() & (1 << i) != 0;
            out.push((
                k::LIMIT_IMPULSES.into(),
                per_axis(&|i| has(data.limit_axes, i).then(|| data.limits[i].impulse)),
            ));
            out.push((
                k::MOTOR_IMPULSES.into(),
                per_axis(&|i| has(data.motor_axes, i).then(|| data.motors[i].impulse)),
            ));
            if let Some((coords, velocity)) = chain {
                // The chain holds the joint flipped: its coordinates run the other way.
                let free_dofs: Vec<usize> = (0..AXIS_WORDS.len())
                    .filter(|i| !has(data.locked_axes, *i))
                    .collect();
                out.push((
                    k::COORDINATES.into(),
                    per_axis(&|i| free_dofs.contains(&i).then(|| -coords[i])),
                ));
                out.push((
                    k::VELOCITIES.into(),
                    per_axis(&|i| {
                        let at = free_dofs.iter().position(|d| *d == i)?;
                        velocity.get(at).map(|v| -*v)
                    }),
                ));
            }
            Ok(Value::Map(out))
        }

        /// Whether rapier still holds this joint.
        ///
        /// It drops one when either end's body goes, and a map that kept the handle
        /// would report a joint that is not there and never retry it.
        pub(crate) fn is_live(world: &$World, reference: &$Ref) -> bool {
            match reference.handle {
                $Handle::Impulse(handle) => world.impulse_joints.get(handle).is_some(),
                $Handle::Multibody(handle) => world.multibody_joints.get(handle).is_some(),
            }
        }

        pub(crate) fn drop_joint(world: &mut $World, reference: &$Ref) {
            match reference.handle {
                $Handle::Impulse(handle) => {
                    world.remove_impulse_joint(handle);
                }
                $Handle::Multibody(handle) => world.remove_multibody_joint(handle),
            }
        }

        fn handles(
            state: &$State,
            a: Entity,
            b: Entity,
        ) -> Result<(RigidBodyHandle, RigidBodyHandle)> {
            let first = *state
                .bodies
                .get(&a)
                .ok_or_else(|| anyhow!("the node holding the joint has no {}", $body))?;
            let second = *state
                .bodies
                .get(&b)
                .ok_or_else(|| anyhow!("the node at the joint's other end has no {}", $body))?;
            Ok((first, second))
        }

        /// Joints authored but not yet made, because the node at the other end had
        /// not been spawned when this one was.
        ///
        /// A scene file names nodes in whatever order it likes, and a joint that
        /// pointed forwards used to be silently inert. Retried once per step, over
        /// the few that are unresolved rather than over every joint.
        pub fn pending(state: &$State, world: &balaur_core::hecs::World) -> Vec<Entity> {
            let out: Vec<Entity> = state
                .joint_params
                .iter()
                // An articulation switched off has params and no handle for ever;
                // retrying it every step would re-apply the whole component sixty
                // times a second.
                .filter(|(entity, params)| {
                    !state.joints.contains_key(*entity)
                        && (v::boolean(params, k::ENABLED, true)
                            || !v::boolean(params, k::ARTICULATION, false))
                })
                .map(|(entity, _)| *entity)
                .collect();
            in_id_order(world, out, "made")
        }

        /// The degree of freedom an `axes` word names, checked against the
        /// axes the joint's kind leaves free.
        fn free_axis(kind: &str, params: &toml::Value, word: &str) -> Result<JointAxis> {
            let words: Vec<&str> = AXIS_WORDS.iter().map(|(name, _)| *name).collect();
            let axis = AXIS_WORDS
                .iter()
                .find(|(name, _)| *name == word)
                .map(|(_, axis)| *axis)
                .ok_or_else(|| {
                    anyhow!("`{word}` is not a joint axis; one of {}", words.join(", "))
                })?;
            let free = free_axes(kind, params);
            if !free.contains(axis.into()) {
                let named: Vec<&str> = AXIS_WORDS
                    .iter()
                    .filter(|(_, axis)| free.contains((*axis).into()))
                    .map(|(name, _)| *name)
                    .collect();
                let free = if named.is_empty() {
                    "no axis".to_string()
                } else {
                    named.join(", ")
                };
                return Err(anyhow!(
                    "a `{kind}` joint leaves {free} free; an `axes` record or call names `{word}`"
                ));
            }
            Ok(axis)
        }

        /// The axis a rope or a spring measures its length along: rapier
        /// couples the linear axes, and the first stands for all of them.
        fn is_distance(kind: &str, axis: JointAxis) -> bool {
            axis == JointAxis::LinX && matches!(kind, w::ROPE | w::SPRING)
        }

        /// One record onto the joint: its limit and its motor, or none.
        fn write_axis(
            joint: &mut GenericJoint,
            kind: &str,
            rest_length: Real,
            axis: JointAxis,
            record: &crate::shared::joint::AxisRecord<'_>,
        ) {
            let bit: JointAxesMask = axis.into();
            let real = scalar::real;
            let distance = is_distance(kind, axis);
            // A rope's limit is its `max_length`, already on the axis.
            if !(distance && kind == w::ROPE) {
                if record.limited() {
                    joint.set_limits(axis, [real(record.limits[0]), real(record.limits[1])]);
                } else {
                    joint.limit_axes.remove(bit);
                }
            }
            let spring = distance && kind == w::SPRING;
            let (stiffness, damping) = (real(record.stiffness), real(record.damping));
            if spring {
                joint.set_motor(axis, rest_length, 0.0, stiffness, damping);
            } else {
                match record.motor {
                    w::VELOCITY => {
                        joint.set_motor(axis, 0.0, real(record.target), 0.0, damping);
                    }
                    w::POSITION => {
                        let (target, velocity) =
                            (real(record.target), real(record.target_velocity));
                        joint.set_motor(axis, target, velocity, stiffness, damping);
                    }
                    _ => joint.motor_axes.remove(bit),
                }
            }
            let model = match crate::shared::joint::resolved_model(record.model, spring) {
                w::FORCE => MotorModel::ForceBased,
                _ => MotorModel::AccelerationBased,
            };
            joint.set_motor_model(axis, model);
            let max_force = if record.max_force > 0.0 {
                real(record.max_force)
            } else {
                Real::MAX
            };
            joint.set_motor_max_force(axis, max_force);
        }

        /// Every `axes` record onto a freshly built joint.
        fn write_axes(joint: &mut GenericJoint, params: &toml::Value, kind: &str) -> Result<()> {
            let rest_length = scalar::real(v::f(params, k::REST_LENGTH, 0.0).max(0.0));
            let mut seen = JointAxesMask::empty();
            for entry in crate::shared::joint::records(params) {
                let record = crate::shared::joint::AxisRecord::of(entry);
                let axis = free_axis(kind, params, record.axis)?;
                if seen.contains(axis.into()) {
                    return Err(anyhow!("two `axes` records name `{}`", record.axis));
                }
                seen |= axis.into();
                crate::shared::joint::check_distance_axis(kind, is_distance(kind, axis), &record)?;
                write_axis(joint, kind, rest_length, axis, &record);
            }
            Ok(())
        }

        /// Rewrite one axis's record, in the params `get` reports and the next
        /// patch rebuilds from, and on the live joint.
        ///
        /// A joint still waiting for its other end takes the record too, and
        /// is made with it.
        fn set_axis(
            eng: &Engine,
            node: NodeId,
            word: &str,
            change: impl FnOnce(&mut toml::map::Map<String, toml::Value>),
        ) -> Result<()> {
            let entity = entity_of(node)?;
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            let state = &mut *state;
            let mut params = state
                .joint_params
                .get(&entity)
                .cloned()
                .ok_or_else(|| anyhow!("node has no {}", $component))?;
            let kind = v::text(&params, k::KIND, w::FIXED).to_string();
            let axis = free_axis(&kind, &params, word)?;
            let mut rows: Vec<toml::Value> = crate::shared::joint::records(&params).to_vec();
            let at = rows
                .iter()
                .position(|row| v::text(row, k::AXIS, "") == word)
                .unwrap_or_else(|| {
                    rows.push(toml::Value::Table(crate::shared::joint::default_record(
                        word,
                    )));
                    rows.len() - 1
                });
            if let Some(table) = rows[at].as_table_mut() {
                change(table);
            }
            let row = rows[at].clone();
            let record = crate::shared::joint::AxisRecord::of(&row);
            crate::shared::joint::check_distance_axis(&kind, is_distance(&kind, axis), &record)?;
            if let Some(table) = params.as_table_mut() {
                table.insert(k::AXES.into(), toml::Value::Array(rows));
            }
            let rest_length = scalar::real(v::f(&params, k::REST_LENGTH, 0.0).max(0.0));
            state.joint_params.insert(entity, params);
            let Some(reference) = state.joints.get(&entity).copied() else {
                return Ok(());
            };
            match reference.handle {
                $Handle::Impulse(handle) => {
                    if let Some(joint) = state.world.impulse_joints.get_mut(handle, true) {
                        write_axis(&mut joint.data, &kind, rest_length, axis, &record);
                    }
                }
                $Handle::Multibody(handle) => {
                    let Some((multibody, link)) = state.world.multibody_joints.get_mut(handle)
                    else {
                        return Ok(());
                    };
                    let Some(link) = multibody.link_mut(link) else {
                        return Ok(());
                    };
                    // An articulation holds its frames flipped (see `apply_joint`).
                    link.joint.data.flip();
                    write_axis(&mut link.joint.data, &kind, rest_length, axis, &record);
                    link.joint.data.flip();
                    let body = link.rigid_body_handle();
                    if let Some(body) = state.world.bodies.get_mut(body) {
                        body.wake_up(true);
                    }
                }
            }
            Ok(())
        }

        /// The force and the torque an impulse joint held through the last
        /// step, as `break_force` and `break_torque` measure them: the torque
        /// about the joint's anchor.
        ///
        /// Rapier writes back the last substep's impulse, so it is divided by
        /// the substep rather than by the whole step.
        fn reaction(world: &$World, handle: ImpulseJointHandle) -> Option<(Real, Real)> {
            let joint = world.impulse_joints.get(handle)?;
            let extra = [joint.body1(), joint.body2()]
                .into_iter()
                .filter_map(|body| world.bodies.get(body))
                .map(|body| body.additional_solver_iterations())
                .max()
                .unwrap_or(0);
            let params = &world.integration_parameters;
            let substeps = (params.num_solver_iterations + extra).max(1) as Real;
            let dt = params.dt / substeps;
            if dt <= 0.0 {
                return Some((0.0, 0.0));
            }
            let (linear, angular) = $parts(world, joint)?;
            Some((linear / dt, angular / dt))
        }

        /// What `joint_force` answers: zeros for a joint not made yet, and an
        /// error for an articulation, whose constraint leaves no force to read.
        fn joint_force(eng: &Engine, node: NodeId) -> Result<balaur_script::Value> {
            use balaur_script::Value;
            let entity = entity_of(node)?;
            let state = eng.resource::<$State>();
            let state = state.borrow();
            let (force, torque) = match state.joints.get(&entity).map(|j| j.handle) {
                Some($Handle::Impulse(handle)) => {
                    reaction(&state.world, handle).unwrap_or((0.0, 0.0))
                }
                Some($Handle::Multibody(_)) => {
                    return Err(anyhow!(
                        "an articulation has no reaction force to read: its constraint is built \
                         into the chain's coordinates"
                    ));
                }
                None => (0.0, 0.0),
            };
            Ok(crate::vocabulary::map([
                (k::FORCE, Value::Num(f64::from(scalar::f32_of(force)))),
                (k::TORQUE, Value::Num(f64::from(scalar::f32_of(torque)))),
            ]))
        }

        /// What a joint giving way says: the body at each end and the force
        /// and torque it broke at, read before the joint is removed.
        pub(crate) fn break_payload(state: &$State, entity: Entity) -> balaur_script::Value {
            use balaur_script::Value;
            let mut out: Vec<(String, Value)> = Vec::new();
            let handle = state
                .joints
                .get(&entity)
                .and_then(|reference| match reference.handle {
                    $Handle::Impulse(handle) => Some(handle),
                    $Handle::Multibody(_) => None,
                });
            if let Some(handle) = handle
                && let Some(joint) = state.world.impulse_joints.get(handle)
            {
                let owner = |body| {
                    state
                        .bodies
                        .iter()
                        .find(|(_, handle)| **handle == body)
                        .map(|(e, _)| Value::Node(e.to_bits().get()))
                };
                if let Some(a) = owner(joint.body1()) {
                    out.push((k::A.into(), a));
                }
                if let Some(b) = owner(joint.body2()) {
                    out.push((k::B.into(), b));
                }
                let (force, torque) = reaction(&state.world, handle).unwrap_or((0.0, 0.0));
                let number = |value: Real| Value::Num(f64::from(scalar::f32_of(value)));
                out.push((k::FORCE.into(), number(force)));
                out.push((k::TORQUE.into(), number(torque)));
            }
            Value::Map(out.into_iter().map(|(key, v)| (key.into(), v)).collect())
        }

        /// `joints` in the order both runs of a session agree on. A joint on a
        /// node with no id is refused, loudly: nothing could order it the same
        /// way twice.
        fn in_id_order(
            world: &balaur_core::hecs::World,
            joints: Vec<Entity>,
            what: &str,
        ) -> Vec<Entity> {
            let mut keyed: Vec<(String, Entity)> = joints
                .into_iter()
                .filter_map(|e| match balaur_core::ids::order_key(world, e) {
                    Ok(key) => Some((key, e)),
                    Err(why) => {
                        tracing::error!("{why:#}: its {} is not {what}", $component);
                        None
                    }
                })
                .collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            keyed.into_iter().map(|(_, e)| e).collect()
        }

        /// Joints whose force passed `break_force`, or whose torque passed
        /// `break_torque`, this step.
        ///
        /// Checked after the step with the world still borrowed, and acted on after it
        /// is released — the same shape as an event, because a break *is* one.
        pub(crate) fn broken(state: &$State, world: &balaur_core::hecs::World) -> Vec<Entity> {
            let mut out = Vec::new();
            for (entity, reference) in &state.joints {
                if reference.break_force <= 0.0 && reference.break_torque <= 0.0 {
                    continue;
                }
                // Refused at apply: an articulation has no reaction to read.
                let $Handle::Impulse(handle) = reference.handle else {
                    continue;
                };
                let Some((force, torque)) = reaction(&state.world, handle) else {
                    continue;
                };
                let snapped = reference.break_force > 0.0 && force > reference.break_force;
                let twisted = reference.break_torque > 0.0 && torque > reference.break_torque;
                if snapped || twisted {
                    out.push(*entity);
                }
            }
            in_id_order(world, out, "broken")
        }

        /// The `solve_ik` options, as rapier's solver takes them.
        ///
        /// `constrain` holds the world axes the end link must match; it
        /// defaults to the translation, and to every axis when `rotation` is
        /// given, so a position target leaves the end link's turn free.
        fn ik_options(
            opts: &crate::vocabulary::Opts<'_>,
            rotation: bool,
        ) -> Result<InverseKinematicsOption> {
            let defaults = InverseKinematicsOption::default();
            let constrained_axes = match opts.list(k::CONSTRAIN) {
                Some(words) => {
                    let mut mask = JointAxesMask::empty();
                    for word in words {
                        let balaur_script::Value::Str(word) = word else {
                            return Err(anyhow!("`constrain` holds axis words"));
                        };
                        let axis = AXIS_WORDS
                            .iter()
                            .find(|(name, _)| *name == word.as_str())
                            .map(|(_, axis)| *axis)
                            .ok_or_else(|| anyhow!("`{word}` is not an axis `constrain` takes"))?;
                        mask |= axis.into();
                    }
                    mask
                }
                None if rotation => JointAxesMask::LIN_AXES | JointAxesMask::ANG_AXES,
                None => JointAxesMask::LIN_AXES,
            };
            let iterations = opts.f32(k::ITERATIONS, defaults.max_iters as f32).max(0.0);
            let tolerance = opts.f32(k::TOLERANCE, scalar::f32_of(defaults.epsilon_linear));
            let tolerance = scalar::real(tolerance);
            Ok(InverseKinematicsOption {
                damping: scalar::real(opts.f32(k::DAMPING, scalar::f32_of(defaults.damping))),
                max_iters: iterations as usize,
                constrained_axes,
                epsilon_linear: tolerance,
                epsilon_angular: tolerance,
            })
        }
    };
}
pub(crate) use functions;
