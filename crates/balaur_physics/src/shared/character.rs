//! The character functions both dimensions share: rapier's controller from the
//! component, and the collision list a move reports.

macro_rules! functions {
    (
        state = $State:ty,
        vector = $Vector:ty,
        pose = $Pose:ty,
        value = $vec:ident,
        array = $a:ident,
        collider = $collider:expr
    ) => {
        /// Rapier's controller, built from the component every time it is used.
        ///
        /// Cheap — the struct is a dozen floats — and it means a script that changes
        /// `floor_max_angle` mid-game is obeyed on the next move rather than at the
        /// next scene load.
        pub(crate) fn controller_of(
            params: &toml::Value,
            up: $Vector,
        ) -> KinematicCharacterController {
            use crate::vocabulary::words as w;
            // Each length reads its own mode key: world units, or a fraction of
            // the character's own size.
            let length = |key: &str, mode: &str, default: f32| {
                let value = scalar::real(crate::vocabulary::f(params, key, default));
                if crate::vocabulary::text(params, mode, w::ABSOLUTE) == w::RELATIVE {
                    CharacterLength::Relative(value)
                } else {
                    CharacterLength::Absolute(value)
                }
            };
            let autostep_height = crate::vocabulary::f(params, k::STEP_HEIGHT, 0.3);
            KinematicCharacterController {
                up,
                offset: length(k::SAFE_MARGIN, k::SAFE_MARGIN_LENGTHS, 0.01),
                slide: crate::vocabulary::boolean(params, k::SLIDE, true),
                autostep: (autostep_height > 0.0).then(|| CharacterAutostep {
                    max_height: length(k::STEP_HEIGHT, k::STEP_HEIGHT_LENGTHS, 0.3),
                    min_width: length(k::STEP_MIN_WIDTH, k::STEP_MIN_WIDTH_LENGTHS, 0.2),
                    include_dynamic_bodies: crate::vocabulary::boolean(
                        params,
                        k::STEP_ON_DYNAMIC,
                        false,
                    ),
                }),
                max_slope_climb_angle: scalar::real(crate::vocabulary::f(
                    params,
                    k::FLOOR_MAX_ANGLE,
                    std::f32::consts::FRAC_PI_4,
                )),
                min_slope_slide_angle: scalar::real(crate::vocabulary::f(
                    params,
                    k::MIN_SLIDE_ANGLE,
                    std::f32::consts::FRAC_PI_6,
                )),
                snap_to_ground: {
                    let distance = crate::vocabulary::f(params, k::FLOOR_SNAP_LENGTH, 0.2);
                    (distance > 0.0)
                        .then(|| length(k::FLOOR_SNAP_LENGTH, k::FLOOR_SNAP_LENGTHS, 0.2))
                },
                normal_nudge_factor: scalar::real(crate::vocabulary::f(
                    params,
                    k::NORMAL_NUDGE,
                    0.0001,
                )),
            }
        }

        /// What a move sweeps: every solid collider of the character's body,
        /// or of its node when it has none, as one shape posed where the first
        /// of them stands; and the layers that first one is on.
        fn sweep_shape(
            state: &$State,
            entity: Entity,
        ) -> Result<(SharedShape, $Pose, InteractionGroups)> {
            let handles: Vec<ColliderHandle> = match state
                .bodies
                .get(&entity)
                .and_then(|handle| state.world.bodies.get(*handle))
            {
                Some(body) => body.colliders().to_vec(),
                None => state.colliders.get(&entity).cloned().unwrap_or_default(),
            };
            let solids: Vec<&Collider> = handles
                .iter()
                .filter_map(|handle| state.world.colliders.get(*handle))
                .filter(|collider| !collider.is_sensor())
                .collect();
            let first = solids
                .first()
                .ok_or_else(|| anyhow!("a character needs a {} that is not a sensor to move with", $collider))?;
            let (origin, groups) = (*first.position(), first.collision_groups());
            if solids.len() == 1 {
                return Ok((first.shared_shape().clone(), origin, groups));
            }
            let mut parts = Vec::with_capacity(solids.len());
            for collider in &solids {
                if collider.shape().as_composite_shape().is_some() {
                    return Err(anyhow!(
                        "a character with several colliders sweeps them as one compound, which \
                         cannot hold a mesh, a heightfield or another compound; give it simple shapes"
                    ));
                }
                parts.push((origin.inv_mul(collider.position()), collider.shared_shape().clone()));
            }
            Ok((SharedShape::compound(parts), origin, groups))
        }

        /// The mass a push is worked out with: `push_mass` when set, else the
        /// character's body, else its colliders.
        fn push_mass(state: &$State, entity: Entity, params: &toml::Value) -> Real {
            let asked = crate::vocabulary::f(params, k::PUSH_MASS, 0.0);
            if asked > 0.0 {
                return scalar::real(asked);
            }
            if let Some(body) = state
                .bodies
                .get(&entity)
                .and_then(|handle| state.world.bodies.get(*handle))
            {
                return body.mass();
            }
            state
                .colliders
                .get(&entity)
                .into_iter()
                .flatten()
                .filter_map(|handle| state.world.colliders.get(*handle))
                .map(Collider::mass)
                .sum()
        }

        /// The nodes whose colliders a move passes through: the character's own,
        /// and every node `ignore_nodes` names.
        fn ignored_nodes(eng: &Engine, entity: Entity, params: &toml::Value) -> Vec<Entity> {
            let mut out = vec![entity];
            if let Some(items) = params.get(k::IGNORE_NODES).and_then(toml::Value::as_array) {
                out.extend(
                    items.iter().filter_map(|item| {
                        balaur_core::components::as_node(eng, entity, Some(item))
                    }),
                );
            }
            out
        }

        /// The colliders a move passes through beyond `ignore`'s kinds: those
        /// of the nodes `ignored_nodes` answers, and every one on their bodies.
        struct Skipped {
            nodes: Vec<u64>,
            bodies: Vec<RigidBodyHandle>,
        }

        impl Skipped {
            fn of(nodes: &[Entity], state: &$State) -> Self {
                Self {
                    nodes: nodes.iter().map(|e| e.to_bits().get()).collect(),
                    bodies: nodes
                        .iter()
                        .filter_map(|e| state.bodies.get(e).copied())
                        .collect(),
                }
            }

            fn passes(&self, collider: &Collider) -> bool {
                !self.nodes.contains(&(collider.user_data as u64))
                    && !collider
                        .parent()
                        .is_some_and(|body| self.bodies.contains(&body))
            }
        }

        /// The kinds of collider `ignore` names, as rapier's query flags.
        fn ignore_flags(params: &toml::Value) -> QueryFilterFlags {
            let table = crate::vocabulary::flags::query_ignores();
            QueryFilterFlags::from_bits_truncate(crate::vocabulary::bits(params, k::IGNORE, &table))
        }

        /// What the character bumped into on the way, as nodes rather than handles.
        fn collision_list(eng: &Engine, collisions: &[CharacterCollision]) -> Value {
            let state = eng.resource::<$State>();
            let state = state.borrow();
            let mut out: Vec<(u64, Value)> = Vec::new();
            for collision in collisions {
                let Some(other) = state
                    .world
                    .colliders
                    .get(collision.handle)
                    .and_then(|collider| Entity::from_bits(collider.user_data as u64))
                else {
                    continue;
                };
                let hit = &collision.hit;
                let pose = collision.character_pos;
                let status = match hit.status {
                    ShapeCastStatus::Converged => crate::vocabulary::words::CONVERGED,
                    ShapeCastStatus::OutOfIterations => crate::vocabulary::words::OUT_OF_ITERATIONS,
                    ShapeCastStatus::Failed => crate::vocabulary::words::FAILED,
                    ShapeCastStatus::PenetratingOrWithinTargetDist => {
                        crate::vocabulary::words::PENETRATING
                    }
                };
                let vector = |v: $Vector| Value::$vec(scalar::$a(v));
                out.push((
                    other.to_bits().get(),
                    map([
                        (k::NODE, Value::Node(other.to_bits().get())),
                        // The obstacle's half is in world space already; the
                        // character's own half comes in its frame.
                        (k::POINT, vector(hit.witness1)),
                        (k::NORMAL, vector(hit.normal1)),
                        (k::OWN_POINT, vector(pose * hit.witness2)),
                        (k::OWN_NORMAL, vector(pose.rotation * hit.normal2)),
                        (k::POSITION, vector(pose.translation)),
                        (k::APPLIED, vector(collision.translation_applied)),
                        (k::REMAINING, vector(collision.translation_remaining)),
                        (
                            k::DISTANCE,
                            Value::Num(f64::from(scalar::f32_of(hit.time_of_impact))),
                        ),
                        (k::STATUS, Value::Str(status.into())),
                        (k::SUBSHAPE, Value::Int(i64::from(hit.subshape1))),
                    ]),
                ));
            }
            // Sorted, like every other list that crosses the seam.
            out.sort_by_key(|(bits, _)| *bits);
            Value::List(out.into_iter().map(|(_, value)| value).collect())
        }
    };
}
pub(crate) use functions;
