//! The character functions both dimensions share: rapier's controller from the
//! component, and the collision list a move reports.

macro_rules! functions {
    (state = $State:ty, vector = $Vector:ty, value = $vec:ident, array = $a:ident) => {
        /// Rapier's controller, built from the component every time it is used.
        ///
        /// Cheap — the struct is a dozen floats — and it means a script that changes
        /// `max_climb_angle` mid-game is obeyed on the next move rather than at the
        /// next scene load.
        pub(crate) fn controller_of(
            params: &toml::Value,
            up: $Vector,
        ) -> KinematicCharacterController {
            use crate::vocabulary::words as w;
            let relative = crate::vocabulary::text(params, "lengths", w::ABSOLUTE) == w::RELATIVE;
            let length = |value: f32| {
                let value = scalar::real(value);
                if relative {
                    CharacterLength::Relative(value)
                } else {
                    CharacterLength::Absolute(value)
                }
            };
            let autostep_height = crate::vocabulary::f(params, "autostep", 0.3);
            KinematicCharacterController {
                up,
                offset: length(crate::vocabulary::f(params, "offset", 0.01)),
                slide: crate::vocabulary::boolean(params, "slide", true),
                autostep: (autostep_height > 0.0).then(|| CharacterAutostep {
                    max_height: length(autostep_height),
                    min_width: length(crate::vocabulary::f(params, "autostep_min_width", 0.2)),
                    include_dynamic_bodies: crate::vocabulary::boolean(
                        params,
                        "autostep_dynamic",
                        false,
                    ),
                }),
                max_slope_climb_angle: scalar::real(
                    crate::vocabulary::f(params, "max_climb_angle", 45.0).to_radians(),
                ),
                min_slope_slide_angle: scalar::real(
                    crate::vocabulary::f(params, "min_slide_angle", 30.0).to_radians(),
                ),
                snap_to_ground: {
                    let distance = crate::vocabulary::f(params, "snap_to_ground", 0.2);
                    (distance > 0.0).then(|| length(distance))
                },
                normal_nudge_factor: scalar::real(crate::vocabulary::f(
                    params,
                    "normal_nudge",
                    0.0001,
                )),
            }
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
                let point = collision.hit.witness1;
                let normal = collision.hit.normal1;
                out.push((
                    other.to_bits().get(),
                    map([
                        ("node", Value::Node(other.to_bits().get())),
                        ("point", Value::$vec(scalar::$a(point))),
                        ("normal", Value::$vec(scalar::$a(normal))),
                        (
                            "remaining",
                            Value::$vec(scalar::$a(collision.translation_remaining)),
                        ),
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
