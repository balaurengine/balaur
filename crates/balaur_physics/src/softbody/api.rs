//! What a script may ask a 3D soft body and do to it.

use anyhow::{Result, anyhow};
use balaur_core::{Engine, entity_of};
use balaur_script::{Module, NodeId};

use super::install_runtime;
use crate::PhysicsState3d;
use crate::scalar::{self, Real, Vector};
use crate::shared::softbody::Family;
use crate::vocabulary::{component as c, keys as k};

/// What a script may ask a soft body and do to it; the calls on particles,
/// forces and edges are `install_runtime`.
pub(crate) fn install_softbody_api(m: &mut Module<Engine>) {
    m.describe(&[
        ("softbody_volume", &[c::SOFTBODY_3D], "", "How much space the body and every piece torn off it enclose right now, against `softbody_rest_volume` for how far it is squeezed."),
        ("softbody_rest_volume", &[c::SOFTBODY_3D], "", "How much they enclose at rest."),
        ("softbody_center", &[c::SOFTBODY_3D], "", "The centre of mass of the body and its pieces, which is where it is when a deformable body has no one position."),
        ("softbody_dihedrals", &[c::SOFTBODY_3D], "() -> [[int]]", "Every bending hinge between two surface triangles, as four particle indices: the shared edge, then the two opposite particles."),
        ("softbody_dihedral", &[c::SOFTBODY_3D], "(index: int) -> map", "Everything one hinge holds: `{ particles, rest_angle, initial_rest_angle, plastic_set }`, angles in radians; `plastic_set` is how far its rest angle has flowed from the one it was built with."),
    ]);
    m.function("softbody_volume", |eng: &Engine, node: NodeId| {
        with_family(eng, node, |bodies| {
            Ok(bodies.iter().map(|(b, _)| b.volume()).sum::<f32>())
        })
    });
    m.function("softbody_rest_volume", |eng: &Engine, node: NodeId| {
        with_family(eng, node, |bodies| {
            Ok(bodies.iter().map(|(b, _)| b.rest_volume()).sum::<f32>())
        })
    });
    m.function("softbody_center", |eng: &Engine, node: NodeId| {
        with_family(eng, node, |bodies| {
            let mass: Real = bodies.iter().map(|(b, _)| b.mass()).sum();
            let weighted: Vector = bodies
                .iter()
                .map(|(b, _)| b.center_of_mass() * b.mass())
                .sum();
            let center = if mass > 0.0 {
                weighted / mass
            } else {
                Vector::ZERO
            };
            Ok(balaur_script::Value::Vec3(scalar::a3(center)))
        })
    });
    m.function("softbody_dihedrals", |eng: &Engine, node: NodeId| {
        with_family(eng, node, |bodies| {
            let mut out = Vec::new();
            for (body, offset) in bodies {
                for hinge in body.dihedrals() {
                    let at = |i: u32| {
                        balaur_script::Value::Int(
                            i64::from(i) + i64::try_from(*offset).unwrap_or(i64::MAX),
                        )
                    };
                    out.push(balaur_script::Value::List(hinge.vertices.map(at).to_vec()));
                }
            }
            Ok(balaur_script::Value::List(out))
        })
    });
    m.function(
        "softbody_dihedral",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_family(eng, node, |bodies| {
                let mut left = usize::try_from(index)
                    .map_err(|_| anyhow!("this body has no dihedral {index}"))?;
                for (body, offset) in bodies {
                    let Some(hinge) = body.dihedrals().get(left) else {
                        left -= body.dihedrals().len();
                        continue;
                    };
                    let number = |x: Real| balaur_script::Value::Num(f64::from(x));
                    let at = |i: u32| {
                        balaur_script::Value::Int(
                            i64::from(i) + i64::try_from(*offset).unwrap_or(i64::MAX),
                        )
                    };
                    return Ok(balaur_script::Value::Map(vec![
                        (
                            k::PARTICLES.into(),
                            balaur_script::Value::List(hinge.vertices.map(at).to_vec()),
                        ),
                        (k::REST_ANGLE.into(), number(hinge.rest_angle)),
                        (
                            k::INITIAL_REST_ANGLE.into(),
                            number(hinge.initial_rest_angle()),
                        ),
                        (k::PLASTIC_SET.into(), number(hinge.plastic_set())),
                    ]));
                }
                Err(anyhow!("this body has no dihedral {index}"))
            })
        },
    );

    install_runtime(m);
}

/// Run `f` over the node's body and every piece torn off it, each with the
/// family-wide index of its first particle.
fn with_family<T>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&[(&crate::rapier3d::prelude::SoftBody, usize)]) -> Result<T>,
) -> Result<T> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    let handle = *state
        .soft_bodies
        .get(&entity)
        .ok_or_else(|| anyhow!("node has no soft body"))?;
    let set = &state.world.soft_bodies;
    let mut offset = 0;
    let mut bodies = Vec::new();
    for piece in set.family(handle) {
        if let Some(body) = set.get(piece) {
            bodies.push((body, offset));
            offset += body.num_particles();
        }
    }
    anyhow::ensure!(!bodies.is_empty(), "node has no soft body");
    f(&bodies)
}
